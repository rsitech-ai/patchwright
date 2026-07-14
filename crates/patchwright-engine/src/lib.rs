#![allow(clippy::missing_errors_doc, clippy::needless_pass_by_value)]

pub mod codex;
mod command;
mod conversion;
mod delivery;
mod github;
mod jobs;
mod monitoring;
mod repository;
mod rpc;
mod store;
mod worktree;

pub use command::{CommandOutput, CommandRunner, CommandSpec};
pub use conversion::{
    ConversionError, ConversionOutcome, ConversionPreview, ConversionRequest, TaskConversionService,
};
pub use delivery::{
    DeliveryError, DeliveryPreview, approve_delivery, authorize_execution, preview_delivery,
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
pub use rpc::{serve, serve_with_codex};
pub use store::EventStore;
pub use worktree::WorktreeService;
