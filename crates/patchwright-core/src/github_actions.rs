#![allow(clippy::missing_errors_doc)]

use crate::Capability;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

const MAX_BODY_BYTES: usize = 65_536;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ReviewEvent {
    Approve,
    RequestChanges,
    Comment,
    Pending,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MergeMethod {
    Merge,
    Squash,
    Rebase,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum GitHubAction {
    CreateBranch {
        branch: String,
        #[serde(alias = "from_sha")]
        from_sha: String,
    },
    PushIntent {
        branch: String,
        #[serde(alias = "head_sha")]
        head_sha: String,
    },
    Comment {
        #[serde(alias = "issue_number")]
        issue_number: u64,
        body: String,
    },
    Review {
        #[serde(alias = "pull_request_number")]
        pull_request_number: u64,
        #[serde(alias = "expected_head_sha")]
        expected_head_sha: String,
        event: ReviewEvent,
        body: String,
        #[serde(alias = "inline_comments")]
        inline_comments: Vec<InlineReviewComment>,
    },
    ResolveReviewThread {
        #[serde(alias = "pull_request_number")]
        pull_request_number: u64,
        #[serde(alias = "thread_id")]
        thread_id: String,
        #[serde(alias = "expected_head_sha")]
        expected_head_sha: String,
    },
    CheckRun {
        name: String,
        #[serde(alias = "head_sha")]
        head_sha: String,
        status: String,
        conclusion: Option<String>,
    },
    DraftPullRequest {
        title: String,
        head: String,
        base: String,
        body: String,
    },
    UpdatePullRequestBranch {
        #[serde(alias = "pull_request_number")]
        pull_request_number: u64,
        #[serde(alias = "expected_head_sha")]
        expected_head_sha: String,
    },
    ReadyPullRequest {
        #[serde(alias = "pull_request_number")]
        pull_request_number: u64,
        #[serde(alias = "expected_head_sha")]
        expected_head_sha: String,
    },
    ClosePullRequest {
        #[serde(alias = "pull_request_number")]
        pull_request_number: u64,
    },
    CloseIssue {
        #[serde(alias = "issue_number")]
        issue_number: u64,
    },
    EnqueuePullRequest {
        #[serde(alias = "pull_request_number")]
        pull_request_number: u64,
        #[serde(alias = "expected_head_sha")]
        expected_head_sha: String,
    },
    MergePullRequest {
        #[serde(alias = "pull_request_number")]
        pull_request_number: u64,
        #[serde(alias = "expected_head_sha")]
        expected_head_sha: String,
        method: MergeMethod,
    },
}

#[derive(Deserialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
enum GitHubActionWire {
    CreateBranch {
        branch: String,
        #[serde(alias = "from_sha")]
        from_sha: String,
    },
    PushIntent {
        branch: String,
        #[serde(alias = "head_sha")]
        head_sha: String,
    },
    Comment {
        #[serde(alias = "issue_number")]
        issue_number: u64,
        body: String,
    },
    Review {
        #[serde(alias = "pull_request_number")]
        pull_request_number: u64,
        #[serde(alias = "expected_head_sha")]
        expected_head_sha: String,
        event: ReviewEvent,
        body: String,
        #[serde(alias = "inline_comments")]
        inline_comments: Vec<InlineReviewComment>,
    },
    ResolveReviewThread {
        #[serde(alias = "pull_request_number")]
        pull_request_number: u64,
        #[serde(alias = "thread_id")]
        thread_id: String,
        #[serde(alias = "expected_head_sha")]
        expected_head_sha: String,
    },
    CheckRun {
        name: String,
        #[serde(alias = "head_sha")]
        head_sha: String,
        status: String,
        conclusion: Option<String>,
    },
    DraftPullRequest {
        title: String,
        head: String,
        base: String,
        body: String,
    },
    UpdatePullRequestBranch {
        #[serde(alias = "pull_request_number")]
        pull_request_number: u64,
        #[serde(alias = "expected_head_sha")]
        expected_head_sha: String,
    },
    ReadyPullRequest {
        #[serde(alias = "pull_request_number")]
        pull_request_number: u64,
        #[serde(alias = "expected_head_sha")]
        expected_head_sha: String,
    },
    ClosePullRequest {
        #[serde(alias = "pull_request_number")]
        pull_request_number: u64,
    },
    CloseIssue {
        #[serde(alias = "issue_number")]
        issue_number: u64,
    },
    EnqueuePullRequest {
        #[serde(alias = "pull_request_number")]
        pull_request_number: u64,
        #[serde(alias = "expected_head_sha")]
        expected_head_sha: String,
    },
    MergePullRequest {
        #[serde(alias = "pull_request_number")]
        pull_request_number: u64,
        #[serde(alias = "expected_head_sha")]
        expected_head_sha: String,
        method: MergeMethod,
    },
}

impl<'de> Deserialize<'de> for GitHubAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = GitHubActionWire::deserialize(deserializer)?;
        let action = match wire {
            GitHubActionWire::CreateBranch { branch, from_sha } => {
                Self::create_branch(&branch, &from_sha)
            }
            GitHubActionWire::PushIntent { branch, head_sha } => {
                Self::push_intent(&branch, &head_sha)
            }
            GitHubActionWire::Comment { issue_number, body } => Self::comment(issue_number, &body),
            GitHubActionWire::Review {
                pull_request_number,
                expected_head_sha,
                event,
                body,
                inline_comments,
            } => Self::review(
                pull_request_number,
                &expected_head_sha,
                event,
                &body,
                inline_comments,
            ),
            GitHubActionWire::ResolveReviewThread {
                pull_request_number,
                thread_id,
                expected_head_sha,
            } => Self::resolve_review_thread(pull_request_number, &thread_id, &expected_head_sha),
            GitHubActionWire::CheckRun {
                name,
                head_sha,
                status,
                conclusion,
            } => Self::check_run(&name, &head_sha, &status, conclusion.as_deref()),
            GitHubActionWire::DraftPullRequest {
                title,
                head,
                base,
                body,
            } => Self::draft_pull_request(&title, &head, &base, &body),
            GitHubActionWire::UpdatePullRequestBranch {
                pull_request_number,
                expected_head_sha,
            } => Self::update_pull_request_branch(pull_request_number, &expected_head_sha),
            GitHubActionWire::ReadyPullRequest {
                pull_request_number,
                expected_head_sha,
            } => Self::ready_pull_request(pull_request_number, &expected_head_sha),
            GitHubActionWire::ClosePullRequest {
                pull_request_number,
            } => Self::close_pull_request(pull_request_number),
            GitHubActionWire::CloseIssue { issue_number } => Self::close_issue(issue_number),
            GitHubActionWire::EnqueuePullRequest {
                pull_request_number,
                expected_head_sha,
            } => Self::enqueue_pull_request(pull_request_number, &expected_head_sha),
            GitHubActionWire::MergePullRequest {
                pull_request_number,
                expected_head_sha,
                method,
            } => Self::merge_pull_request(pull_request_number, &expected_head_sha, method),
        };
        action.map_err(serde::de::Error::custom)
    }
}

impl GitHubAction {
    pub fn create_branch(branch: &str, from_sha: &str) -> Result<Self, GitHubActionError> {
        Ok(Self::CreateBranch {
            branch: validate_ref(branch)?,
            from_sha: validate_sha(from_sha)?,
        })
    }

    pub fn push_intent(branch: &str, head_sha: &str) -> Result<Self, GitHubActionError> {
        Ok(Self::PushIntent {
            branch: validate_ref(branch)?,
            head_sha: validate_sha(head_sha)?,
        })
    }

    pub fn comment(issue_number: u64, body: &str) -> Result<Self, GitHubActionError> {
        Ok(Self::Comment {
            issue_number: validate_number(issue_number)?,
            body: validate_body(body)?,
        })
    }

    pub fn review(
        pull_request_number: u64,
        expected_head_sha: &str,
        event: ReviewEvent,
        body: &str,
        inline_comments: Vec<InlineReviewComment>,
    ) -> Result<Self, GitHubActionError> {
        let pull_request_number = validate_number(pull_request_number)?;
        let body = validate_body(body)?;
        let mut positions = std::collections::HashSet::new();
        for comment in &inline_comments {
            if !positions.insert((comment.path.as_str(), comment.line)) {
                return Err(GitHubActionError::DuplicateInlinePosition);
            }
        }
        Ok(Self::Review {
            pull_request_number,
            expected_head_sha: validate_sha(expected_head_sha)?,
            event,
            body,
            inline_comments,
        })
    }

    pub fn resolve_review_thread(
        pull_request_number: u64,
        thread_id: &str,
        expected_head_sha: &str,
    ) -> Result<Self, GitHubActionError> {
        Ok(Self::ResolveReviewThread {
            pull_request_number: validate_number(pull_request_number)?,
            thread_id: validate_node_id(thread_id)?,
            expected_head_sha: validate_sha(expected_head_sha)?,
        })
    }

    pub fn check_run(
        name: &str,
        head_sha: &str,
        status: &str,
        conclusion: Option<&str>,
    ) -> Result<Self, GitHubActionError> {
        if !matches!(status, "queued" | "in_progress" | "completed")
            || (status == "completed" && conclusion.is_none())
            || (status != "completed" && conclusion.is_some())
        {
            return Err(GitHubActionError::InvalidField("status"));
        }
        Ok(Self::CheckRun {
            name: validate_text(name, 100, "name")?,
            head_sha: validate_sha(head_sha)?,
            status: status.to_owned(),
            conclusion: conclusion
                .map(|value| validate_text(value, 32, "conclusion"))
                .transpose()?,
        })
    }

    pub fn draft_pull_request(
        title: &str,
        head: &str,
        base: &str,
        body: &str,
    ) -> Result<Self, GitHubActionError> {
        Ok(Self::DraftPullRequest {
            title: validate_text(title, 256, "title")?,
            head: validate_ref(head)?,
            base: validate_ref(base)?,
            body: validate_body(body)?,
        })
    }

    pub fn update_pull_request_branch(
        pull_request_number: u64,
        expected_head_sha: &str,
    ) -> Result<Self, GitHubActionError> {
        Ok(Self::UpdatePullRequestBranch {
            pull_request_number: validate_number(pull_request_number)?,
            expected_head_sha: validate_sha(expected_head_sha)?,
        })
    }

    pub fn close_pull_request(pull_request_number: u64) -> Result<Self, GitHubActionError> {
        Ok(Self::ClosePullRequest {
            pull_request_number: validate_number(pull_request_number)?,
        })
    }

    pub fn ready_pull_request(
        pull_request_number: u64,
        expected_head_sha: &str,
    ) -> Result<Self, GitHubActionError> {
        Ok(Self::ReadyPullRequest {
            pull_request_number: validate_number(pull_request_number)?,
            expected_head_sha: validate_sha(expected_head_sha)?,
        })
    }

    pub fn close_issue(issue_number: u64) -> Result<Self, GitHubActionError> {
        Ok(Self::CloseIssue {
            issue_number: validate_number(issue_number)?,
        })
    }

    pub fn enqueue_pull_request(
        pull_request_number: u64,
        expected_head_sha: &str,
    ) -> Result<Self, GitHubActionError> {
        Ok(Self::EnqueuePullRequest {
            pull_request_number: validate_number(pull_request_number)?,
            expected_head_sha: validate_sha(expected_head_sha)?,
        })
    }

    pub fn merge_pull_request(
        pull_request_number: u64,
        expected_head_sha: &str,
        method: MergeMethod,
    ) -> Result<Self, GitHubActionError> {
        Ok(Self::MergePullRequest {
            pull_request_number: validate_number(pull_request_number)?,
            expected_head_sha: validate_sha(expected_head_sha)?,
            method,
        })
    }

    #[must_use]
    pub const fn capability(&self) -> Capability {
        match self {
            Self::CreateBranch { .. } => Capability::CreateBranch,
            Self::PushIntent { .. } => Capability::PushBranch,
            Self::Comment { .. } => Capability::PostComment,
            Self::Review { .. } => Capability::PostReview,
            Self::ResolveReviewThread { .. } => Capability::ResolveThread,
            Self::CheckRun { .. } => Capability::CreateCheckRun,
            Self::DraftPullRequest { .. } => Capability::CreatePullRequest,
            Self::UpdatePullRequestBranch { .. } => Capability::UpdatePullRequestBranch,
            Self::ReadyPullRequest { .. } => Capability::ReadyPullRequest,
            Self::ClosePullRequest { .. } => Capability::ClosePullRequest,
            Self::CloseIssue { .. } => Capability::CloseIssue,
            Self::EnqueuePullRequest { .. } => Capability::EnqueuePullRequest,
            Self::MergePullRequest { .. } => Capability::MergePullRequest,
        }
    }

    #[must_use]
    pub const fn action_kind(&self) -> &'static str {
        self.capability().action_kind()
    }

    #[must_use]
    pub const fn pull_request_number(&self) -> Option<u64> {
        match self {
            Self::Review {
                pull_request_number,
                ..
            }
            | Self::ResolveReviewThread {
                pull_request_number,
                ..
            }
            | Self::UpdatePullRequestBranch {
                pull_request_number,
                ..
            }
            | Self::ClosePullRequest {
                pull_request_number,
            }
            | Self::ReadyPullRequest {
                pull_request_number,
                ..
            }
            | Self::EnqueuePullRequest {
                pull_request_number,
                ..
            }
            | Self::MergePullRequest {
                pull_request_number,
                ..
            } => Some(*pull_request_number),
            Self::Comment { issue_number, .. } | Self::CloseIssue { issue_number } => {
                Some(*issue_number)
            }
            _ => None,
        }
    }

    #[must_use]
    pub fn branch(&self) -> Option<&str> {
        match self {
            Self::CreateBranch { branch, .. } | Self::PushIntent { branch, .. } => Some(branch),
            Self::DraftPullRequest { head, .. } => Some(head),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InlineReviewComment {
    path: String,
    line: u64,
    body: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InlineReviewCommentWire {
    path: String,
    line: u64,
    body: String,
}

impl<'de> Deserialize<'de> for InlineReviewComment {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = InlineReviewCommentWire::deserialize(deserializer)?;
        Self::new(&wire.path, wire.line, &wire.body).map_err(serde::de::Error::custom)
    }
}

impl InlineReviewComment {
    pub fn new(path: &str, line: u64, body: &str) -> Result<Self, GitHubActionError> {
        if line == 0 || path.starts_with('/') || path.contains("..") {
            return Err(GitHubActionError::InvalidField("inlineComment"));
        }
        Ok(Self {
            path: validate_text(path, 1_024, "path")?,
            line,
            body: validate_body(body)?,
        })
    }

    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    #[must_use]
    pub const fn line(&self) -> u64 {
        self.line
    }

    #[must_use]
    pub fn body(&self) -> &str {
        &self.body
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteIdentity {
    repository_id: u64,
    installation_id: u64,
    repository_full_name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteIdentityWire {
    #[serde(alias = "repository_id")]
    repository_id: u64,
    #[serde(alias = "installation_id")]
    installation_id: u64,
    #[serde(alias = "repository_full_name")]
    repository_full_name: String,
}

impl<'de> Deserialize<'de> for RemoteIdentity {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = RemoteIdentityWire::deserialize(deserializer)?;
        Self::new(
            wire.repository_id,
            wire.installation_id,
            &wire.repository_full_name,
        )
        .map_err(serde::de::Error::custom)
    }
}

impl RemoteIdentity {
    pub fn new(
        repository_id: u64,
        installation_id: u64,
        repository_full_name: &str,
    ) -> Result<Self, GitHubActionError> {
        if repository_id == 0 || installation_id == 0 {
            return Err(GitHubActionError::InvalidField("remoteIdentity"));
        }
        let Some((owner, repository)) = repository_full_name.split_once('/') else {
            return Err(GitHubActionError::InvalidField("repositoryFullName"));
        };
        validate_repository_component(owner)?;
        validate_repository_component(repository)?;
        Ok(Self {
            repository_id,
            installation_id,
            repository_full_name: repository_full_name.to_owned(),
        })
    }

    #[must_use]
    pub const fn repository_id(&self) -> u64 {
        self.repository_id
    }

    #[must_use]
    pub const fn installation_id(&self) -> u64 {
        self.installation_id
    }

    #[must_use]
    pub fn repository_full_name(&self) -> &str {
        &self.repository_full_name
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemotePrecondition {
    expected_head_sha: Option<String>,
    expected_base_sha: Option<String>,
    snapshot_generation: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemotePreconditionWire {
    #[serde(alias = "expected_head_sha")]
    expected_head_sha: Option<String>,
    #[serde(alias = "expected_base_sha")]
    expected_base_sha: Option<String>,
    #[serde(alias = "snapshot_generation")]
    snapshot_generation: u64,
}

impl<'de> Deserialize<'de> for RemotePrecondition {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = RemotePreconditionWire::deserialize(deserializer)?;
        Self::new(
            wire.expected_head_sha.as_deref(),
            wire.expected_base_sha.as_deref(),
            wire.snapshot_generation,
        )
        .map_err(serde::de::Error::custom)
    }
}

impl RemotePrecondition {
    pub fn new(
        expected_head_sha: Option<&str>,
        expected_base_sha: Option<&str>,
        snapshot_generation: u64,
    ) -> Result<Self, GitHubActionError> {
        if snapshot_generation == 0 {
            return Err(GitHubActionError::InvalidField("snapshotGeneration"));
        }
        Ok(Self {
            expected_head_sha: expected_head_sha.map(validate_sha).transpose()?,
            expected_base_sha: expected_base_sha.map(validate_sha).transpose()?,
            snapshot_generation,
        })
    }

    #[must_use]
    pub const fn snapshot_generation(&self) -> u64 {
        self.snapshot_generation
    }

    #[must_use]
    pub fn expected_head_sha(&self) -> Option<&str> {
        self.expected_head_sha.as_deref()
    }

    #[must_use]
    pub fn expected_base_sha(&self) -> Option<&str> {
        self.expected_base_sha.as_deref()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubActionPreview {
    remote: RemoteIdentity,
    action: GitHubAction,
    precondition: RemotePrecondition,
    payload_sha256: String,
    idempotency_sha256: String,
    required_permissions: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GitHubActionPreviewWire {
    remote: RemoteIdentity,
    action: GitHubAction,
    precondition: RemotePrecondition,
    #[serde(alias = "payload_sha256")]
    payload_sha256: String,
    #[serde(alias = "idempotency_sha256")]
    idempotency_sha256: String,
    #[serde(alias = "required_permissions")]
    required_permissions: Vec<String>,
}

impl<'de> Deserialize<'de> for GitHubActionPreview {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = GitHubActionPreviewWire::deserialize(deserializer)?;
        let preview = Self::new(wire.remote, wire.action, wire.precondition)
            .map_err(serde::de::Error::custom)?;
        if wire.payload_sha256 != preview.payload_sha256 {
            return Err(serde::de::Error::custom(GitHubActionError::InvalidField(
                "payloadSha256",
            )));
        }
        if wire.idempotency_sha256 != preview.idempotency_sha256 {
            return Err(serde::de::Error::custom(GitHubActionError::InvalidField(
                "idempotencySha256",
            )));
        }
        if wire.required_permissions != preview.required_permissions {
            return Err(serde::de::Error::custom(GitHubActionError::InvalidField(
                "requiredPermissions",
            )));
        }
        Ok(preview)
    }
}

impl GitHubActionPreview {
    pub fn new(
        remote: RemoteIdentity,
        action: GitHubAction,
        precondition: RemotePrecondition,
    ) -> Result<Self, GitHubActionError> {
        let payload = serde_json::to_vec(&action).map_err(|_| GitHubActionError::Serialization)?;
        let payload_sha256 = format!("{:x}", Sha256::digest(&payload));
        let required_permissions = permissions_for(&action)
            .iter()
            .map(ToString::to_string)
            .collect();
        let identity_payload = serde_json::to_vec(&(&remote, &action, &precondition))
            .map_err(|_| GitHubActionError::Serialization)?;
        let idempotency_sha256 = format!("{:x}", Sha256::digest(identity_payload));
        Ok(Self {
            remote,
            action,
            precondition,
            payload_sha256,
            idempotency_sha256,
            required_permissions,
        })
    }

    #[must_use]
    pub const fn remote(&self) -> &RemoteIdentity {
        &self.remote
    }

    #[must_use]
    pub const fn action(&self) -> &GitHubAction {
        &self.action
    }

    #[must_use]
    pub const fn precondition(&self) -> &RemotePrecondition {
        &self.precondition
    }

    #[must_use]
    pub fn idempotency_sha256(&self) -> &str {
        &self.idempotency_sha256
    }

    #[must_use]
    pub fn payload_sha256(&self) -> &str {
        &self.payload_sha256
    }

    #[must_use]
    pub fn required_permissions(&self) -> &[String] {
        &self.required_permissions
    }
}

const fn permissions_for(action: &GitHubAction) -> &'static [&'static str] {
    match action {
        GitHubAction::CreateBranch { .. } | GitHubAction::PushIntent { .. } => {
            &["contents:write", "metadata:read"]
        }
        GitHubAction::Comment { .. } | GitHubAction::CloseIssue { .. } => {
            &["issues:write", "metadata:read"]
        }
        GitHubAction::Review { .. }
        | GitHubAction::ResolveReviewThread { .. }
        | GitHubAction::DraftPullRequest { .. }
        | GitHubAction::UpdatePullRequestBranch { .. }
        | GitHubAction::ReadyPullRequest { .. }
        | GitHubAction::ClosePullRequest { .. }
        | GitHubAction::EnqueuePullRequest { .. }
        | GitHubAction::MergePullRequest { .. } => &["pull_requests:write", "metadata:read"],
        GitHubAction::CheckRun { .. } => &["checks:write", "metadata:read"],
    }
}

fn validate_node_id(value: &str) -> Result<String, GitHubActionError> {
    if value.is_empty()
        || value.len() > 256
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(GitHubActionError::InvalidField("threadId"));
    }
    Ok(value.to_owned())
}

fn validate_number(value: u64) -> Result<u64, GitHubActionError> {
    if value == 0 {
        return Err(GitHubActionError::InvalidField("number"));
    }
    Ok(value)
}

fn validate_sha(value: &str) -> Result<String, GitHubActionError> {
    if value.len() != 40 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(GitHubActionError::InvalidField("sha"));
    }
    Ok(value.to_ascii_lowercase())
}

fn validate_ref(value: &str) -> Result<String, GitHubActionError> {
    if value.is_empty()
        || value.len() > 255
        || value.starts_with(['-', '/', '.'])
        || value.ends_with(['/', '.'])
        || value.contains("..")
        || value.contains("@{")
        || value.bytes().any(|byte| {
            byte.is_ascii_control()
                || matches!(byte, b' ' | b'~' | b'^' | b':' | b'?' | b'*' | b'[' | b'\\')
        })
    {
        return Err(GitHubActionError::InvalidField("ref"));
    }
    Ok(value.to_owned())
}

fn validate_body(value: &str) -> Result<String, GitHubActionError> {
    let value = value.trim();
    let lowercase = value.to_ascii_lowercase();
    if value.is_empty()
        || value.len() > MAX_BODY_BYTES
        || value.contains('\0')
        || [
            "authorization: bearer ",
            "ghp_",
            "gho_",
            "github_pat_",
            "-----begin",
        ]
        .iter()
        .any(|marker| lowercase.contains(marker))
    {
        return Err(GitHubActionError::InvalidField("body"));
    }
    Ok(value.to_owned())
}

fn validate_text(
    value: &str,
    maximum: usize,
    field: &'static str,
) -> Result<String, GitHubActionError> {
    let value = value.trim();
    if value.is_empty() || value.len() > maximum || value.chars().any(char::is_control) {
        return Err(GitHubActionError::InvalidField(field));
    }
    Ok(value.to_owned())
}

fn validate_repository_component(value: &str) -> Result<(), GitHubActionError> {
    if value.is_empty()
        || value.len() > 100
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(GitHubActionError::InvalidField("repositoryFullName"));
    }
    Ok(())
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum GitHubActionError {
    #[error("invalid GitHub action field: {0}")]
    InvalidField(&'static str),
    #[error("duplicate inline review position")]
    DuplicateInlinePosition,
    #[error("GitHub action serialization failed")]
    Serialization,
}
