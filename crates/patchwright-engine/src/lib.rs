#![allow(clippy::missing_errors_doc, clippy::needless_pass_by_value)]

mod command;
mod github;
mod repository;
mod rpc;
mod store;
mod worktree;

pub use command::{CommandOutput, CommandRunner, CommandSpec};
pub use github::{
    GhCliCredentialBroker, GitHubAccount, GitHubCheckRun, GitHubDiscussion, GitHubRepository,
    GitHubRepositorySnapshot, GitHubSource, GitHubSyncSummary, GitHubToken, GitHubWorkItem,
    GitHubWorkflowRun, WorkItemKind,
};
pub use repository::{RepositoryInspection, RepositoryService};
pub use rpc::serve;
pub use store::EventStore;
pub use worktree::WorktreeService;
