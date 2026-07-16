use chrono::{DateTime, Utc};
use patchwright_core::{TaskId, TaskState};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};
use thiserror::Error;
use uuid::Uuid;

const MAX_SUMMARY_BYTES: usize = 256;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JobId(Uuid);

impl JobId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for JobId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for JobId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for JobId {
    type Err = uuid::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(value).map(Self)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JobKind {
    GitHubSync,
    TaskExecution,
    GitHubDelivery,
    Merge,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JobState {
    Queued,
    Running,
    Cancelling,
    Cancelled,
    Succeeded,
    Failed,
    Interrupted,
}

impl JobState {
    pub(crate) const fn permits(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Queued, Self::Running | Self::Cancelled | Self::Failed)
                | (
                    Self::Running,
                    Self::Cancelling | Self::Succeeded | Self::Failed | Self::Interrupted
                )
                | (
                    Self::Cancelling,
                    Self::Cancelled | Self::Succeeded | Self::Failed | Self::Interrupted
                )
                | (
                    Self::Interrupted,
                    Self::Queued | Self::Cancelled | Self::Failed
                )
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CancellationState {
    NotRequested,
    Requested,
    Acknowledged,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobCheckpoint {
    pub(crate) sequence: u64,
    pub(crate) kind: String,
    pub(crate) summary: String,
    pub(crate) recorded_at: DateTime<Utc>,
}

impl JobCheckpoint {
    /// Creates a bounded durable job checkpoint.
    ///
    /// # Errors
    /// Returns [`JobError`] when kind or summary is not safe bounded metadata.
    pub fn new(
        sequence: u64,
        kind: impl Into<String>,
        summary: impl Into<String>,
    ) -> Result<Self, JobError> {
        let kind = validate_metadata(kind.into(), "checkpoint kind")?;
        let summary = validate_summary(summary.into())?;
        Ok(Self {
            sequence,
            kind,
            summary,
            recorded_at: Utc::now(),
        })
    }

    #[must_use]
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    #[must_use]
    pub fn kind(&self) -> &str {
        &self.kind
    }

    #[must_use]
    pub fn summary(&self) -> &str {
        &self.summary
    }

    #[must_use]
    pub const fn recorded_at(&self) -> DateTime<Utc> {
        self.recorded_at
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskCheckpoint {
    pub(crate) id: Uuid,
    pub(crate) task_id: TaskId,
    pub(crate) state: TaskState,
    pub(crate) summary: String,
    pub(crate) occurred_at: DateTime<Utc>,
}

impl TaskCheckpoint {
    /// Creates a bounded task state checkpoint.
    ///
    /// # Errors
    /// Returns [`JobError`] when summary is not safe bounded metadata.
    pub fn new(
        task_id: TaskId,
        state: TaskState,
        summary: impl Into<String>,
    ) -> Result<Self, JobError> {
        Ok(Self {
            id: Uuid::new_v4(),
            task_id,
            state,
            summary: validate_summary(summary.into())?,
            occurred_at: Utc::now(),
        })
    }

    #[must_use]
    pub const fn id(&self) -> Uuid {
        self.id
    }

    #[must_use]
    pub const fn task_id(&self) -> TaskId {
        self.task_id
    }

    #[must_use]
    pub const fn state(&self) -> TaskState {
        self.state
    }

    #[must_use]
    pub fn summary(&self) -> &str {
        &self.summary
    }

    #[must_use]
    pub const fn occurred_at(&self) -> DateTime<Utc> {
        self.occurred_at
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub(crate) id: JobId,
    pub(crate) kind: JobKind,
    pub(crate) task_id: Option<TaskId>,
    pub(crate) state: JobState,
    pub(crate) cancellation: CancellationState,
    pub(crate) summary: String,
    pub(crate) checkpoint: Option<JobCheckpoint>,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) updated_at: DateTime<Utc>,
    pub(crate) generation: u64,
}

impl Job {
    /// Creates a queued durable job with bounded non-sensitive summary metadata.
    ///
    /// # Errors
    /// Returns [`JobError`] for multiline, oversized, or credential-shaped summary text.
    pub fn new(
        kind: JobKind,
        task_id: Option<TaskId>,
        summary: impl Into<String>,
    ) -> Result<Self, JobError> {
        let now = Utc::now();
        Ok(Self {
            id: JobId::new(),
            kind,
            task_id,
            state: JobState::Queued,
            cancellation: CancellationState::NotRequested,
            summary: validate_summary(summary.into())?,
            checkpoint: None,
            created_at: now,
            updated_at: now,
            generation: 0,
        })
    }

    #[must_use]
    pub const fn id(&self) -> JobId {
        self.id
    }

    #[must_use]
    pub const fn kind(&self) -> JobKind {
        self.kind
    }

    #[must_use]
    pub const fn task_id(&self) -> Option<TaskId> {
        self.task_id
    }

    #[must_use]
    pub const fn state(&self) -> JobState {
        self.state
    }

    #[must_use]
    pub const fn cancellation(&self) -> CancellationState {
        self.cancellation
    }

    #[must_use]
    pub fn summary(&self) -> &str {
        &self.summary
    }

    #[must_use]
    pub const fn checkpoint(&self) -> Option<&JobCheckpoint> {
        self.checkpoint.as_ref()
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum JobError {
    #[error("invalid {0}")]
    InvalidMetadata(&'static str),
    #[error("invalid job transition: {from:?} -> {to:?}")]
    InvalidTransition { from: JobState, to: JobState },
}

pub(crate) fn validate_summary(value: String) -> Result<String, JobError> {
    let value = value.trim().to_owned();
    let lowercase = value.to_ascii_lowercase();
    let credential_shaped = [
        "bearer ",
        "gho_",
        "ghp_",
        "github_pat_",
        "token=",
        "-----begin",
    ]
    .iter()
    .any(|marker| lowercase.contains(marker));
    if value.is_empty()
        || value.len() > MAX_SUMMARY_BYTES
        || value
            .chars()
            .any(|character| matches!(character, '\n' | '\r' | '\0'))
        || credential_shaped
    {
        return Err(JobError::InvalidMetadata("summary"));
    }
    Ok(value)
}

fn validate_metadata(value: String, field: &'static str) -> Result<String, JobError> {
    let value = value.trim().to_owned();
    if value.is_empty() || value.len() > 64 || value.chars().any(char::is_control) {
        return Err(JobError::InvalidMetadata(field));
    }
    Ok(value)
}
