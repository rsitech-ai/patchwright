use crate::{EventStore, PreparationClaimOutcome};
use chrono::{Duration, Utc};
use patchwright_core::{
    ActionFingerprint, ActionFingerprintDraft, Approval, ApprovalClass, Capability, Policy,
    PolicyDecision, RepositoryBindingId, TaskId, TaskState,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
use thiserror::Error;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparationPreview {
    pub task_id: TaskId,
    pub repository_binding_id: RepositoryBindingId,
    pub repository_full_name: String,
    pub repository_path: String,
    pub source_sha: String,
    pub worktree_path: String,
    pub branch: String,
    pub invalidation_generation: u64,
    pub policy_sha256: String,
    pub instruction_sha256: String,
    pub fingerprint: ActionFingerprint,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PreparationPayload<'a> {
    task_id: TaskId,
    repository_binding_id: RepositoryBindingId,
    repository_full_name: &'a str,
    repository_path: &'a str,
    source_sha: &'a str,
    worktree_path: &'a str,
    branch: &'a str,
    invalidation_generation: u64,
}

pub fn preview_preparation(
    store: &EventStore,
    task_id: TaskId,
) -> Result<PreparationPreview, PreparationError> {
    let task = store
        .load_task(task_id)
        .map_err(persistence)?
        .ok_or(PreparationError::TaskMissing)?;
    if task.state != TaskState::AwaitingPreparationApproval {
        return Err(PreparationError::TaskStateInvalid);
    }
    let binding_id = task
        .repository_binding_id
        .ok_or(PreparationError::BindingMissing)?;
    let binding = store
        .repository_binding(binding_id)
        .map_err(persistence)?
        .ok_or(PreparationError::BindingMissing)?;
    let contract = store
        .task_contract(task_id)
        .map_err(persistence)?
        .ok_or(PreparationError::ContractMissing)?;
    if contract.repository_binding_id() != binding_id || contract.task_id() != task.id {
        return Err(PreparationError::BindingMismatch);
    }
    let source_sha = contract
        .head_sha()
        .or_else(|| contract.base_sha())
        .ok_or(PreparationError::SourceShaMissing)?
        .to_owned();
    let repository_path = binding
        .managed_clone()
        .or_else(|| binding.user_checkout())
        .ok_or(PreparationError::RepositoryUnavailable)?
        .to_owned();
    let worktree_path = Path::new(binding.worktree_root())
        .join(task.id.to_string())
        .to_string_lossy()
        .into_owned();
    let branch = format!("patchwright/{}", task.id);
    let invalidation_generation = task.updated_at.timestamp_micros().unsigned_abs();
    let policy_sha256 = digest(b"patchwright-preparation-policy-v1");
    let instruction_sha256 = digest(
        &serde_json::to_vec(contract.instruction_digests())
            .map_err(|_| PreparationError::Serialization)?,
    );
    let payload_sha256 = digest(
        &serde_json::to_vec(&PreparationPayload {
            task_id,
            repository_binding_id: binding_id,
            repository_full_name: binding.full_name(),
            repository_path: &repository_path,
            source_sha: &source_sha,
            worktree_path: &worktree_path,
            branch: &branch,
            invalidation_generation,
        })
        .map_err(|_| PreparationError::Serialization)?,
    );
    let fingerprint = ActionFingerprint::try_from(ActionFingerprintDraft {
        task_id,
        github_repository_id: binding.github_repository_id(),
        repository_full_name: binding.full_name().to_owned(),
        action_kind: Capability::PrepareWorktree.action_kind().to_owned(),
        pull_request_number: None,
        branch: Some(branch.clone()),
        head_sha: Some(source_sha.clone()),
        base_sha: None,
        payload_sha256,
        policy_sha256: policy_sha256.clone(),
        instruction_sha256: instruction_sha256.clone(),
        invalidation_generation,
    })
    .map_err(|_| PreparationError::FingerprintInvalid)?;
    Ok(PreparationPreview {
        task_id,
        repository_binding_id: binding_id,
        repository_full_name: binding.full_name().to_owned(),
        repository_path,
        source_sha,
        worktree_path,
        branch,
        invalidation_generation,
        policy_sha256,
        instruction_sha256,
        fingerprint,
    })
}

pub fn approve_preparation(
    store: &EventStore,
    preview: &PreparationPreview,
    approved_by: &str,
) -> Result<Approval, PreparationError> {
    if preview_preparation(store, preview.task_id)? != *preview {
        return Err(PreparationError::PreviewStale);
    }
    let now = Utc::now();
    let approval = Approval::new(
        ApprovalClass::Preparation,
        Capability::PrepareWorktree,
        preview.fingerprint.clone(),
        approved_by,
        now,
        now + Duration::minutes(10),
    )
    .map_err(|_| PreparationError::ApprovalInvalid)?;
    store.save_approval(&approval).map_err(persistence)?;
    Ok(approval)
}

pub fn authorize_preparation(
    store: &EventStore,
    preview: &PreparationPreview,
    approval_id: uuid::Uuid,
) -> Result<(), PreparationError> {
    if preview_preparation(store, preview.task_id)? != *preview {
        return Err(PreparationError::PreviewStale);
    }
    let approval = store
        .approval(approval_id)
        .map_err(persistence)?
        .ok_or(PreparationError::ApprovalMissing)?;
    match Policy::default().authorize(
        Capability::PrepareWorktree,
        &preview.fingerprint,
        Some(&approval),
        Utc::now(),
    ) {
        PolicyDecision::Allowed => {}
        PolicyDecision::ApprovalRequired(_) => return Err(PreparationError::ApprovalInvalid),
        PolicyDecision::Denied(_) => return Err(PreparationError::PolicyDenied),
    }
    match store
        .claim_preparation(
            approval_id,
            preview.task_id,
            &preview.fingerprint.digest_sha256(),
            Utc::now(),
        )
        .map_err(persistence)?
    {
        PreparationClaimOutcome::Claimed => Ok(()),
        PreparationClaimOutcome::ApprovalUnavailable => Err(PreparationError::ApprovalInvalid),
        PreparationClaimOutcome::AlreadyClaimed => Err(PreparationError::ApprovalAlreadyUsed),
        PreparationClaimOutcome::TaskInProgress => Err(PreparationError::TaskInProgress),
    }
}

fn digest(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}

fn persistence(error: anyhow::Error) -> PreparationError {
    PreparationError::Persistence(error.to_string())
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum PreparationError {
    #[error("task is missing")]
    TaskMissing,
    #[error("task is not awaiting preparation approval")]
    TaskStateInvalid,
    #[error("task repository binding is missing")]
    BindingMissing,
    #[error("task contract is missing")]
    ContractMissing,
    #[error("task repository binding does not match its contract")]
    BindingMismatch,
    #[error("task contract has no captured source SHA")]
    SourceShaMissing,
    #[error("task repository has no managed clone or checkout")]
    RepositoryUnavailable,
    #[error("preparation fingerprint is invalid")]
    FingerprintInvalid,
    #[error("preparation preview is stale")]
    PreviewStale,
    #[error("preparation approval is missing")]
    ApprovalMissing,
    #[error("preparation approval is expired, invalid, or mismatched")]
    ApprovalInvalid,
    #[error("preparation approval was already used")]
    ApprovalAlreadyUsed,
    #[error("another preparation is already active for this task")]
    TaskInProgress,
    #[error("preparation policy denied the action")]
    PolicyDenied,
    #[error("preparation serialization failed")]
    Serialization,
    #[error("preparation persistence failed: {0}")]
    Persistence(String),
}
