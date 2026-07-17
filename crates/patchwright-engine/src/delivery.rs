use crate::{EventStore, GitHubRepositorySnapshot, WorkItemKind};
use chrono::{Duration, Utc};
use patchwright_core::{
    ActionFingerprint, ActionFingerprintDraft, Approval, ApprovalClass, Capability, GitHubAction,
    GitHubActionPreview, Policy, PolicyDecision, TaskId, TaskSource, TaskState,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryPreview {
    pub task_id: TaskId,
    pub action: GitHubActionPreview,
    pub fingerprint: ActionFingerprint,
}

pub fn preview_delivery(
    store: &EventStore,
    task_id: TaskId,
    action: GitHubActionPreview,
) -> Result<DeliveryPreview, DeliveryError> {
    let task = store
        .load_task(task_id)
        .map_err(persistence)?
        .ok_or(DeliveryError::TaskMissing)?;
    let contract = store
        .task_contract(task_id)
        .map_err(persistence)?
        .ok_or(DeliveryError::ContractMissing)?;
    let capability = action.action().capability();
    let expected_state = if matches!(
        capability,
        Capability::MergePullRequest | Capability::EnqueuePullRequest
    ) {
        TaskState::AwaitingMergeApproval
    } else {
        TaskState::AwaitingDeliveryApproval
    };
    if task.state != expected_state {
        return Err(DeliveryError::TaskStateInvalid);
    }
    if !contract.required_capabilities().contains(&capability) {
        return Err(DeliveryError::CapabilityNotDeclared);
    }
    let binding_id = task
        .repository_binding_id
        .ok_or(DeliveryError::BindingMissing)?;
    if binding_id != contract.repository_binding_id() {
        return Err(DeliveryError::BindingMismatch);
    }
    let binding = store
        .repository_binding(binding_id)
        .map_err(persistence)?
        .ok_or(DeliveryError::BindingMissing)?;
    if action.remote().repository_id() != binding.github_repository_id()
        || action.remote().installation_id() != binding.installation_id()
        || action.remote().repository_full_name() != binding.full_name()
    {
        return Err(DeliveryError::RemoteMismatch);
    }
    if contract
        .source()
        .repository_id()
        .is_some_and(|repository_id| repository_id != binding.github_repository_id())
        || contract
            .source()
            .repository_full_name()
            .is_some_and(|full_name| full_name != binding.full_name())
    {
        return Err(DeliveryError::SourceMismatch);
    }
    validate_action_target(&contract, &binding, task_id, action.action())?;
    validate_bound_preconditions(&action, &contract)?;
    validate_action_sha(&task, &contract, action.action())?;
    let policy_sha256 = digest(b"patchwright-policy-v1");
    let instruction_sha256 = digest(
        &serde_json::to_vec(contract.instruction_digests())
            .map_err(|_| DeliveryError::Serialization)?,
    );
    let fingerprint = ActionFingerprint::try_from(ActionFingerprintDraft {
        task_id,
        github_repository_id: action.remote().repository_id(),
        repository_full_name: action.remote().repository_full_name().to_owned(),
        action_kind: action.action().action_kind().to_owned(),
        pull_request_number: action.action().pull_request_number(),
        branch: action.action().branch().map(ToOwned::to_owned),
        head_sha: action
            .precondition()
            .expected_head_sha()
            .map(ToOwned::to_owned),
        base_sha: action
            .precondition()
            .expected_base_sha()
            .map(ToOwned::to_owned),
        payload_sha256: action.payload_sha256().to_owned(),
        policy_sha256,
        instruction_sha256,
        invalidation_generation: action.precondition().snapshot_generation(),
    })
    .map_err(|_| DeliveryError::FingerprintInvalid)?;
    Ok(DeliveryPreview {
        task_id,
        action,
        fingerprint,
    })
}

fn validate_bound_preconditions(
    action: &GitHubActionPreview,
    contract: &patchwright_core::TaskContract,
) -> Result<(), DeliveryError> {
    let precondition = action.precondition();
    if action.action().expected_head_sha().is_some()
        && action.action().expected_head_sha() != precondition.expected_head_sha()
        || action.action().expected_base_sha().is_some()
            && action.action().expected_base_sha() != precondition.expected_base_sha()
        || contract.head_sha().is_some() && precondition.expected_head_sha() != contract.head_sha()
        || contract.base_sha().is_some() && precondition.expected_base_sha() != contract.base_sha()
    {
        return Err(DeliveryError::PreconditionMismatch);
    }
    Ok(())
}

pub fn approve_delivery(
    store: &EventStore,
    preview: &DeliveryPreview,
    approved_by: &str,
) -> Result<Approval, DeliveryError> {
    let fresh = preview_delivery(store, preview.task_id, preview.action.clone())?;
    if fresh != *preview {
        return Err(DeliveryError::PreviewStale);
    }
    let class = if matches!(
        preview.action.action().capability(),
        patchwright_core::Capability::MergePullRequest
            | patchwright_core::Capability::EnqueuePullRequest
    ) {
        ApprovalClass::Merge
    } else {
        ApprovalClass::GitHubDelivery
    };
    let now = Utc::now();
    let approval = Approval::new(
        class,
        preview.action.action().capability(),
        preview.fingerprint.clone(),
        approved_by,
        now,
        now + Duration::minutes(10),
    )
    .map_err(|_| DeliveryError::ApprovalInvalid)?;
    store.save_approval(&approval).map_err(persistence)?;
    Ok(approval)
}

pub fn authorize_execution(
    store: &EventStore,
    preview: &DeliveryPreview,
    approval_id: uuid::Uuid,
) -> Result<String, DeliveryError> {
    let fresh = preview_delivery(store, preview.task_id, preview.action.clone())?;
    if fresh != *preview {
        return Err(DeliveryError::PreviewStale);
    }
    let approval = store
        .approval(approval_id)
        .map_err(persistence)?
        .ok_or(DeliveryError::ApprovalMissing)?;
    match Policy::default().authorize(
        preview.action.action().capability(),
        &preview.fingerprint,
        Some(&approval),
        Utc::now(),
    ) {
        PolicyDecision::Allowed => {}
        PolicyDecision::ApprovalRequired(_) => return Err(DeliveryError::ApprovalInvalid),
        PolicyDecision::Denied(_) => return Err(DeliveryError::PolicyDenied),
    }
    let key = preview.action.idempotency_sha256().to_owned();
    let task_event = if matches!(
        preview.action.action().capability(),
        Capability::MergePullRequest | Capability::EnqueuePullRequest
    ) {
        let mut task = store
            .load_task(preview.task_id)
            .map_err(persistence)?
            .ok_or(DeliveryError::TaskMissing)?;
        task.transition(TaskState::Merging)
            .map_err(|error| DeliveryError::Persistence(error.to_string()))?;
        Some((task, "Approved merge execution started".to_owned()))
    } else {
        None
    };
    if !store
        .claim_delivery_with_task_event(&key, task_event.as_ref())
        .map_err(persistence)?
    {
        return Err(DeliveryError::AlreadyClaimed);
    }
    Ok(key)
}

pub fn complete_successful_delivery(
    store: &EventStore,
    preview: &DeliveryPreview,
    key: &str,
    encoded_result: &str,
    merged: bool,
) -> Result<(), DeliveryError> {
    let capability = preview.action.action().capability();
    let terminal = matches!(
        capability,
        Capability::CloseIssue | Capability::ClosePullRequest
    ) || (capability == Capability::MergePullRequest && merged);
    let enters_monitoring = matches!(
        capability,
        Capability::CreatePullRequest
            | Capability::PostReview
            | Capability::UpdatePullRequestBranch
            | Capability::ReadyPullRequest
    );
    if !terminal && !enters_monitoring {
        return store
            .complete_delivery(key, encoded_result)
            .map_err(persistence);
    }

    let mut task = store
        .load_task(preview.task_id)
        .map_err(persistence)?
        .ok_or(DeliveryError::TaskMissing)?;
    let merge_lifecycle = [(
        TaskState::Merging,
        TaskState::Completed,
        "GitHub confirmed the approved merge completed",
    )];
    let close_lifecycle = [
        (
            TaskState::AwaitingDeliveryApproval,
            TaskState::Delivering,
            "Approved terminal GitHub delivery started",
        ),
        (
            TaskState::Delivering,
            TaskState::Monitoring,
            "GitHub accepted the terminal delivery; reconciling remote state",
        ),
        (
            TaskState::Monitoring,
            TaskState::Completed,
            "GitHub confirmed the closed task outcome completed",
        ),
    ];
    let monitoring_lifecycle = [
        (
            TaskState::AwaitingDeliveryApproval,
            TaskState::Delivering,
            "Approved GitHub delivery started",
        ),
        (
            TaskState::Delivering,
            TaskState::Monitoring,
            "GitHub accepted the delivery; fresh CI and review evidence is required",
        ),
    ];
    let lifecycle = if capability == Capability::MergePullRequest {
        merge_lifecycle.as_slice()
    } else if enters_monitoring {
        monitoring_lifecycle.as_slice()
    } else {
        close_lifecycle.as_slice()
    };
    let mut events = Vec::new();
    if task.state != TaskState::Completed {
        let Some(start) = lifecycle
            .iter()
            .position(|(state, _, _)| *state == task.state)
        else {
            return store
                .complete_delivery(key, encoded_result)
                .map_err(persistence);
        };
        for (_, next, summary) in &lifecycle[start..] {
            task.transition(*next)
                .map_err(|error| DeliveryError::Persistence(error.to_string()))?;
            events.push((task.clone(), (*summary).to_owned()));
        }
    }
    store
        .complete_delivery_with_task_events(key, encoded_result, &events)
        .map_err(persistence)
}

pub fn complete_failed_delivery(
    store: &EventStore,
    preview: &DeliveryPreview,
    key: &str,
    encoded_result: &str,
) -> Result<(), DeliveryError> {
    if !matches!(
        preview.action.action().capability(),
        Capability::MergePullRequest | Capability::EnqueuePullRequest
    ) {
        return store
            .complete_delivery(key, encoded_result)
            .map_err(persistence);
    }
    let mut task = store
        .load_task(preview.task_id)
        .map_err(persistence)?
        .ok_or(DeliveryError::TaskMissing)?;
    if task.state != TaskState::Merging {
        return store
            .complete_delivery(key, encoded_result)
            .map_err(persistence);
    }
    task.transition(TaskState::AwaitingMergeApproval)
        .map_err(|error| DeliveryError::Persistence(error.to_string()))?;
    store
        .complete_failed_merge_delivery(
            key,
            encoded_result,
            &task,
            "GitHub definitively rejected the merge; fresh merge approval is required",
        )
        .map_err(persistence)
}

pub fn record_ambiguous_delivery(
    store: &EventStore,
    key: &str,
    encoded_result: &str,
) -> Result<(), DeliveryError> {
    store
        .mark_delivery_ambiguous(key, encoded_result)
        .map_err(persistence)
}

pub fn reconcile_completed_task_from_snapshot(
    store: &EventStore,
    task_id: TaskId,
    snapshot: &GitHubRepositorySnapshot,
) -> Result<patchwright_core::Task, DeliveryError> {
    let task = store
        .load_task(task_id)
        .map_err(persistence)?
        .ok_or(DeliveryError::TaskMissing)?;
    let repository_id = task
        .source
        .repository_id()
        .ok_or(DeliveryError::SourceMismatch)?;
    let repository_full_name = task
        .source
        .repository_full_name()
        .ok_or(DeliveryError::SourceMismatch)?;
    let number = task
        .source
        .item_number()
        .ok_or(DeliveryError::SourceMismatch)?;
    if snapshot.repository.id != repository_id
        || snapshot.repository.full_name != repository_full_name
    {
        return Err(DeliveryError::SourceMismatch);
    }
    let item = snapshot
        .work_items
        .iter()
        .find(|item| item.number == number)
        .ok_or(DeliveryError::RemoteItemMissing)?;
    let completed = match item.kind {
        WorkItemKind::PullRequest => {
            if task.source.head_sha() != item.head_sha.as_deref() {
                return Err(DeliveryError::PreconditionMismatch);
            }
            item.merged == Some(true)
        }
        WorkItemKind::Issue => {
            item.state == "closed" && item.state_reason.as_deref() != Some("not_planned")
        }
    };
    let ambiguous_key = if task.state == TaskState::Merging {
        store
            .ambiguous_delivery_for_task(task_id)
            .map_err(persistence)?
    } else {
        None
    };
    if !completed {
        if let Some(key) = ambiguous_key.as_deref() {
            if item.kind != WorkItemKind::PullRequest || item.state != "open" {
                return Err(DeliveryError::RemoteNotCompleted);
            }
            let mut retry = task;
            retry
                .transition(TaskState::AwaitingMergeApproval)
                .map_err(|error| DeliveryError::Persistence(error.to_string()))?;
            let result = serde_json::json!({
                "state": "failed",
                "error": "fresh GitHub reconciliation confirmed the approved merge did not complete"
            });
            store
                .complete_failed_merge_delivery(
                    key,
                    &result.to_string(),
                    &retry,
                    "Fresh GitHub reconciliation found no merge; fresh merge approval is required",
                )
                .map_err(persistence)?;
            return Ok(retry);
        }
        return Err(DeliveryError::RemoteNotCompleted);
    }
    if task.state == TaskState::Completed {
        return Ok(task);
    }
    let events = completion_events(task)?;
    if let Some(key) = ambiguous_key.as_deref() {
        let result = serde_json::json!({
            "state": "succeeded",
            "result": {
                "merged": true,
                "sha": item.merge_commit_sha.as_deref()
            }
        });
        store
            .complete_delivery_with_task_events(key, &result.to_string(), &events)
            .map_err(persistence)?;
    } else {
        store.save_task_events(&events).map_err(persistence)?;
    }
    events
        .last()
        .map(|(task, _)| task.clone())
        .ok_or(DeliveryError::TaskMissing)
}

fn completion_events(
    mut task: patchwright_core::Task,
) -> Result<Vec<(patchwright_core::Task, String)>, DeliveryError> {
    let lifecycle = [
        (TaskState::AwaitingDeliveryApproval, TaskState::Delivering),
        (TaskState::Delivering, TaskState::Monitoring),
        (TaskState::Monitoring, TaskState::AwaitingMergeApproval),
        (TaskState::AwaitingMergeApproval, TaskState::Merging),
        (TaskState::Merging, TaskState::Completed),
    ];
    let start = lifecycle
        .iter()
        .position(|(state, _)| *state == task.state)
        .ok_or_else(|| {
            DeliveryError::Persistence(format!(
                "completed task cannot reconcile from {}",
                task.state
            ))
        })?;
    let mut events = Vec::new();
    for (_, next) in &lifecycle[start..] {
        task.transition(*next)
            .map_err(|error| DeliveryError::Persistence(error.to_string()))?;
        let summary = if *next == TaskState::Completed {
            "GitHub confirmed the task outcome is complete"
        } else {
            "GitHub completion reconciled into the durable task lifecycle"
        };
        events.push((task.clone(), summary.into()));
    }
    Ok(events)
}

fn digest(input: &[u8]) -> String {
    format!("{:x}", Sha256::digest(input))
}

fn validate_action_target(
    contract: &patchwright_core::TaskContract,
    binding: &patchwright_core::RepositoryBinding,
    task_id: TaskId,
    action: &GitHubAction,
) -> Result<(), DeliveryError> {
    let source_number = contract.source().item_number();
    let action_number = action.pull_request_number();
    if action_number.is_some() && action_number != source_number {
        return Err(DeliveryError::ActionTargetMismatch);
    }
    match (action, contract.source()) {
        (
            GitHubAction::CloseIssue { .. } | GitHubAction::Comment { .. },
            TaskSource::GitHubIssue(_),
        )
        | (
            GitHubAction::Comment { .. }
            | GitHubAction::Review { .. }
            | GitHubAction::ResolveReviewThread { .. }
            | GitHubAction::UpdatePullRequestBranch { .. }
            | GitHubAction::ReadyPullRequest { .. }
            | GitHubAction::ClosePullRequest { .. }
            | GitHubAction::EnqueuePullRequest { .. }
            | GitHubAction::MergePullRequest { .. },
            TaskSource::GitHubPullRequest(_),
        ) => {}
        (
            GitHubAction::Comment { .. }
            | GitHubAction::Review { .. }
            | GitHubAction::ResolveReviewThread { .. }
            | GitHubAction::UpdatePullRequestBranch { .. }
            | GitHubAction::ReadyPullRequest { .. }
            | GitHubAction::ClosePullRequest { .. }
            | GitHubAction::CloseIssue { .. }
            | GitHubAction::EnqueuePullRequest { .. }
            | GitHubAction::MergePullRequest { .. },
            _,
        ) => return Err(DeliveryError::ActionTargetMismatch),
        _ => {}
    }
    let prepared_branch = format!("patchwright/{task_id}");
    if action
        .branch()
        .is_some_and(|branch| branch != prepared_branch)
    {
        return Err(DeliveryError::BranchMismatch);
    }
    if let GitHubAction::DraftPullRequest { base, .. } = action
        && base != binding.default_branch()
    {
        return Err(DeliveryError::BranchMismatch);
    }
    Ok(())
}

fn validate_action_sha(
    task: &patchwright_core::Task,
    contract: &patchwright_core::TaskContract,
    action: &GitHubAction,
) -> Result<(), DeliveryError> {
    match action {
        GitHubAction::CreateBranch { from_sha, .. } => {
            if contract.base_sha() != Some(from_sha.as_str()) {
                return Err(DeliveryError::PreconditionMismatch);
            }
        }
        GitHubAction::CheckRun { head_sha, .. } => {
            let expected = if let Some(contract_head) = contract.head_sha() {
                contract_head.to_owned()
            } else {
                crate::RepositoryService::inspect(std::path::Path::new(&task.repository_path))
                    .map_err(|_| DeliveryError::PreconditionMismatch)?
                    .head_sha
            };
            if *head_sha != expected {
                return Err(DeliveryError::PreconditionMismatch);
            }
        }
        _ => {}
    }
    Ok(())
}

fn persistence(error: anyhow::Error) -> DeliveryError {
    DeliveryError::Persistence(error.to_string())
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum DeliveryError {
    #[error("delivery task is missing")]
    TaskMissing,
    #[error("delivery task contract is missing")]
    ContractMissing,
    #[error("delivery task is not awaiting the required approval class")]
    TaskStateInvalid,
    #[error("delivery action capability is not declared by the task contract")]
    CapabilityNotDeclared,
    #[error("delivery repository binding is missing")]
    BindingMissing,
    #[error("delivery repository binding changed")]
    BindingMismatch,
    #[error("delivery remote identity does not match the task binding")]
    RemoteMismatch,
    #[error("delivery action target does not match the task source")]
    ActionTargetMismatch,
    #[error("delivery branch does not match the prepared task branch")]
    BranchMismatch,
    #[error("delivery source SHA precondition changed")]
    PreconditionMismatch,
    #[error("GitHub reconciliation source does not match the task")]
    SourceMismatch,
    #[error("GitHub reconciliation item is missing from the fresh snapshot")]
    RemoteItemMissing,
    #[error("GitHub has not completed the task outcome")]
    RemoteNotCompleted,
    #[error("delivery action fingerprint is invalid")]
    FingerprintInvalid,
    #[error("delivery preview changed and must be approved again")]
    PreviewStale,
    #[error("delivery approval is missing")]
    ApprovalMissing,
    #[error("delivery approval is expired or does not match")]
    ApprovalInvalid,
    #[error("delivery is denied by the automation policy")]
    PolicyDenied,
    #[error("delivery action was already claimed; reconcile status before retrying")]
    AlreadyClaimed,
    #[error("delivery serialization failed")]
    Serialization,
    #[error("delivery persistence failed: {0}")]
    Persistence(String),
}
