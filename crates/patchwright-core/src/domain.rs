use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{fmt, path::Path, str::FromStr};
use thiserror::Error;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TaskId(Uuid);

impl TaskId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for TaskId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for TaskId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for TaskId {
    type Err = uuid::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(value).map(Self)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskState {
    Discovered,
    Planned,
    AwaitingApproval,
    Preparing,
    Implementing,
    Verifying,
    Reviewing,
    AwaitingDeliveryApproval,
    Delivering,
    Monitoring,
    Completed,
    Failed,
    Cancelled,
}

impl fmt::Display for TaskState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = serde_json::to_value(self).expect("TaskState serialization is infallible");
        formatter.write_str(value.as_str().expect("TaskState serializes as a string"))
    }
}

impl TaskState {
    fn permits(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Discovered, Self::Planned)
                | (Self::Planned, Self::AwaitingApproval)
                | (Self::AwaitingApproval, Self::Preparing)
                | (Self::Preparing, Self::Implementing)
                | (Self::Implementing, Self::Verifying)
                | (Self::Verifying, Self::Reviewing)
                | (Self::Reviewing, Self::AwaitingDeliveryApproval)
                | (Self::AwaitingDeliveryApproval, Self::Delivering)
                | (Self::Delivering, Self::Monitoring)
                | (Self::Monitoring, Self::Completed)
        ) || (!matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
            && matches!(next, Self::Failed | Self::Cancelled))
    }
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum ValidationError {
    #[error("title must not be empty")]
    EmptyTitle,
    #[error("repository path must be absolute")]
    RelativeRepository,
    #[error("invalid transition: {from} -> {to}")]
    InvalidTransition { from: TaskState, to: TaskState },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: TaskId,
    pub title: String,
    pub repository_path: String,
    pub state: TaskState,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Task {
    /// Creates a validated task rooted at an absolute repository path.
    ///
    /// # Errors
    /// Returns [`ValidationError`] when the title is empty or the repository path is relative.
    pub fn new(
        title: impl Into<String>,
        repository_path: impl Into<String>,
    ) -> Result<Self, ValidationError> {
        let title = title.into().trim().to_owned();
        if title.is_empty() {
            return Err(ValidationError::EmptyTitle);
        }
        let repository_path = repository_path.into();
        if !Path::new(&repository_path).is_absolute() {
            return Err(ValidationError::RelativeRepository);
        }
        let now = Utc::now();
        Ok(Self {
            id: TaskId::new(),
            title,
            repository_path,
            state: TaskState::Discovered,
            created_at: now,
            updated_at: now,
        })
    }

    #[must_use]
    pub const fn state(&self) -> TaskState {
        self.state
    }

    /// Advances the task through the explicit lifecycle.
    ///
    /// # Errors
    /// Returns [`ValidationError::InvalidTransition`] when the requested edge is not legal.
    pub fn transition(&mut self, next: TaskState) -> Result<(), ValidationError> {
        if !self.state.permits(next) {
            return Err(ValidationError::InvalidTransition {
                from: self.state,
                to: next,
            });
        }
        self.state = next;
        self.updated_at = Utc::now();
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskEvent {
    pub task_id: TaskId,
    pub sequence: u64,
    pub state: TaskState,
    pub summary: String,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FindingSeverity {
    P0,
    P1,
    P2,
    P3,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub severity: FindingSeverity,
    pub confidence_percent: u8,
    pub path: String,
    pub start_line: u32,
    pub end_line: u32,
    pub title: String,
    pub failure_scenario: String,
    pub evidence: String,
    pub suggested_test: String,
    pub instruction_sources: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    pub task_id: TaskId,
    pub kind: String,
    pub summary: String,
    pub content_sha256: String,
    pub artifact_path: Option<String>,
    pub recorded_at: DateTime<Utc>,
}
