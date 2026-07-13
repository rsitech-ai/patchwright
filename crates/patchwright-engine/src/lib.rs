#![allow(clippy::missing_errors_doc, clippy::needless_pass_by_value)]

mod command;
mod conversion;
mod github;
mod jobs;
mod repository;
mod rpc;
mod store;
mod worktree;

pub use command::{CommandOutput, CommandRunner, CommandSpec};
pub use conversion::{
    ConversionError, ConversionOutcome, ConversionPreview, ConversionRequest, TaskConversionService,
};
pub use github::{
    GhCliCredentialBroker, GitHubAccount, GitHubCheckRun, GitHubDiscussion, GitHubPermission,
    GitHubRepository, GitHubRepositoryPermissions, GitHubRepositorySnapshot, GitHubSource,
    GitHubSyncSummary, GitHubToken, GitHubWorkItem, GitHubWorkflowRun, WorkItemKind,
};
pub use jobs::{
    CancellationState, Job, JobCheckpoint, JobError, JobId, JobKind, JobState, TaskCheckpoint,
};
pub use repository::{RepositoryInspection, RepositoryService};
pub use rpc::serve;
pub use store::EventStore;
pub use worktree::WorktreeService;
