use crate::TaskId;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

const MAX_APPROVAL_DURATION: Duration = Duration::minutes(30);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Capability {
    ReadRepository,
    ModifyWorktree,
    RunKnownCommand,
    AccessNetwork,
    InstallDependency,
    CreateBranch,
    PushBranch,
    CreatePullRequest,
    PostComment,
    PostReview,
    ResolveThread,
    CreateCheckRun,
    UpdatePullRequestBranch,
    ReadyPullRequest,
    ClosePullRequest,
    CloseIssue,
    ModifyWorkflow,
    EnqueuePullRequest,
    MergePullRequest,
    AdministratorBypass,
}

impl Capability {
    #[must_use]
    pub const fn action_kind(self) -> &'static str {
        match self {
            Self::ReadRepository => "readRepository",
            Self::ModifyWorktree => "modifyWorktree",
            Self::RunKnownCommand => "runKnownCommand",
            Self::AccessNetwork => "accessNetwork",
            Self::InstallDependency => "installDependency",
            Self::CreateBranch => "createBranch",
            Self::PushBranch => "pushBranch",
            Self::CreatePullRequest => "createPullRequest",
            Self::PostComment => "postComment",
            Self::PostReview => "postReview",
            Self::ResolveThread => "resolveThread",
            Self::CreateCheckRun => "createCheckRun",
            Self::UpdatePullRequestBranch => "updatePullRequestBranch",
            Self::ReadyPullRequest => "readyPullRequest",
            Self::ClosePullRequest => "closePullRequest",
            Self::CloseIssue => "closeIssue",
            Self::ModifyWorkflow => "modifyWorkflow",
            Self::EnqueuePullRequest => "enqueuePullRequest",
            Self::MergePullRequest => "mergePullRequest",
            Self::AdministratorBypass => "administratorBypass",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ApprovalClass {
    CodexRuntime,
    LocalCapability,
    GitHubDelivery,
    Merge,
}

#[derive(Clone, Debug)]
pub struct ActionFingerprintDraft {
    pub task_id: TaskId,
    pub github_repository_id: u64,
    pub repository_full_name: String,
    pub action_kind: String,
    pub pull_request_number: Option<u64>,
    pub branch: Option<String>,
    pub head_sha: Option<String>,
    pub base_sha: Option<String>,
    pub payload_sha256: String,
    pub policy_sha256: String,
    pub instruction_sha256: String,
    pub invalidation_generation: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionFingerprint {
    task_id: TaskId,
    github_repository_id: u64,
    repository_full_name: String,
    action_kind: String,
    pull_request_number: Option<u64>,
    branch: Option<String>,
    head_sha: Option<String>,
    base_sha: Option<String>,
    payload_sha256: String,
    policy_sha256: String,
    instruction_sha256: String,
    invalidation_generation: u64,
}

impl TryFrom<ActionFingerprintDraft> for ActionFingerprint {
    type Error = ApprovalError;

    fn try_from(mut draft: ActionFingerprintDraft) -> Result<Self, Self::Error> {
        if draft.github_repository_id == 0 {
            return Err(ApprovalError::InvalidField("githubRepositoryId"));
        }
        validate_repository_name(&draft.repository_full_name)?;
        draft.action_kind = validated_text(draft.action_kind, "actionKind")?;
        if draft.pull_request_number == Some(0) {
            return Err(ApprovalError::InvalidField("pullRequestNumber"));
        }
        draft.branch = validated_optional_text(draft.branch, "branch")?;
        validate_optional_git_sha(draft.head_sha.as_deref(), "headSha")?;
        validate_optional_git_sha(draft.base_sha.as_deref(), "baseSha")?;
        validate_sha256(&draft.payload_sha256, "payloadSha256")?;
        validate_sha256(&draft.policy_sha256, "policySha256")?;
        validate_sha256(&draft.instruction_sha256, "instructionSha256")?;
        Ok(Self {
            task_id: draft.task_id,
            github_repository_id: draft.github_repository_id,
            repository_full_name: draft.repository_full_name,
            action_kind: draft.action_kind,
            pull_request_number: draft.pull_request_number,
            branch: draft.branch,
            head_sha: draft.head_sha,
            base_sha: draft.base_sha,
            payload_sha256: draft.payload_sha256,
            policy_sha256: draft.policy_sha256,
            instruction_sha256: draft.instruction_sha256,
            invalidation_generation: draft.invalidation_generation,
        })
    }
}

impl ActionFingerprint {
    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        self.task_id
    }

    #[must_use]
    pub const fn github_repository_id(&self) -> u64 {
        self.github_repository_id
    }

    #[must_use]
    pub fn repository_full_name(&self) -> &str {
        &self.repository_full_name
    }

    #[must_use]
    pub fn action_kind(&self) -> &str {
        &self.action_kind
    }

    #[must_use]
    pub const fn pull_request_number(&self) -> Option<u64> {
        self.pull_request_number
    }

    #[must_use]
    pub fn branch(&self) -> Option<&str> {
        self.branch.as_deref()
    }

    #[must_use]
    pub fn head_sha(&self) -> Option<&str> {
        self.head_sha.as_deref()
    }

    #[must_use]
    pub fn base_sha(&self) -> Option<&str> {
        self.base_sha.as_deref()
    }

    #[must_use]
    pub fn payload_sha256(&self) -> &str {
        &self.payload_sha256
    }

    #[must_use]
    pub fn policy_sha256(&self) -> &str {
        &self.policy_sha256
    }

    #[must_use]
    pub fn instruction_sha256(&self) -> &str {
        &self.instruction_sha256
    }

    #[must_use]
    pub const fn invalidation_generation(&self) -> u64 {
        self.invalidation_generation
    }

    #[must_use]
    /// Returns the canonical SHA-256 identity used by durable approval storage.
    ///
    /// # Panics
    /// Panics only if serde cannot serialize the validated in-memory fingerprint structure.
    pub fn digest_sha256(&self) -> String {
        let payload = serde_json::to_vec(self).expect("action fingerprint serialization");
        format!("{:x}", Sha256::digest(payload))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Approval {
    id: Uuid,
    class: ApprovalClass,
    capability: Capability,
    fingerprint: ActionFingerprint,
    approved_by: String,
    approved_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

impl Approval {
    /// Creates an approval for one exact action fingerprint and short time window.
    ///
    /// # Errors
    /// Returns [`ApprovalError`] for an empty approver, invalid duration, or incompatible class.
    pub fn new(
        class: ApprovalClass,
        capability: Capability,
        fingerprint: ActionFingerprint,
        approved_by: impl Into<String>,
        approved_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<Self, ApprovalError> {
        let approved_by = validated_text(approved_by.into(), "approvedBy")?;
        if expires_at <= approved_at || expires_at - approved_at > MAX_APPROVAL_DURATION {
            return Err(ApprovalError::InvalidExpiration);
        }
        if !class_supports(class, capability) {
            return Err(ApprovalError::IncompatibleClass { class, capability });
        }
        if fingerprint.action_kind() != capability.action_kind() {
            return Err(ApprovalError::InvalidField("actionKind"));
        }
        Ok(Self {
            id: Uuid::new_v4(),
            class,
            capability,
            fingerprint,
            approved_by,
            approved_at,
            expires_at,
        })
    }

    #[must_use]
    pub const fn class(&self) -> ApprovalClass {
        self.class
    }

    #[must_use]
    pub const fn id(&self) -> Uuid {
        self.id
    }

    #[must_use]
    pub const fn capability(&self) -> Capability {
        self.capability
    }

    #[must_use]
    pub fn approved_by(&self) -> &str {
        &self.approved_by
    }

    #[must_use]
    pub const fn approved_at(&self) -> DateTime<Utc> {
        self.approved_at
    }

    #[must_use]
    pub const fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }

    #[must_use]
    pub const fn fingerprint(&self) -> &ActionFingerprint {
        &self.fingerprint
    }

    fn is_valid_for(
        &self,
        class: ApprovalClass,
        capability: Capability,
        fingerprint: &ActionFingerprint,
        now: DateTime<Utc>,
    ) -> bool {
        self.class == class
            && self.capability == capability
            && self.fingerprint == *fingerprint
            && now >= self.approved_at
            && now < self.expires_at
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PolicyDecision {
    Allowed,
    ApprovalRequired(String),
    Denied(String),
}

#[derive(Clone, Debug)]
pub struct Policy {
    automation_disabled: bool,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            automation_disabled: std::env::var("PATCHWRIGHT_AUTOMATION_DISABLED")
                .is_ok_and(|value| value == "1"),
        }
    }
}

impl Policy {
    #[must_use]
    pub const fn with_automation_disabled(automation_disabled: bool) -> Self {
        Self {
            automation_disabled,
        }
    }

    #[must_use]
    pub fn authorize(
        &self,
        capability: Capability,
        fingerprint: &ActionFingerprint,
        approval: Option<&Approval>,
        now: DateTime<Utc>,
    ) -> PolicyDecision {
        if capability == Capability::AdministratorBypass {
            return PolicyDecision::Denied("administrator bypass is prohibited".into());
        }
        if fingerprint.action_kind() != capability.action_kind() {
            return PolicyDecision::Denied("action fingerprint does not match capability".into());
        }
        if self.automation_disabled && capability != Capability::ReadRepository {
            return PolicyDecision::Denied("automation kill switch is active".into());
        }
        if matches!(
            capability,
            Capability::ReadRepository | Capability::ModifyWorktree | Capability::RunKnownCommand
        ) {
            return PolicyDecision::Allowed;
        }
        let required_class = required_class(capability);
        match approval
            .filter(|value| value.is_valid_for(required_class, capability, fingerprint, now))
        {
            Some(_) => PolicyDecision::Allowed,
            None => PolicyDecision::ApprovalRequired(format!(
                "{capability:?} requires an exact {required_class:?} approval"
            )),
        }
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ApprovalError {
    #[error("invalid approval field: {0}")]
    InvalidField(&'static str),
    #[error("approval expiration must be after approval and within 30 minutes")]
    InvalidExpiration,
    #[error("{class:?} approval cannot grant {capability:?}")]
    IncompatibleClass {
        class: ApprovalClass,
        capability: Capability,
    },
}

const fn required_class(capability: Capability) -> ApprovalClass {
    match capability {
        Capability::MergePullRequest | Capability::EnqueuePullRequest => ApprovalClass::Merge,
        Capability::AccessNetwork | Capability::InstallDependency | Capability::ModifyWorkflow => {
            ApprovalClass::LocalCapability
        }
        Capability::ReadRepository | Capability::ModifyWorktree | Capability::RunKnownCommand => {
            ApprovalClass::CodexRuntime
        }
        Capability::CreateBranch
        | Capability::PushBranch
        | Capability::CreatePullRequest
        | Capability::PostComment
        | Capability::PostReview
        | Capability::ResolveThread
        | Capability::CreateCheckRun
        | Capability::UpdatePullRequestBranch
        | Capability::ReadyPullRequest
        | Capability::ClosePullRequest
        | Capability::CloseIssue => ApprovalClass::GitHubDelivery,
        Capability::AdministratorBypass => ApprovalClass::LocalCapability,
    }
}

const fn class_supports(class: ApprovalClass, capability: Capability) -> bool {
    match class {
        ApprovalClass::CodexRuntime => matches!(
            capability,
            Capability::ModifyWorktree | Capability::RunKnownCommand
        ),
        ApprovalClass::LocalCapability => matches!(
            capability,
            Capability::AccessNetwork | Capability::InstallDependency | Capability::ModifyWorkflow
        ),
        ApprovalClass::GitHubDelivery => matches!(
            capability,
            Capability::CreateBranch
                | Capability::PushBranch
                | Capability::CreatePullRequest
                | Capability::PostComment
                | Capability::PostReview
                | Capability::ResolveThread
                | Capability::CreateCheckRun
                | Capability::UpdatePullRequestBranch
                | Capability::ReadyPullRequest
                | Capability::ClosePullRequest
                | Capability::CloseIssue
        ),
        ApprovalClass::Merge => matches!(
            capability,
            Capability::EnqueuePullRequest | Capability::MergePullRequest
        ),
    }
}

fn validate_repository_name(value: &str) -> Result<(), ApprovalError> {
    let Some((owner, name)) = value.split_once('/') else {
        return Err(ApprovalError::InvalidField("repositoryFullName"));
    };
    if owner.trim().is_empty()
        || name.trim().is_empty()
        || name.contains('/')
        || value
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(ApprovalError::InvalidField("repositoryFullName"));
    }
    Ok(())
}

fn validated_text(mut value: String, field: &'static str) -> Result<String, ApprovalError> {
    value = value.trim().to_owned();
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err(ApprovalError::InvalidField(field));
    }
    Ok(value)
}

fn validated_optional_text(
    value: Option<String>,
    field: &'static str,
) -> Result<Option<String>, ApprovalError> {
    value.map(|value| validated_text(value, field)).transpose()
}

fn validate_optional_git_sha(
    value: Option<&str>,
    field: &'static str,
) -> Result<(), ApprovalError> {
    if let Some(value) = value {
        if !matches!(value.len(), 40 | 64) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(ApprovalError::InvalidField(field));
        }
    }
    Ok(())
}

fn validate_sha256(value: &str, field: &'static str) -> Result<(), ApprovalError> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ApprovalError::InvalidField(field));
    }
    Ok(())
}
