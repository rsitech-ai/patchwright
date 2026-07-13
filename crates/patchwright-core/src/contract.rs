use crate::{Capability, TaskId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fmt,
    path::{Component, Path},
    str::FromStr,
};
use thiserror::Error;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RepositoryBindingId(Uuid);

impl RepositoryBindingId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for RepositoryBindingId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for RepositoryBindingId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for RepositoryBindingId {
    type Err = uuid::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(value).map(Self)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RepositoryPermissionLevel {
    #[default]
    None,
    Read,
    Write,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryPermissionSnapshot {
    pub metadata: RepositoryPermissionLevel,
    pub contents: RepositoryPermissionLevel,
    pub issues: RepositoryPermissionLevel,
    pub pull_requests: RepositoryPermissionLevel,
    pub checks: RepositoryPermissionLevel,
    pub administration: RepositoryPermissionLevel,
}

impl RepositoryPermissionSnapshot {
    #[must_use]
    pub const fn read_only() -> Self {
        Self {
            metadata: RepositoryPermissionLevel::Read,
            contents: RepositoryPermissionLevel::Read,
            issues: RepositoryPermissionLevel::Read,
            pull_requests: RepositoryPermissionLevel::Read,
            checks: RepositoryPermissionLevel::Read,
            administration: RepositoryPermissionLevel::None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CredentialHealth {
    Unknown,
    Healthy,
    Expired,
    Revoked,
    MissingPermission,
}

#[derive(Clone, Debug)]
pub struct RepositoryBindingDraft {
    pub github_repository_id: u64,
    pub full_name: String,
    pub installation_id: u64,
    pub clone_url: String,
    pub html_url: String,
    pub default_branch: String,
    pub user_checkout: Option<String>,
    pub managed_clone: Option<String>,
    pub state_root: String,
    pub worktree_root: String,
    pub default_branch_sha: Option<String>,
    pub default_branch_committed_at: Option<DateTime<Utc>>,
    pub permissions: RepositoryPermissionSnapshot,
    pub credential_health: CredentialHealth,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryBinding {
    id: RepositoryBindingId,
    github_repository_id: u64,
    full_name: String,
    installation_id: u64,
    clone_url: String,
    html_url: String,
    default_branch: String,
    user_checkout: Option<String>,
    managed_clone: Option<String>,
    state_root: String,
    worktree_root: String,
    default_branch_sha: Option<String>,
    default_branch_committed_at: Option<DateTime<Utc>>,
    permissions: RepositoryPermissionSnapshot,
    credential_health: CredentialHealth,
}

impl TryFrom<RepositoryBindingDraft> for RepositoryBinding {
    type Error = ContractError;

    fn try_from(draft: RepositoryBindingDraft) -> Result<Self, Self::Error> {
        nonzero(draft.github_repository_id, "githubRepositoryId")?;
        nonzero(draft.installation_id, "installationId")?;
        validate_repository_name(&draft.full_name)?;
        validate_https_url(&draft.clone_url, "cloneUrl")?;
        validate_https_url(&draft.html_url, "htmlUrl")?;
        nonempty(&draft.default_branch, "defaultBranch")?;
        validate_optional_absolute_path(draft.user_checkout.as_deref(), "userCheckout")?;
        validate_optional_absolute_path(draft.managed_clone.as_deref(), "managedClone")?;
        validate_absolute_path(&draft.state_root, "stateRoot")?;
        validate_absolute_path(&draft.worktree_root, "worktreeRoot")?;
        if let Some(sha) = draft.default_branch_sha.as_deref() {
            validate_git_sha(sha, "defaultBranchSha")?;
        }
        if draft.default_branch_sha.is_some() != draft.default_branch_committed_at.is_some() {
            return Err(ContractError::InvalidField("defaultBranchCommit"));
        }
        Ok(Self {
            id: RepositoryBindingId::new(),
            github_repository_id: draft.github_repository_id,
            full_name: draft.full_name,
            installation_id: draft.installation_id,
            clone_url: draft.clone_url,
            html_url: draft.html_url,
            default_branch: draft.default_branch,
            user_checkout: draft.user_checkout,
            managed_clone: draft.managed_clone,
            state_root: draft.state_root,
            worktree_root: draft.worktree_root,
            default_branch_sha: draft.default_branch_sha,
            default_branch_committed_at: draft.default_branch_committed_at,
            permissions: draft.permissions,
            credential_health: draft.credential_health,
        })
    }
}

impl RepositoryBinding {
    #[must_use]
    pub const fn id(&self) -> RepositoryBindingId {
        self.id
    }

    #[must_use]
    pub const fn github_repository_id(&self) -> u64 {
        self.github_repository_id
    }

    #[must_use]
    pub fn full_name(&self) -> &str {
        &self.full_name
    }

    #[must_use]
    pub const fn installation_id(&self) -> u64 {
        self.installation_id
    }

    #[must_use]
    pub fn clone_url(&self) -> &str {
        &self.clone_url
    }

    #[must_use]
    pub fn html_url(&self) -> &str {
        &self.html_url
    }

    #[must_use]
    pub fn default_branch(&self) -> &str {
        &self.default_branch
    }

    #[must_use]
    pub fn user_checkout(&self) -> Option<&str> {
        self.user_checkout.as_deref()
    }

    #[must_use]
    pub fn managed_clone(&self) -> Option<&str> {
        self.managed_clone.as_deref()
    }

    #[must_use]
    pub fn state_root(&self) -> &str {
        &self.state_root
    }

    #[must_use]
    pub fn worktree_root(&self) -> &str {
        &self.worktree_root
    }

    #[must_use]
    pub fn default_branch_sha(&self) -> Option<&str> {
        self.default_branch_sha.as_deref()
    }

    #[must_use]
    pub const fn default_branch_committed_at(&self) -> Option<DateTime<Utc>> {
        self.default_branch_committed_at
    }

    #[must_use]
    pub const fn permissions(&self) -> RepositoryPermissionSnapshot {
        self.permissions
    }

    #[must_use]
    pub const fn credential_health(&self) -> CredentialHealth {
        self.credential_health
    }
}

#[derive(Clone, Debug)]
pub struct GitHubIssueSourceInput {
    pub repository_id: u64,
    pub repository_full_name: String,
    pub number: u64,
    pub html_url: String,
    pub snapshot_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub struct GitHubPullRequestSourceInput {
    pub repository_id: u64,
    pub repository_full_name: String,
    pub number: u64,
    pub html_url: String,
    pub snapshot_at: DateTime<Utc>,
    pub base_ref: String,
    pub base_sha: String,
    pub head_ref: String,
    pub head_sha: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubIssueSource {
    repository_id: u64,
    repository_full_name: String,
    number: u64,
    html_url: String,
    snapshot_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubPullRequestSource {
    repository_id: u64,
    repository_full_name: String,
    number: u64,
    html_url: String,
    snapshot_at: DateTime<Utc>,
    base_ref: String,
    base_sha: String,
    head_ref: String,
    head_sha: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "details", rename_all = "camelCase")]
pub enum TaskSource {
    #[default]
    LocalRequest,
    GitHubIssue(GitHubIssueSource),
    GitHubPullRequest(GitHubPullRequestSource),
}

impl TaskSource {
    /// Creates a validated GitHub issue source snapshot.
    ///
    /// # Errors
    /// Returns [`ContractError`] when repository, item, or URL identity is invalid.
    pub fn github_issue(input: GitHubIssueSourceInput) -> Result<Self, ContractError> {
        validate_github_item(
            input.repository_id,
            &input.repository_full_name,
            input.number,
            &input.html_url,
        )?;
        Ok(Self::GitHubIssue(GitHubIssueSource {
            repository_id: input.repository_id,
            repository_full_name: input.repository_full_name,
            number: input.number,
            html_url: input.html_url,
            snapshot_at: input.snapshot_at,
        }))
    }

    /// Creates a validated GitHub pull-request source snapshot.
    ///
    /// # Errors
    /// Returns [`ContractError`] when repository, item, URL, ref, or SHA identity is invalid.
    pub fn github_pull_request(input: GitHubPullRequestSourceInput) -> Result<Self, ContractError> {
        validate_github_item(
            input.repository_id,
            &input.repository_full_name,
            input.number,
            &input.html_url,
        )?;
        validate_ref(&input.base_ref, "baseRef")?;
        validate_ref(&input.head_ref, "headRef")?;
        validate_git_sha(&input.base_sha, "baseSha")?;
        validate_git_sha(&input.head_sha, "headSha")?;
        Ok(Self::GitHubPullRequest(GitHubPullRequestSource {
            repository_id: input.repository_id,
            repository_full_name: input.repository_full_name,
            number: input.number,
            html_url: input.html_url,
            snapshot_at: input.snapshot_at,
            base_ref: input.base_ref,
            base_sha: input.base_sha,
            head_ref: input.head_ref,
            head_sha: input.head_sha,
        }))
    }

    #[must_use]
    pub const fn repository_id(&self) -> Option<u64> {
        match self {
            Self::LocalRequest => None,
            Self::GitHubIssue(source) => Some(source.repository_id),
            Self::GitHubPullRequest(source) => Some(source.repository_id),
        }
    }

    #[must_use]
    pub const fn item_number(&self) -> Option<u64> {
        match self {
            Self::LocalRequest => None,
            Self::GitHubIssue(source) => Some(source.number),
            Self::GitHubPullRequest(source) => Some(source.number),
        }
    }

    #[must_use]
    pub fn repository_full_name(&self) -> Option<&str> {
        match self {
            Self::LocalRequest => None,
            Self::GitHubIssue(source) => Some(&source.repository_full_name),
            Self::GitHubPullRequest(source) => Some(&source.repository_full_name),
        }
    }

    #[must_use]
    pub fn html_url(&self) -> Option<&str> {
        match self {
            Self::LocalRequest => None,
            Self::GitHubIssue(source) => Some(&source.html_url),
            Self::GitHubPullRequest(source) => Some(&source.html_url),
        }
    }

    #[must_use]
    pub fn snapshot_at(&self) -> Option<DateTime<Utc>> {
        match self {
            Self::LocalRequest => None,
            Self::GitHubIssue(source) => Some(source.snapshot_at),
            Self::GitHubPullRequest(source) => Some(source.snapshot_at),
        }
    }

    #[must_use]
    pub fn base_ref(&self) -> Option<&str> {
        match self {
            Self::GitHubPullRequest(source) => Some(&source.base_ref),
            Self::LocalRequest | Self::GitHubIssue(_) => None,
        }
    }

    #[must_use]
    pub fn base_sha(&self) -> Option<&str> {
        match self {
            Self::GitHubPullRequest(source) => Some(&source.base_sha),
            Self::LocalRequest | Self::GitHubIssue(_) => None,
        }
    }

    #[must_use]
    pub fn head_ref(&self) -> Option<&str> {
        match self {
            Self::GitHubPullRequest(source) => Some(&source.head_ref),
            Self::LocalRequest | Self::GitHubIssue(_) => None,
        }
    }

    #[must_use]
    pub fn head_sha(&self) -> Option<&str> {
        match self {
            Self::GitHubPullRequest(source) => Some(&source.head_sha),
            Self::LocalRequest | Self::GitHubIssue(_) => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstructionDigest {
    source: String,
    sha256: String,
    precedence: u16,
}

impl InstructionDigest {
    /// Creates a validated instruction source digest.
    ///
    /// # Errors
    /// Returns [`ContractError`] for an empty source or non-SHA-256 digest.
    pub fn new(
        source: impl Into<String>,
        sha256: impl Into<String>,
        precedence: u16,
    ) -> Result<Self, ContractError> {
        let source = source.into();
        nonempty(&source, "instructionSource")?;
        let sha256 = sha256.into();
        if sha256.len() != 64 || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(ContractError::InvalidField("instructionSha256"));
        }
        Ok(Self {
            source,
            sha256,
            precedence,
        })
    }

    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    #[must_use]
    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    #[must_use]
    pub const fn precedence(&self) -> u16 {
        self.precedence
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationCommand {
    program: String,
    args: Vec<String>,
}

impl VerificationCommand {
    /// Creates a command with an explicit executable and argv.
    ///
    /// # Errors
    /// Returns [`ContractError`] when the executable is empty or contains control characters.
    pub fn new<P, A, I>(program: P, args: I) -> Result<Self, ContractError>
    where
        P: Into<String>,
        A: Into<String>,
        I: IntoIterator<Item = A>,
    {
        let program = program.into();
        nonempty(&program, "verificationProgram")?;
        if program.chars().any(char::is_control) {
            return Err(ContractError::InvalidField("verificationProgram"));
        }
        let args = args.into_iter().map(Into::into).collect::<Vec<String>>();
        if args
            .iter()
            .any(|argument| argument.chars().any(char::is_control))
        {
            return Err(ContractError::InvalidField("verificationArgument"));
        }
        Ok(Self { program, args })
    }

    #[must_use]
    pub fn program(&self) -> &str {
        &self.program
    }

    #[must_use]
    pub fn args(&self) -> &[String] {
        &self.args
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RiskClass {
    Low,
    Moderate,
    High,
    Critical,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SensitivePath {
    path: String,
    reason: String,
}

impl SensitivePath {
    /// Creates a repository-relative sensitive path classification.
    ///
    /// # Errors
    /// Returns [`ContractError`] for absolute/traversing paths or an empty reason.
    pub fn new(path: impl Into<String>, reason: impl Into<String>) -> Result<Self, ContractError> {
        let path = path.into();
        let parsed = Path::new(&path);
        if path.trim().is_empty()
            || parsed.is_absolute()
            || parsed.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err(ContractError::InvalidField("sensitivePath"));
        }
        let reason = reason.into();
        nonempty(&reason, "sensitivePathReason")?;
        Ok(Self { path, reason })
    }

    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

#[derive(Clone, Debug)]
pub struct TaskContractDraft {
    pub task_id: TaskId,
    pub source: TaskSource,
    pub repository_binding_id: RepositoryBindingId,
    pub goal: String,
    pub acceptance_criteria: Vec<String>,
    pub base_sha: Option<String>,
    pub head_sha: Option<String>,
    pub instruction_digests: Vec<InstructionDigest>,
    pub verification_commands: Vec<VerificationCommand>,
    pub required_capabilities: Vec<Capability>,
    pub risk: RiskClass,
    pub sensitive_paths: Vec<SensitivePath>,
    pub dependencies: Vec<TaskId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskContract {
    version: u32,
    task_id: TaskId,
    source: TaskSource,
    repository_binding_id: RepositoryBindingId,
    goal: String,
    acceptance_criteria: Vec<String>,
    base_sha: Option<String>,
    head_sha: Option<String>,
    instruction_digests: Vec<InstructionDigest>,
    verification_commands: Vec<VerificationCommand>,
    required_capabilities: Vec<Capability>,
    risk: RiskClass,
    sensitive_paths: Vec<SensitivePath>,
    dependencies: Vec<TaskId>,
}

impl TryFrom<TaskContractDraft> for TaskContract {
    type Error = ContractError;

    fn try_from(mut draft: TaskContractDraft) -> Result<Self, Self::Error> {
        let goal = draft.goal.trim().to_owned();
        nonempty(&goal, "goal")?;
        for criterion in &mut draft.acceptance_criteria {
            *criterion = criterion.trim().to_owned();
        }
        if draft.acceptance_criteria.is_empty()
            || draft.acceptance_criteria.iter().any(String::is_empty)
        {
            return Err(ContractError::InvalidField("acceptanceCriteria"));
        }
        if let Some(sha) = draft.base_sha.as_deref() {
            validate_git_sha(sha, "baseSha")?;
        }
        if let Some(sha) = draft.head_sha.as_deref() {
            validate_git_sha(sha, "headSha")?;
        }
        if let TaskSource::GitHubPullRequest(source) = &draft.source {
            if draft.base_sha.as_deref() != Some(source.base_sha.as_str())
                || draft.head_sha.as_deref() != Some(source.head_sha.as_str())
            {
                return Err(ContractError::InvalidField("pullRequestShaBinding"));
            }
        }
        let unique_dependencies = draft.dependencies.iter().copied().collect::<HashSet<_>>();
        if unique_dependencies.len() != draft.dependencies.len()
            || unique_dependencies.contains(&draft.task_id)
        {
            return Err(ContractError::InvalidField("dependencies"));
        }
        Ok(Self {
            version: 1,
            task_id: draft.task_id,
            source: draft.source,
            repository_binding_id: draft.repository_binding_id,
            goal,
            acceptance_criteria: draft.acceptance_criteria,
            base_sha: draft.base_sha,
            head_sha: draft.head_sha,
            instruction_digests: draft.instruction_digests,
            verification_commands: draft.verification_commands,
            required_capabilities: draft.required_capabilities,
            risk: draft.risk,
            sensitive_paths: draft.sensitive_paths,
            dependencies: draft.dependencies,
        })
    }
}

impl TaskContract {
    #[must_use]
    pub const fn version(&self) -> u32 {
        self.version
    }

    #[must_use]
    pub const fn repository_binding_id(&self) -> RepositoryBindingId {
        self.repository_binding_id
    }

    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        self.task_id
    }

    #[must_use]
    pub const fn source(&self) -> &TaskSource {
        &self.source
    }

    #[must_use]
    pub fn goal(&self) -> &str {
        &self.goal
    }

    #[must_use]
    pub fn acceptance_criteria(&self) -> &[String] {
        &self.acceptance_criteria
    }

    #[must_use]
    pub fn base_sha(&self) -> Option<&str> {
        self.base_sha.as_deref()
    }

    #[must_use]
    pub fn head_sha(&self) -> Option<&str> {
        self.head_sha.as_deref()
    }

    #[must_use]
    pub fn instruction_digests(&self) -> &[InstructionDigest] {
        &self.instruction_digests
    }

    #[must_use]
    pub fn verification_commands(&self) -> &[VerificationCommand] {
        &self.verification_commands
    }

    #[must_use]
    pub fn required_capabilities(&self) -> &[Capability] {
        &self.required_capabilities
    }

    #[must_use]
    pub const fn risk(&self) -> RiskClass {
        self.risk
    }

    #[must_use]
    pub fn sensitive_paths(&self) -> &[SensitivePath] {
        &self.sensitive_paths
    }

    #[must_use]
    pub fn dependencies(&self) -> &[TaskId] {
        &self.dependencies
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ContractError {
    #[error("invalid contract field: {0}")]
    InvalidField(&'static str),
}

fn validate_github_item(
    repository_id: u64,
    repository_full_name: &str,
    number: u64,
    html_url: &str,
) -> Result<(), ContractError> {
    nonzero(repository_id, "repositoryId")?;
    validate_repository_name(repository_full_name)?;
    nonzero(number, "itemNumber")?;
    validate_https_url(html_url, "htmlUrl")
}

fn validate_repository_name(value: &str) -> Result<(), ContractError> {
    let Some((owner, name)) = value.split_once('/') else {
        return Err(ContractError::InvalidField("repositoryFullName"));
    };
    if owner.trim().is_empty()
        || name.trim().is_empty()
        || name.contains('/')
        || value
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(ContractError::InvalidField("repositoryFullName"));
    }
    Ok(())
}

fn validate_ref(value: &str, field: &'static str) -> Result<(), ContractError> {
    nonempty(value, field)?;
    if value.chars().any(char::is_control) {
        return Err(ContractError::InvalidField(field));
    }
    Ok(())
}

fn validate_https_url(value: &str, field: &'static str) -> Result<(), ContractError> {
    let valid = value.strip_prefix("https://").is_some_and(|remainder| {
        !remainder.is_empty() && !remainder.chars().any(char::is_whitespace)
    });
    if !valid {
        return Err(ContractError::InvalidField(field));
    }
    Ok(())
}

fn validate_git_sha(value: &str, field: &'static str) -> Result<(), ContractError> {
    if !matches!(value.len(), 40 | 64) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ContractError::InvalidField(field));
    }
    Ok(())
}

fn validate_optional_absolute_path(
    value: Option<&str>,
    field: &'static str,
) -> Result<(), ContractError> {
    if let Some(value) = value {
        validate_absolute_path(value, field)?;
    }
    Ok(())
}

fn validate_absolute_path(value: &str, field: &'static str) -> Result<(), ContractError> {
    if !Path::new(value).is_absolute() {
        return Err(ContractError::InvalidField(field));
    }
    Ok(())
}

fn nonempty(value: &str, field: &'static str) -> Result<(), ContractError> {
    if value.trim().is_empty() {
        return Err(ContractError::InvalidField(field));
    }
    Ok(())
}

fn nonzero(value: u64, field: &'static str) -> Result<(), ContractError> {
    if value == 0 {
        return Err(ContractError::InvalidField(field));
    }
    Ok(())
}
