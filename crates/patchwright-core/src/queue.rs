#![allow(clippy::missing_errors_doc)]

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkflowPreset {
    QuickWins,
    CiRescue,
    ReviewClosure,
    ConflictRecovery,
    DependencyChain,
    SecurityFirst,
    ReleaseTrain,
    StalePullRequestTriage,
    DraftCompletion,
    PostMergeWatch,
    ReviewLoadBalancing,
    DuplicateOverlapDetection,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum QueueTier {
    Critical,
    Ready,
    Repair,
    Review,
    Draft,
    Stale,
    Blocked,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueCandidate {
    pub repository_full_name: String,
    pub number: u64,
    pub title: String,
    pub draft: bool,
    pub ci_health: Option<String>,
    pub review_decision: Option<String>,
    pub has_conflicts: Option<bool>,
    pub updated_at: DateTime<Utc>,
    pub labels: Vec<String>,
    pub dependency_numbers: Vec<u64>,
    pub changed_paths: Vec<String>,
    pub manual_priority: Option<i64>,
    pub pinned: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueDecision {
    pub repository_full_name: String,
    pub number: u64,
    pub tier: QueueTier,
    pub score: i64,
    pub reasons: Vec<String>,
    pub decision_input_sha256: String,
}

pub fn assess_queue(
    candidates: &[QueueCandidate],
    preset: WorkflowPreset,
    now: DateTime<Utc>,
) -> Result<Vec<QueueDecision>, QueueError> {
    validate_candidates(candidates)?;
    let identities: HashSet<(&str, u64)> = candidates
        .iter()
        .map(|item| (item.repository_full_name.as_str(), item.number))
        .collect();
    let mut decisions: Vec<QueueDecision> = candidates
        .iter()
        .map(|candidate| assess_candidate(candidate, preset, now, &identities, candidates))
        .collect();
    decisions.sort_by(|left, right| {
        left.tier
            .cmp(&right.tier)
            .then_with(|| right.score.cmp(&left.score))
            .then_with(|| left.repository_full_name.cmp(&right.repository_full_name))
            .then_with(|| left.number.cmp(&right.number))
    });
    Ok(decisions)
}

fn validate_candidates(candidates: &[QueueCandidate]) -> Result<(), QueueError> {
    let mut identities = HashSet::new();
    for candidate in candidates {
        if candidate.number == 0
            || candidate.repository_full_name.split_once('/').is_none()
            || candidate.title.trim().is_empty()
            || !identities.insert((&candidate.repository_full_name, candidate.number))
            || candidate.dependency_numbers.contains(&candidate.number)
            || candidate.changed_paths.iter().any(|path| {
                path.is_empty()
                    || path.starts_with('/')
                    || path.contains("..")
                    || path.contains('\0')
            })
        {
            return Err(QueueError::InvalidCandidate);
        }
    }
    Ok(())
}

fn assess_candidate(
    candidate: &QueueCandidate,
    preset: WorkflowPreset,
    now: DateTime<Utc>,
    identities: &HashSet<(&str, u64)>,
    candidates: &[QueueCandidate],
) -> QueueDecision {
    let mut reasons = Vec::new();
    let mut score = candidate.manual_priority.unwrap_or_default();
    if candidate.pinned {
        score += 2_000;
        reasons.push("Manually pinned".into());
    }
    let security = candidate.labels.iter().any(|label| {
        matches!(
            label.to_ascii_lowercase().as_str(),
            "security" | "vulnerability" | "cve"
        )
    });
    let unknown = candidate.ci_health.is_none()
        || candidate.review_decision.is_none()
        || candidate.has_conflicts.is_none();
    let stale = now - candidate.updated_at >= Duration::days(30);
    let failing = matches!(candidate.ci_health.as_deref(), Some("failing" | "failure"));
    let changes_requested = matches!(
        candidate.review_decision.as_deref(),
        Some("changesRequested" | "changes_requested")
    );
    let conflicts = candidate.has_conflicts == Some(true);
    let approved = candidate.review_decision.as_deref() == Some("approved");
    let passing = matches!(candidate.ci_health.as_deref(), Some("passing" | "success"));

    let mut tier = if security {
        score += 1_000;
        reasons.push("Security-sensitive change".into());
        QueueTier::Critical
    } else if unknown {
        reasons.push("Unknown CI, review, or mergeability state".into());
        QueueTier::Blocked
    } else if conflicts {
        score += 820;
        reasons.push("Conflict recovery required".into());
        QueueTier::Repair
    } else if failing {
        score += 800;
        reasons.push("CI is failing".into());
        QueueTier::Repair
    } else if changes_requested {
        score += 700;
        reasons.push("Requested review changes remain".into());
        QueueTier::Review
    } else if candidate.draft {
        score += 400;
        reasons.push("Draft completion required".into());
        QueueTier::Draft
    } else if approved && passing {
        score += 900;
        reasons.push("Approved with passing CI".into());
        QueueTier::Ready
    } else {
        score += 600;
        reasons.push("Awaiting review closure".into());
        QueueTier::Review
    };
    if stale {
        reasons.push("Stale for at least 30 days".into());
        if tier != QueueTier::Blocked {
            tier = QueueTier::Stale;
        }
    }
    apply_preset(
        preset,
        candidate,
        &mut score,
        &mut reasons,
        security,
        failing,
        changes_requested,
        conflicts,
        stale,
    );
    for dependency in &candidate.dependency_numbers {
        if identities.contains(&(candidate.repository_full_name.as_str(), *dependency)) {
            tier = QueueTier::Blocked;
            score -= 500;
            reasons.push(format!("Blocked by #{dependency}"));
        }
    }
    if let Some(path) = overlapping_path(candidate, candidates) {
        reasons.push(format!("Overlaps another open PR at {path}"));
        if preset == WorkflowPreset::DuplicateOverlapDetection {
            score += 300;
        }
    }
    let input =
        serde_json::to_vec(&(candidate, preset)).expect("validated queue candidate serializes");
    QueueDecision {
        repository_full_name: candidate.repository_full_name.clone(),
        number: candidate.number,
        tier,
        score,
        reasons,
        decision_input_sha256: sha256_hex(&input),
    }
}

#[allow(clippy::too_many_arguments, clippy::fn_params_excessive_bools)]
fn apply_preset(
    preset: WorkflowPreset,
    candidate: &QueueCandidate,
    score: &mut i64,
    reasons: &mut Vec<String>,
    security: bool,
    failing: bool,
    changes_requested: bool,
    conflicts: bool,
    stale: bool,
) {
    let matched = match preset {
        WorkflowPreset::QuickWins => !candidate.draft && !failing && !conflicts,
        WorkflowPreset::CiRescue => failing,
        WorkflowPreset::ReviewClosure | WorkflowPreset::ReviewLoadBalancing => changes_requested,
        WorkflowPreset::ConflictRecovery => conflicts,
        WorkflowPreset::DependencyChain => !candidate.dependency_numbers.is_empty(),
        WorkflowPreset::SecurityFirst => security,
        WorkflowPreset::ReleaseTrain => candidate.labels.iter().any(|label| label == "release"),
        WorkflowPreset::StalePullRequestTriage => stale,
        WorkflowPreset::DraftCompletion => candidate.draft,
        WorkflowPreset::PostMergeWatch => {
            candidate.labels.iter().any(|label| label == "post-merge")
        }
        WorkflowPreset::DuplicateOverlapDetection => !candidate.changed_paths.is_empty(),
    };
    if matched {
        *score += 250;
        reasons.push(format!("Matches {} workflow", preset_name(preset)));
    }
}

const fn preset_name(preset: WorkflowPreset) -> &'static str {
    match preset {
        WorkflowPreset::QuickWins => "Quick Wins",
        WorkflowPreset::CiRescue => "CI Rescue",
        WorkflowPreset::ReviewClosure => "Review Closure",
        WorkflowPreset::ConflictRecovery => "Conflict Recovery",
        WorkflowPreset::DependencyChain => "Dependency Chain",
        WorkflowPreset::SecurityFirst => "Security First",
        WorkflowPreset::ReleaseTrain => "Release Train",
        WorkflowPreset::StalePullRequestTriage => "Stale PR Triage",
        WorkflowPreset::DraftCompletion => "Draft Completion",
        WorkflowPreset::PostMergeWatch => "Post-Merge Watch",
        WorkflowPreset::ReviewLoadBalancing => "Review Load Balancing",
        WorkflowPreset::DuplicateOverlapDetection => "Duplicate/Overlap Detection",
    }
}

fn overlapping_path(candidate: &QueueCandidate, candidates: &[QueueCandidate]) -> Option<String> {
    let paths: HashSet<&str> = candidate.changed_paths.iter().map(String::as_str).collect();
    candidates
        .iter()
        .filter(|other| {
            other.number != candidate.number
                && other.repository_full_name == candidate.repository_full_name
        })
        .flat_map(|other| other.changed_paths.iter())
        .find(|path| paths.contains(path.as_str()))
        .cloned()
}

fn sha256_hex(input: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(input))
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum QueueError {
    #[error("invalid or duplicate pull request queue candidate")]
    InvalidCandidate,
}
