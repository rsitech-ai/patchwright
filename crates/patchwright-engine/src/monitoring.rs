use chrono::{DateTime, Duration, Utc};
use patchwright_core::TaskId;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

const INITIAL_BACKOFF_SECONDS: i64 = 30;
const MAX_BACKOFF_SECONDS: i64 = 15 * 60;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CIState {
    Pending,
    Success,
    Failure,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ReviewState {
    Pending,
    Approved,
    ChangesRequested,
    ApprovalDismissed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Mergeability {
    Unknown,
    Mergeable,
    Conflicting,
    InaccessibleFork,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MonitorState {
    Pending,
    RepairNeeded,
    Succeeded,
    Blocked,
    Cancelled,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteObservation {
    pub observed_at: DateTime<Utc>,
    pub head_sha: String,
    pub base_sha: String,
    pub ci: CIState,
    pub review: ReviewState,
    pub mergeability: Mergeability,
    pub repository_accessible: bool,
    pub network_available: bool,
    pub rate_limited_until: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorOutcome {
    pub state: MonitorState,
    pub summary: String,
    pub invalidate_approvals: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorRecord {
    pub id: Uuid,
    pub task_id: TaskId,
    pub repository_full_name: String,
    pub pull_request_number: u64,
    pub expected_head_sha: String,
    pub expected_base_sha: String,
    pub state: MonitorState,
    pub attempt_count: u32,
    pub repair_iteration: u32,
    pub repair_budget: u32,
    pub next_attempt_at: Option<DateTime<Utc>>,
    pub latest_observation: Option<RemoteObservation>,
    pub summary: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl MonitorRecord {
    /// Creates a durable monitor bound to one repository, pull request, and exact SHA pair.
    ///
    /// # Errors
    /// Rejects invalid repository names, pull-request numbers, SHA values, or a zero repair budget.
    pub fn new(
        task_id: TaskId,
        repository_full_name: impl Into<String>,
        pull_request_number: u64,
        expected_head_sha: String,
        expected_base_sha: String,
        now: DateTime<Utc>,
        repair_budget: u32,
    ) -> Result<Self, MonitoringError> {
        let repository_full_name = repository_full_name.into();
        if repository_full_name.split_once('/').is_none()
            || repository_full_name.chars().any(char::is_whitespace)
        {
            return Err(MonitoringError::InvalidRepository);
        }
        if pull_request_number == 0 {
            return Err(MonitoringError::InvalidPullRequest);
        }
        validate_sha(&expected_head_sha)?;
        validate_sha(&expected_base_sha)?;
        if repair_budget == 0 {
            return Err(MonitoringError::InvalidRepairBudget);
        }
        Ok(Self {
            id: Uuid::new_v4(),
            task_id,
            repository_full_name,
            pull_request_number,
            expected_head_sha,
            expected_base_sha,
            state: MonitorState::Pending,
            attempt_count: 0,
            repair_iteration: 0,
            repair_budget,
            next_attempt_at: Some(now),
            latest_observation: None,
            summary: "waiting for first remote observation".into(),
            created_at: now,
            updated_at: now,
        })
    }

    /// Applies one untrusted remote observation without treating its text as authority.
    ///
    /// # Errors
    /// Rejects observations containing invalid SHA values or observations applied after termination.
    pub fn observe(
        &mut self,
        observation: RemoteObservation,
        now: DateTime<Utc>,
    ) -> Result<MonitorOutcome, MonitoringError> {
        if matches!(
            self.state,
            MonitorState::Succeeded | MonitorState::Blocked | MonitorState::Cancelled
        ) {
            return Err(MonitoringError::Terminal);
        }
        validate_sha(&observation.head_sha)?;
        validate_sha(&observation.base_sha)?;
        self.attempt_count = self.attempt_count.saturating_add(1);
        self.latest_observation = Some(observation.clone());
        self.updated_at = now;

        if !observation.network_available {
            return Ok(self.schedule(now, "network unavailable; monitoring will retry"));
        }
        if let Some(until) = observation.rate_limited_until.filter(|until| *until > now) {
            self.state = MonitorState::Pending;
            self.next_attempt_at = Some(until);
            self.summary = format!("GitHub rate limit active until {}", until.to_rfc3339());
            return Ok(self.outcome(false));
        }
        if !observation.repository_accessible {
            return Ok(self.block("repository or installation is no longer accessible", true));
        }
        if observation.head_sha != self.expected_head_sha {
            return Ok(self.block("pull request head SHA changed", true));
        }
        if observation.base_sha != self.expected_base_sha {
            return Ok(self.block("pull request base SHA changed", true));
        }
        match observation.mergeability {
            Mergeability::Conflicting => {
                return Ok(self.block("pull request now has merge conflicts", true));
            }
            Mergeability::InaccessibleFork => {
                return Ok(self.block("pull request fork is inaccessible", true));
            }
            Mergeability::Unknown => {
                return Ok(self.schedule(now, "mergeability is unknown; monitoring will retry"));
            }
            Mergeability::Mergeable => {}
        }

        let repair_reason = match (observation.ci, observation.review) {
            (CIState::Failure, _) => Some("CI failed"),
            (_, ReviewState::ChangesRequested) => Some("review requested changes"),
            (_, ReviewState::ApprovalDismissed) => Some("a prior review approval was dismissed"),
            _ => None,
        };
        if let Some(reason) = repair_reason {
            if self.repair_iteration >= self.repair_budget {
                return Ok(self.block(&format!("repair budget exhausted after {reason}"), true));
            }
            self.repair_iteration += 1;
            self.state = MonitorState::RepairNeeded;
            self.next_attempt_at = None;
            self.summary = format!(
                "{reason}; repair iteration {} of {} requires a new plan",
                self.repair_iteration, self.repair_budget
            );
            return Ok(self.outcome(true));
        }
        if observation.ci == CIState::Success && observation.review == ReviewState::Approved {
            self.state = MonitorState::Succeeded;
            self.next_attempt_at = None;
            self.summary = "CI passed and review is approved".into();
            return Ok(self.outcome(false));
        }
        Ok(self.schedule(now, "CI or review is still pending"))
    }

    #[must_use]
    pub fn wake(&mut self, now: DateTime<Utc>) -> bool {
        if self.state != MonitorState::Pending {
            return false;
        }
        self.next_attempt_at = Some(now);
        self.updated_at = now;
        self.summary = "webhook requested an immediate refresh".into();
        true
    }

    #[must_use]
    pub fn cancel(&mut self, now: DateTime<Utc>) -> bool {
        if matches!(
            self.state,
            MonitorState::Succeeded | MonitorState::Blocked | MonitorState::Cancelled
        ) {
            return false;
        }
        self.state = MonitorState::Cancelled;
        self.next_attempt_at = None;
        self.updated_at = now;
        self.summary = "monitoring cancelled by the operator".into();
        true
    }

    fn schedule(&mut self, now: DateTime<Utc>, summary: &str) -> MonitorOutcome {
        let exponent = self.attempt_count.saturating_sub(1).min(5);
        let backoff = (INITIAL_BACKOFF_SECONDS * (1_i64 << exponent)).min(MAX_BACKOFF_SECONDS);
        let jitter = i64::try_from((self.id.as_u128() ^ u128::from(self.attempt_count)) % 11)
            .expect("bounded jitter fits in i64");
        self.state = MonitorState::Pending;
        self.next_attempt_at = Some(now + Duration::seconds(backoff + jitter));
        self.summary = summary.into();
        self.outcome(false)
    }

    fn block(&mut self, summary: &str, invalidate_approvals: bool) -> MonitorOutcome {
        self.state = MonitorState::Blocked;
        self.next_attempt_at = None;
        self.summary = summary.into();
        self.outcome(invalidate_approvals)
    }

    fn outcome(&self, invalidate_approvals: bool) -> MonitorOutcome {
        MonitorOutcome {
            state: self.state,
            summary: self.summary.clone(),
            invalidate_approvals,
        }
    }
}

fn validate_sha(value: &str) -> Result<(), MonitoringError> {
    if value.len() != 40 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(MonitoringError::InvalidSha);
    }
    Ok(())
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum MonitoringError {
    #[error("monitor repository name is invalid")]
    InvalidRepository,
    #[error("monitor pull request number is invalid")]
    InvalidPullRequest,
    #[error("monitor SHA is invalid")]
    InvalidSha,
    #[error("monitor repair budget must be positive")]
    InvalidRepairBudget,
    #[error("monitor is already terminal")]
    Terminal,
}
