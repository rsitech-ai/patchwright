#![allow(clippy::missing_errors_doc, clippy::needless_pass_by_value)]

pub mod codex;
mod command;
mod conversion;
mod delivery;
mod github;
mod jobs;
mod lease;
mod monitoring;
mod repository;
mod rpc;
mod store;
mod verification;
mod worktree;

pub use command::{CommandOutput, CommandRunner, CommandSpec};
pub use conversion::{
    ConversionError, ConversionOutcome, ConversionPreview, ConversionRequest, TaskConversionService,
};
pub use delivery::{
    DeliveryError, DeliveryPreview, approve_delivery, authorize_execution,
    complete_successful_delivery, preview_delivery, reconcile_completed_task_from_snapshot,
};
pub use github::{
    GhCliCredentialBroker, GitHubAccount, GitHubCheckRun, GitHubDiscussion, GitHubPermission,
    GitHubRepository, GitHubRepositoryPermissions, GitHubRepositorySnapshot, GitHubSource,
    GitHubSyncSummary, GitHubToken, GitHubWorkItem, GitHubWorkflowRun, WorkItemKind,
};
pub use jobs::{
    CancellationState, Job, JobCheckpoint, JobError, JobId, JobKind, JobState, TaskCheckpoint,
};
pub use monitoring::{
    CIState, Mergeability, MonitorOutcome, MonitorRecord, MonitorState, MonitoringError,
    RemoteObservation, ReviewState,
};
pub use repository::{RepositoryInspection, RepositoryService};
pub use rpc::{serve, serve_until, serve_with_codex};
pub use store::EventStore;
pub use verification::{VerificationError, VerificationEvidence, verify_task_for_delivery};
pub use worktree::{GitTransport, WorktreeService};
