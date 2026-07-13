use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Capability {
    ReadRepository,
    ModifyWorktree,
    RunKnownCommand,
    AccessNetwork,
    InstallDependency,
    PushBranch,
    CreatePullRequest,
    PostReview,
    ResolveThread,
    ModifyWorkflow,
    MergePullRequest,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Approval {
    pub id: Uuid,
    pub capability: Capability,
    pub approved_by: String,
    pub approved_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

impl Approval {
    #[must_use]
    pub fn for_capability(capability: Capability, approved_by: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            capability,
            approved_by: approved_by.into(),
            approved_at: now,
            expires_at: now + Duration::minutes(30),
        }
    }

    fn is_valid_for(&self, capability: Capability, now: DateTime<Utc>) -> bool {
        self.capability == capability && self.expires_at >= now
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
    pub fn authorize(&self, capability: Capability, approval: Option<&Approval>) -> PolicyDecision {
        if capability == Capability::MergePullRequest {
            return PolicyDecision::Denied("merge is disabled".into());
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
        if capability == Capability::ModifyWorkflow {
            return match approval.filter(|value| value.is_valid_for(capability, Utc::now())) {
                Some(_) => PolicyDecision::Allowed,
                None => PolicyDecision::ApprovalRequired(
                    "workflow changes always require approval".into(),
                ),
            };
        }
        match approval.filter(|value| value.is_valid_for(capability, Utc::now())) {
            Some(_) => PolicyDecision::Allowed,
            None => PolicyDecision::ApprovalRequired(format!("{capability:?} requires approval")),
        }
    }
}
