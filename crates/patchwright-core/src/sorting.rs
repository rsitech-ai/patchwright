use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, collections::BTreeSet};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum SortDirection {
    Ascending,
    Descending,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum RepositorySortKey {
    QueuePriority,
    RecentlyUpdated,
    RecentlyPushed,
    LatestDefaultBranchCommit,
    OpenPullRequestCount,
    FailingCheckCount,
    Name,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RepositorySort {
    pub key: RepositorySortKey,
    pub direction: SortDirection,
}

impl RepositorySort {
    #[must_use]
    pub const fn new(key: RepositorySortKey, direction: SortDirection) -> Self {
        Self { key, direction }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryQueueRecord {
    pub id: u64,
    pub full_name: String,
    pub queue_priority: Option<i64>,
    pub updated_at: DateTime<Utc>,
    pub pushed_at: Option<DateTime<Utc>>,
    pub default_branch_committed_at: Option<DateTime<Utc>>,
    pub open_pull_request_count: u64,
    pub failing_check_count: u64,
}

#[must_use]
pub fn sort_repositories(
    records: &[RepositoryQueueRecord],
    sort: RepositorySort,
) -> Vec<RepositoryQueueRecord> {
    let mut sorted = records.to_vec();
    sorted.sort_by(|left, right| {
        repository_primary_order(left, right, sort)
            .then_with(|| left.full_name.cmp(&right.full_name))
            .then_with(|| left.id.cmp(&right.id))
    });
    sorted
}

fn repository_primary_order(
    left: &RepositoryQueueRecord,
    right: &RepositoryQueueRecord,
    sort: RepositorySort,
) -> Ordering {
    match sort.key {
        RepositorySortKey::QueuePriority => {
            compare_optional(left.queue_priority, right.queue_priority, sort.direction)
        }
        RepositorySortKey::RecentlyUpdated => {
            compare_value(&left.updated_at, &right.updated_at, sort.direction)
        }
        RepositorySortKey::RecentlyPushed => {
            compare_optional(left.pushed_at, right.pushed_at, sort.direction)
        }
        RepositorySortKey::LatestDefaultBranchCommit => compare_optional(
            left.default_branch_committed_at,
            right.default_branch_committed_at,
            sort.direction,
        ),
        RepositorySortKey::OpenPullRequestCount => compare_value(
            &left.open_pull_request_count,
            &right.open_pull_request_count,
            sort.direction,
        ),
        RepositorySortKey::FailingCheckCount => compare_value(
            &left.failing_check_count,
            &right.failing_check_count,
            sort.direction,
        ),
        RepositorySortKey::Name => compare_value(&left.full_name, &right.full_name, sort.direction),
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub enum CiHealth {
    Failing,
    Pending,
    Passing,
    Unknown,
}

impl CiHealth {
    const fn sort_rank(self) -> Option<u8> {
        match self {
            Self::Failing => Some(1),
            Self::Pending => Some(2),
            Self::Passing => Some(3),
            Self::Unknown => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub enum ReviewState {
    ChangesRequested,
    ReviewRequired,
    Dismissed,
    Approved,
    Unknown,
}

impl ReviewState {
    const fn sort_rank(self) -> Option<u8> {
        match self {
            Self::ChangesRequested => Some(1),
            Self::ReviewRequired => Some(2),
            Self::Dismissed => Some(3),
            Self::Approved => Some(4),
            Self::Unknown => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "camelCase")]
pub enum PullRequestQueueState {
    Inbox,
    Assessed,
    Ready,
    NeedsWork,
    Blocked,
    Active,
    AwaitingWriteApproval,
    Monitoring,
    MergeReady,
    AwaitingMergeApproval,
    Merged,
    Failed,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum PullRequestSortKey {
    QueuePriority,
    RecentlyUpdated,
    LatestHeadCommit,
    LatestReviewActivity,
    CiHealth,
    ReviewState,
    CreatedNewest,
    CreatedOldest,
    ChangeSize,
    Number,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestSort {
    pub key: PullRequestSortKey,
    pub direction: SortDirection,
}

impl PullRequestSort {
    #[must_use]
    pub const fn new(key: PullRequestSortKey, direction: SortDirection) -> Self {
        Self { key, direction }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestQueueRecord {
    pub id: u64,
    pub number: u64,
    pub queue_priority: Option<i64>,
    pub updated_at: DateTime<Utc>,
    pub head_committed_at: Option<DateTime<Utc>>,
    pub latest_review_at: Option<DateTime<Utc>>,
    pub ci_health: Option<CiHealth>,
    pub review_state: Option<ReviewState>,
    pub created_at: DateTime<Utc>,
    pub additions: u64,
    pub deletions: u64,
    pub open: bool,
    pub draft: bool,
    pub author: String,
    pub assignees: BTreeSet<String>,
    pub labels: BTreeSet<String>,
    pub has_conflicts: Option<bool>,
    pub queue_state: Option<PullRequestQueueState>,
    pub active_codex_work: bool,
}

#[must_use]
pub fn sort_pull_requests(
    records: &[PullRequestQueueRecord],
    sort: PullRequestSort,
) -> Vec<PullRequestQueueRecord> {
    let mut sorted = records.to_vec();
    sorted.sort_by(|left, right| {
        pull_request_primary_order(left, right, sort)
            .then_with(|| left.number.cmp(&right.number))
            .then_with(|| left.id.cmp(&right.id))
    });
    sorted
}

fn pull_request_primary_order(
    left: &PullRequestQueueRecord,
    right: &PullRequestQueueRecord,
    sort: PullRequestSort,
) -> Ordering {
    match sort.key {
        PullRequestSortKey::QueuePriority => {
            compare_optional(left.queue_priority, right.queue_priority, sort.direction)
        }
        PullRequestSortKey::RecentlyUpdated => {
            compare_value(&left.updated_at, &right.updated_at, sort.direction)
        }
        PullRequestSortKey::LatestHeadCommit => compare_optional(
            left.head_committed_at,
            right.head_committed_at,
            sort.direction,
        ),
        PullRequestSortKey::LatestReviewActivity => compare_optional(
            left.latest_review_at,
            right.latest_review_at,
            sort.direction,
        ),
        PullRequestSortKey::CiHealth => compare_optional(
            left.ci_health.and_then(CiHealth::sort_rank),
            right.ci_health.and_then(CiHealth::sort_rank),
            sort.direction,
        ),
        PullRequestSortKey::ReviewState => compare_optional(
            left.review_state.and_then(ReviewState::sort_rank),
            right.review_state.and_then(ReviewState::sort_rank),
            sort.direction,
        ),
        PullRequestSortKey::CreatedNewest => {
            compare_value(&left.created_at, &right.created_at, sort.direction)
        }
        PullRequestSortKey::CreatedOldest => {
            compare_value(&right.created_at, &left.created_at, sort.direction)
        }
        PullRequestSortKey::ChangeSize => compare_value(
            &left.additions.saturating_add(left.deletions),
            &right.additions.saturating_add(right.deletions),
            sort.direction,
        ),
        PullRequestSortKey::Number => compare_value(&left.number, &right.number, sort.direction),
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct WorkspaceFilter {
    pub open: Option<bool>,
    pub draft: Option<bool>,
    pub authors: BTreeSet<String>,
    pub assignees: BTreeSet<String>,
    pub labels: BTreeSet<String>,
    pub review_states: BTreeSet<ReviewState>,
    pub ci_results: BTreeSet<CiHealth>,
    pub has_conflicts: Option<bool>,
    pub maximum_age_days: Option<u32>,
    pub queue_states: BTreeSet<PullRequestQueueState>,
    pub active_codex_work: Option<bool>,
}

impl WorkspaceFilter {
    #[must_use]
    pub fn matches(&self, record: &PullRequestQueueRecord, now: DateTime<Utc>) -> bool {
        self.open.is_none_or(|expected| record.open == expected)
            && self.draft.is_none_or(|expected| record.draft == expected)
            && (self.authors.is_empty() || self.authors.contains(&record.author))
            && (self.assignees.is_empty()
                || record
                    .assignees
                    .iter()
                    .any(|assignee| self.assignees.contains(assignee)))
            && (self.labels.is_empty()
                || record
                    .labels
                    .iter()
                    .any(|label| self.labels.contains(label)))
            && (self.review_states.is_empty()
                || record
                    .review_state
                    .is_some_and(|state| self.review_states.contains(&state)))
            && (self.ci_results.is_empty()
                || record
                    .ci_health
                    .is_some_and(|health| self.ci_results.contains(&health)))
            && self
                .has_conflicts
                .is_none_or(|expected| record.has_conflicts == Some(expected))
            && self
                .maximum_age_days
                .is_none_or(|days| record.updated_at >= now - Duration::days(i64::from(days)))
            && (self.queue_states.is_empty()
                || record
                    .queue_state
                    .is_some_and(|state| self.queue_states.contains(&state)))
            && self
                .active_codex_work
                .is_none_or(|expected| record.active_codex_work == expected)
    }
}

fn compare_value<T: Ord>(left: &T, right: &T, direction: SortDirection) -> Ordering {
    apply_direction(left.cmp(right), direction)
}

fn compare_optional<T: Ord>(
    left: Option<T>,
    right: Option<T>,
    direction: SortDirection,
) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => apply_direction(left.cmp(&right), direction),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

const fn apply_direction(ordering: Ordering, direction: SortDirection) -> Ordering {
    match direction {
        SortDirection::Ascending => ordering,
        SortDirection::Descending => ordering.reverse(),
    }
}
