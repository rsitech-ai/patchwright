use crate::{EventStore, GitHubWorkItem, WorkItemKind};
use chrono::{DateTime, Utc};
use patchwright_core::{
    Capability, GitHubIssueSourceInput, GitHubPullRequestSourceInput, RepositoryBindingId,
    RiskClass, Task, TaskContract, TaskContractDraft, TaskSource,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConversionRequest {
    pub repository_full_name: String,
    pub item_number: u64,
    pub expected_updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConversionPreview {
    pub repository_full_name: String,
    pub repository_id: u64,
    pub repository_binding_id: RepositoryBindingId,
    pub item_number: u64,
    pub source_kind: WorkItemKind,
    pub title: String,
    pub goal: String,
    pub acceptance_criteria: Vec<String>,
    pub repository_path: String,
    pub base_sha: Option<String>,
    pub head_sha: Option<String>,
    pub source_updated_at: DateTime<Utc>,
    pub snapshot_at: DateTime<Utc>,
    pub requires_confirmation: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConversionOutcome {
    pub preview: ConversionPreview,
    pub task: Task,
    pub created: bool,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ConversionError {
    #[error("repository snapshot is missing")]
    SnapshotMissing,
    #[error("GitHub item is missing from the repository snapshot")]
    ItemMissing,
    #[error("GitHub item changed after it was selected; refresh before converting")]
    SnapshotStale,
    #[error("repository binding is required before conversion")]
    RepositoryBindingMissing,
    #[error("repository binding does not match the ingested repository")]
    RepositoryBindingMismatch,
    #[error("pull request source identity is incomplete")]
    IncompletePullRequest,
    #[error("repository default-branch source identity is incomplete")]
    IncompleteRepository,
    #[error("pull request head fork cannot be updated by this installation")]
    ForkInaccessible,
    #[error("conversion request is invalid: {0}")]
    InvalidRequest(&'static str),
    #[error("conversion contract is invalid: {0}")]
    InvalidContract(String),
    #[error("conversion persistence failed: {0}")]
    Persistence(String),
}

pub struct TaskConversionService<'store> {
    store: &'store EventStore,
}

impl<'store> TaskConversionService<'store> {
    #[must_use]
    pub const fn new(store: &'store EventStore) -> Self {
        Self { store }
    }

    pub fn preview(
        &self,
        request: ConversionRequest,
    ) -> Result<ConversionPreview, ConversionError> {
        validate_request(&request)?;
        let (snapshot, snapshot_at) = self
            .store
            .github_repository_with_snapshot_at(&request.repository_full_name)
            .map_err(persistence)?
            .ok_or(ConversionError::SnapshotMissing)?;
        let item = snapshot
            .work_items
            .iter()
            .find(|item| item.number == request.item_number)
            .ok_or(ConversionError::ItemMissing)?;
        let expected_updated_at =
            parse_timestamp(&request.expected_updated_at, "expectedUpdatedAt")?;
        let source_updated_at = parse_timestamp(&item.updated_at, "sourceUpdatedAt")?;
        if source_updated_at != expected_updated_at {
            return Err(ConversionError::SnapshotStale);
        }
        let binding = self
            .store
            .repository_binding_by_full_name(&request.repository_full_name)
            .map_err(persistence)?
            .ok_or(ConversionError::RepositoryBindingMissing)?;
        if binding.github_repository_id() != snapshot.repository.id
            || binding.full_name() != snapshot.repository.full_name
        {
            return Err(ConversionError::RepositoryBindingMismatch);
        }
        validate_pull_request_access(item, &snapshot.repository.full_name)?;
        let repository_path = binding
            .managed_clone()
            .or_else(|| binding.user_checkout())
            .unwrap_or_else(|| binding.state_root())
            .to_owned();
        let base_sha = match item.kind {
            WorkItemKind::Issue => Some(
                snapshot
                    .repository
                    .default_branch_sha
                    .clone()
                    .ok_or(ConversionError::IncompleteRepository)?,
            ),
            WorkItemKind::PullRequest => item.base_sha.clone(),
        };
        let goal = match item.kind {
            WorkItemKind::Issue => format!(
                "Resolve GitHub issue #{} in {}: {}",
                item.number, snapshot.repository.full_name, item.title
            ),
            WorkItemKind::PullRequest => format!(
                "Complete GitHub pull request #{} in {}: {}",
                item.number, snapshot.repository.full_name, item.title
            ),
        };
        let acceptance_criteria = match item.kind {
            WorkItemKind::Issue => vec![
                "The issue outcome is implemented and verified against the captured source snapshot."
                    .into(),
            ],
            WorkItemKind::PullRequest => vec![
                "Requested changes, review feedback, checks, and conflicts are assessed against the captured pull request head."
                    .into(),
            ],
        };
        Ok(ConversionPreview {
            repository_full_name: snapshot.repository.full_name,
            repository_id: snapshot.repository.id,
            repository_binding_id: binding.id(),
            item_number: item.number,
            source_kind: item.kind,
            title: item.title.clone(),
            goal,
            acceptance_criteria,
            repository_path,
            base_sha,
            head_sha: item.head_sha.clone(),
            source_updated_at,
            snapshot_at,
            requires_confirmation: true,
        })
    }

    pub fn create(&self, request: ConversionRequest) -> Result<ConversionOutcome, ConversionError> {
        let preview = self.preview(request.clone())?;
        let snapshot = self
            .store
            .github_repository(&request.repository_full_name)
            .map_err(persistence)?
            .ok_or(ConversionError::SnapshotMissing)?;
        let item = snapshot
            .work_items
            .iter()
            .find(|item| item.number == request.item_number)
            .ok_or(ConversionError::ItemMissing)?;
        let source = task_source(item, preview.repository_id, preview.snapshot_at)?;
        let mut task = Task::new(&preview.title, &preview.repository_path)
            .map_err(|error| ConversionError::InvalidContract(error.to_string()))?;
        task.source = source.clone();
        task.repository_binding_id = Some(preview.repository_binding_id);
        task.contract_version = 1;
        let contract = TaskContract::try_from(TaskContractDraft {
            task_id: task.id,
            source,
            repository_binding_id: preview.repository_binding_id,
            goal: preview.goal.clone(),
            acceptance_criteria: preview.acceptance_criteria.clone(),
            base_sha: preview.base_sha.clone(),
            head_sha: preview.head_sha.clone(),
            instruction_digests: vec![],
            verification_commands: vec![],
            required_capabilities: capabilities_for(item.kind),
            risk: RiskClass::Moderate,
            sensitive_paths: vec![],
            dependencies: vec![],
        })
        .map_err(|error| ConversionError::InvalidContract(error.to_string()))?;
        let source_key = format!(
            "github:{}:{}:{}",
            preview.repository_id,
            source_kind_key(preview.source_kind),
            preview.item_number
        );
        let (task, created) = self
            .store
            .create_converted_task(&task, &contract, &source_key)
            .map_err(persistence)?;
        Ok(ConversionOutcome {
            preview,
            task,
            created,
        })
    }
}

fn capabilities_for(kind: WorkItemKind) -> Vec<Capability> {
    match kind {
        WorkItemKind::Issue => vec![
            Capability::CreateBranch,
            Capability::PushBranch,
            Capability::CreatePullRequest,
            Capability::PostComment,
            Capability::CreateCheckRun,
        ],
        WorkItemKind::PullRequest => vec![
            Capability::PushBranch,
            Capability::PostComment,
            Capability::PostReview,
            Capability::CreateCheckRun,
            Capability::UpdatePullRequestBranch,
            Capability::ClosePullRequest,
            Capability::EnqueuePullRequest,
            Capability::MergePullRequest,
        ],
    }
}

fn task_source(
    item: &GitHubWorkItem,
    repository_id: u64,
    snapshot_at: DateTime<Utc>,
) -> Result<TaskSource, ConversionError> {
    match item.kind {
        WorkItemKind::Issue => TaskSource::github_issue(GitHubIssueSourceInput {
            repository_id,
            repository_full_name: item.repository_full_name.clone(),
            number: item.number,
            html_url: item.html_url.clone(),
            snapshot_at,
        }),
        WorkItemKind::PullRequest => {
            TaskSource::github_pull_request(GitHubPullRequestSourceInput {
                repository_id,
                repository_full_name: item.repository_full_name.clone(),
                number: item.number,
                html_url: item.html_url.clone(),
                snapshot_at,
                base_ref: item
                    .base_ref
                    .clone()
                    .ok_or(ConversionError::IncompletePullRequest)?,
                base_sha: item
                    .base_sha
                    .clone()
                    .ok_or(ConversionError::IncompletePullRequest)?,
                head_ref: item
                    .head_ref
                    .clone()
                    .ok_or(ConversionError::IncompletePullRequest)?,
                head_sha: item
                    .head_sha
                    .clone()
                    .ok_or(ConversionError::IncompletePullRequest)?,
            })
        }
    }
    .map_err(|error| ConversionError::InvalidContract(error.to_string()))
}

fn validate_pull_request_access(
    item: &GitHubWorkItem,
    base_repository: &str,
) -> Result<(), ConversionError> {
    if item.kind != WorkItemKind::PullRequest {
        return Ok(());
    }
    if item.base_ref.is_none()
        || item.base_sha.is_none()
        || item.head_ref.is_none()
        || item.head_sha.is_none()
        || item.head_repository_full_name.is_none()
    {
        return Err(ConversionError::IncompletePullRequest);
    }
    let different_repository = item.head_repository_full_name.as_deref() != Some(base_repository);
    if item.head_repository_fork && different_repository && !item.maintainer_can_modify {
        return Err(ConversionError::ForkInaccessible);
    }
    Ok(())
}

fn validate_request(request: &ConversionRequest) -> Result<(), ConversionError> {
    if request.repository_full_name.is_empty() || request.repository_full_name.len() > 200 {
        return Err(ConversionError::InvalidRequest("repositoryFullName"));
    }
    if request.item_number == 0 {
        return Err(ConversionError::InvalidRequest("itemNumber"));
    }
    if DateTime::parse_from_rfc3339(&request.expected_updated_at).is_err() {
        return Err(ConversionError::InvalidRequest("expectedUpdatedAt"));
    }
    Ok(())
}

fn parse_timestamp(value: &str, field: &'static str) -> Result<DateTime<Utc>, ConversionError> {
    DateTime::parse_from_rfc3339(value)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(|_| ConversionError::InvalidRequest(field))
}

const fn source_kind_key(kind: WorkItemKind) -> &'static str {
    match kind {
        WorkItemKind::Issue => "issue",
        WorkItemKind::PullRequest => "pullRequest",
    }
}

fn persistence(error: anyhow::Error) -> ConversionError {
    ConversionError::Persistence(error.to_string())
}
