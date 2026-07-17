use crate::{RepositoryBindingId, TaskSource};
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
    Assessing,
    Planned,
    #[serde(alias = "awaitingApproval")]
    AwaitingPreparationApproval,
    Preparing,
    Implementing,
    Verifying,
    Reviewing,
    AwaitingDeliveryApproval,
    Delivering,
    Monitoring,
    AwaitingMergeApproval,
    Merging,
    Paused,
    Blocked,
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
            (Self::Discovered, Self::Assessing)
                | (Self::Assessing, Self::Planned)
                | (Self::Planned, Self::AwaitingPreparationApproval)
                | (Self::AwaitingPreparationApproval, Self::Preparing)
                | (Self::Preparing, Self::Implementing)
                | (Self::Implementing, Self::Verifying)
                | (Self::Verifying, Self::Reviewing)
                | (Self::Reviewing, Self::AwaitingDeliveryApproval)
                | (Self::AwaitingDeliveryApproval, Self::Delivering)
                | (Self::Delivering, Self::Monitoring)
                | (
                    Self::Monitoring | Self::Merging,
                    Self::Completed | Self::AwaitingMergeApproval,
                )
                | (Self::AwaitingMergeApproval, Self::Merging)
        )
    }

    const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    const fn is_interruption(self) -> bool {
        matches!(
            self,
            Self::Paused | Self::Blocked | Self::Failed | Self::Cancelled
        )
    }

    const fn is_recoverable_interruption(self) -> bool {
        matches!(self, Self::Paused | Self::Blocked)
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
    #[error("interruption reason must not be empty")]
    EmptyInterruptionReason,
    #[error("{state} is not an interruption state")]
    InvalidInterruptionState { state: TaskState },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskInterruption {
    pub state: TaskState,
    pub resume_state: TaskState,
    pub reason: String,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interruption: Option<TaskInterruption>,
    #[serde(default)]
    pub source: TaskSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_binding_id: Option<RepositoryBindingId>,
    #[serde(default = "default_contract_version")]
    pub contract_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint_id: Option<Uuid>,
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
            interruption: None,
            source: TaskSource::LocalRequest,
            repository_binding_id: None,
            contract_version: default_contract_version(),
            checkpoint_id: None,
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

    /// Interrupts a nonterminal task while retaining its resumable state and evidence reason.
    ///
    /// # Errors
    /// Returns [`ValidationError`] when the target is not an interruption state, the task is
    /// terminal, or the reason is empty.
    pub fn interrupt(
        &mut self,
        interruption_state: TaskState,
        reason: impl Into<String>,
    ) -> Result<(), ValidationError> {
        if !interruption_state.is_interruption() {
            return Err(ValidationError::InvalidInterruptionState {
                state: interruption_state,
            });
        }
        if self.state.is_terminal() || self.state.is_interruption() {
            return Err(ValidationError::InvalidTransition {
                from: self.state,
                to: interruption_state,
            });
        }
        let reason = reason.into().trim().to_owned();
        if reason.is_empty() {
            return Err(ValidationError::EmptyInterruptionReason);
        }
        let resume_state = self.state;
        self.state = interruption_state;
        self.interruption = Some(TaskInterruption {
            state: interruption_state,
            resume_state,
            reason,
        });
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Resumes a paused or blocked task at its recorded prior state.
    ///
    /// # Errors
    /// Returns [`ValidationError::InvalidTransition`] for terminal or nonrecoverable states.
    pub fn resume(&mut self) -> Result<(), ValidationError> {
        let Some(interruption) = self.interruption.as_ref() else {
            return Err(ValidationError::InvalidTransition {
                from: self.state,
                to: self.state,
            });
        };
        if !self.state.is_recoverable_interruption() || interruption.state != self.state {
            return Err(ValidationError::InvalidTransition {
                from: self.state,
                to: interruption.resume_state,
            });
        }
        self.state = interruption.resume_state;
        self.interruption = None;
        self.updated_at = Utc::now();
        Ok(())
    }

    #[must_use]
    pub const fn interruption(&self) -> Option<&TaskInterruption> {
        self.interruption.as_ref()
    }
}

const fn default_contract_version() -> u32 {
    1
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
