use chrono::{DateTime, Duration, TimeZone, Utc};
use patchwright_core::{
    CiHealth, PullRequestQueueRecord, PullRequestQueueState, PullRequestSort, PullRequestSortKey,
    RepositoryQueueRecord, RepositorySort, RepositorySortKey, ReviewState, SortDirection,
    WorkspaceFilter, sort_pull_requests, sort_repositories,
};
use std::collections::BTreeSet;

fn at(hour: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 13, hour, 0, 0)
        .single()
        .unwrap()
}

fn repository(id: u64, name: &str) -> RepositoryQueueRecord {
    RepositoryQueueRecord {
        id,
        full_name: name.into(),
        queue_priority: None,
        updated_at: at(12),
        pushed_at: None,
        default_branch_committed_at: None,
        open_pull_request_count: 0,
        failing_check_count: 0,
    }
}

fn pull_request(id: u64, number: u64) -> PullRequestQueueRecord {
    PullRequestQueueRecord {
        id,
        number,
        queue_priority: None,
        updated_at: at(12),
        head_committed_at: None,
        latest_review_at: None,
        ci_health: None,
        review_state: None,
        created_at: at(8),
        additions: 0,
        deletions: 0,
        open: true,
        draft: false,
        author: "alice".into(),
        assignees: BTreeSet::new(),
        labels: BTreeSet::new(),
        has_conflicts: None,
        queue_state: None,
        active_codex_work: false,
    }
}

#[test]
fn repository_sorts_cover_every_approved_mode_and_stable_ties() {
    let mut alpha = repository(3, "acme/alpha");
    alpha.queue_priority = Some(2);
    alpha.pushed_at = Some(at(10));
    alpha.default_branch_committed_at = Some(at(9));
    alpha.open_pull_request_count = 4;
    alpha.failing_check_count = 1;
    let mut beta = repository(2, "acme/beta");
    beta.queue_priority = Some(1);
    beta.updated_at = at(11);
    beta.pushed_at = Some(at(11));
    beta.default_branch_committed_at = Some(at(10));
    beta.open_pull_request_count = 2;
    beta.failing_check_count = 3;

    let cases = [
        (RepositorySortKey::QueuePriority, vec![3, 2]),
        (RepositorySortKey::RecentlyUpdated, vec![3, 2]),
        (RepositorySortKey::RecentlyPushed, vec![2, 3]),
        (RepositorySortKey::LatestDefaultBranchCommit, vec![2, 3]),
        (RepositorySortKey::OpenPullRequestCount, vec![3, 2]),
        (RepositorySortKey::FailingCheckCount, vec![2, 3]),
        (RepositorySortKey::Name, vec![2, 3]),
    ];
    for (key, expected) in cases {
        let sorted = sort_repositories(
            &[alpha.clone(), beta.clone()],
            RepositorySort::new(key, SortDirection::Descending),
        );
        assert_eq!(
            sorted.iter().map(|row| row.id).collect::<Vec<_>>(),
            expected
        );
    }

    let same_name = vec![repository(9, "acme/same"), repository(4, "acme/same")];
    let sorted = sort_repositories(
        &same_name,
        RepositorySort::new(RepositorySortKey::Name, SortDirection::Descending),
    );
    assert_eq!(sorted.iter().map(|row| row.id).collect::<Vec<_>>(), [4, 9]);
}

#[test]
fn repository_nil_timestamps_are_last_in_both_directions() {
    let mut known = repository(1, "acme/known");
    known.pushed_at = Some(at(10));
    let missing = repository(2, "acme/missing");
    for direction in [SortDirection::Ascending, SortDirection::Descending] {
        let sorted = sort_repositories(
            &[missing.clone(), known.clone()],
            RepositorySort::new(RepositorySortKey::RecentlyPushed, direction),
        );
        assert_eq!(sorted.iter().map(|row| row.id).collect::<Vec<_>>(), [1, 2]);
    }

    let mut newer = repository(3, "acme/newer");
    newer.pushed_at = Some(at(11));
    let ascending = sort_repositories(
        &[newer.clone(), known.clone()],
        RepositorySort::new(RepositorySortKey::RecentlyPushed, SortDirection::Ascending),
    );
    let descending = sort_repositories(
        &[known, newer],
        RepositorySort::new(RepositorySortKey::RecentlyPushed, SortDirection::Descending),
    );
    assert_eq!(
        ascending.iter().map(|row| row.id).collect::<Vec<_>>(),
        [1, 3]
    );
    assert_eq!(
        descending.iter().map(|row| row.id).collect::<Vec<_>>(),
        [3, 1]
    );
}

#[test]
fn pull_request_sorts_cover_every_approved_mode_and_stable_ties() {
    let mut first = pull_request(10, 2);
    first.queue_priority = Some(1);
    first.updated_at = at(11);
    first.head_committed_at = Some(at(10));
    first.latest_review_at = Some(at(9));
    first.ci_health = Some(CiHealth::Failing);
    first.review_state = Some(ReviewState::ChangesRequested);
    first.created_at = at(7);
    first.additions = 30;
    first.deletions = 10;
    let mut second = pull_request(11, 7);
    second.queue_priority = Some(2);
    second.updated_at = at(12);
    second.head_committed_at = Some(at(11));
    second.latest_review_at = Some(at(10));
    second.ci_health = Some(CiHealth::Passing);
    second.review_state = Some(ReviewState::Approved);
    second.created_at = at(8);
    second.additions = 2;
    second.deletions = 3;

    let cases = [
        (PullRequestSortKey::QueuePriority, vec![11, 10]),
        (PullRequestSortKey::RecentlyUpdated, vec![11, 10]),
        (PullRequestSortKey::LatestHeadCommit, vec![11, 10]),
        (PullRequestSortKey::LatestReviewActivity, vec![11, 10]),
        (PullRequestSortKey::CiHealth, vec![11, 10]),
        (PullRequestSortKey::ReviewState, vec![11, 10]),
        (PullRequestSortKey::CreatedNewest, vec![11, 10]),
        (PullRequestSortKey::CreatedOldest, vec![10, 11]),
        (PullRequestSortKey::ChangeSize, vec![10, 11]),
        (PullRequestSortKey::Number, vec![11, 10]),
    ];
    for (key, expected) in cases {
        let sorted = sort_pull_requests(
            &[first.clone(), second.clone()],
            PullRequestSort::new(key, SortDirection::Descending),
        );
        assert_eq!(
            sorted.iter().map(|row| row.id).collect::<Vec<_>>(),
            expected
        );
    }

    let same_number = vec![pull_request(9, 4), pull_request(4, 4)];
    let sorted = sort_pull_requests(
        &same_number,
        PullRequestSort::new(PullRequestSortKey::Number, SortDirection::Descending),
    );
    assert_eq!(sorted.iter().map(|row| row.id).collect::<Vec<_>>(), [4, 9]);
}

#[test]
fn pull_request_nil_and_unknown_values_are_last_in_both_directions() {
    let mut known = pull_request(1, 1);
    known.head_committed_at = Some(at(10));
    known.ci_health = Some(CiHealth::Pending);
    let mut unknown = pull_request(2, 2);
    unknown.ci_health = Some(CiHealth::Unknown);
    for direction in [SortDirection::Ascending, SortDirection::Descending] {
        let by_commit = sort_pull_requests(
            &[unknown.clone(), known.clone()],
            PullRequestSort::new(PullRequestSortKey::LatestHeadCommit, direction),
        );
        let by_ci = sort_pull_requests(
            &[unknown.clone(), known.clone()],
            PullRequestSort::new(PullRequestSortKey::CiHealth, direction),
        );
        assert_eq!(
            by_commit.iter().map(|row| row.id).collect::<Vec<_>>(),
            [1, 2]
        );
        assert_eq!(by_ci.iter().map(|row| row.id).collect::<Vec<_>>(), [1, 2]);
    }
}

#[test]
fn active_filters_use_and_semantics() {
    let mut candidate = pull_request(1, 1);
    candidate.draft = true;
    candidate.author = "octocat".into();
    candidate.assignees.insert("hubot".into());
    candidate.labels.insert("security".into());
    candidate.review_state = Some(ReviewState::ChangesRequested);
    candidate.ci_health = Some(CiHealth::Failing);
    candidate.has_conflicts = Some(true);
    candidate.queue_state = Some(PullRequestQueueState::NeedsWork);
    candidate.active_codex_work = true;
    candidate.updated_at = at(12);

    let filter = WorkspaceFilter {
        open: Some(true),
        draft: Some(true),
        authors: BTreeSet::from(["octocat".into()]),
        assignees: BTreeSet::from(["hubot".into()]),
        labels: BTreeSet::from(["security".into()]),
        review_states: BTreeSet::from([ReviewState::ChangesRequested]),
        ci_results: BTreeSet::from([CiHealth::Failing]),
        has_conflicts: Some(true),
        maximum_age_days: Some(1),
        queue_states: BTreeSet::from([PullRequestQueueState::NeedsWork]),
        active_codex_work: Some(true),
    };
    assert!(filter.matches(&candidate, at(13)));

    let mut mismatches = Vec::new();
    let mut mismatch = candidate.clone();
    mismatch.open = false;
    mismatches.push(mismatch);
    let mut mismatch = candidate.clone();
    mismatch.draft = false;
    mismatches.push(mismatch);
    let mut mismatch = candidate.clone();
    mismatch.author = "someone-else".into();
    mismatches.push(mismatch);
    let mut mismatch = candidate.clone();
    mismatch.assignees.clear();
    mismatches.push(mismatch);
    let mut mismatch = candidate.clone();
    mismatch.labels.clear();
    mismatches.push(mismatch);
    let mut mismatch = candidate.clone();
    mismatch.review_state = Some(ReviewState::Approved);
    mismatches.push(mismatch);
    let mut mismatch = candidate.clone();
    mismatch.ci_health = Some(CiHealth::Passing);
    mismatches.push(mismatch);
    let mut mismatch = candidate.clone();
    mismatch.has_conflicts = Some(false);
    mismatches.push(mismatch);
    let mut stale = candidate.clone();
    stale.updated_at = at(13) - Duration::days(2);
    mismatches.push(stale);
    let mut mismatch = candidate.clone();
    mismatch.queue_state = Some(PullRequestQueueState::Ready);
    mismatches.push(mismatch);
    let mut mismatch = candidate.clone();
    mismatch.active_codex_work = false;
    mismatches.push(mismatch);

    assert!(
        mismatches
            .iter()
            .all(|mismatch| !filter.matches(mismatch, at(13)))
    );
}

#[test]
fn empty_filter_matches_every_pull_request() {
    assert!(WorkspaceFilter::default().matches(&pull_request(1, 1), at(13)));
}
