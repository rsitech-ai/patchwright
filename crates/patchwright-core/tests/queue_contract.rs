use chrono::{Duration, TimeZone, Utc};
use patchwright_core::{QueueCandidate, QueueTier, WorkflowPreset, assess_queue};

fn candidate(number: u64) -> QueueCandidate {
    QueueCandidate {
        repository_full_name: "octo/fixture".into(),
        number,
        title: format!("PR {number}"),
        draft: false,
        ci_health: Some("passing".into()),
        review_decision: Some("approved".into()),
        has_conflicts: Some(false),
        updated_at: Utc.with_ymd_and_hms(2026, 7, 13, 12, 0, 0).unwrap(),
        labels: Vec::new(),
        dependency_numbers: Vec::new(),
        changed_paths: Vec::new(),
        manual_priority: None,
        pinned: false,
    }
}

#[test]
fn presets_are_deterministic_explainable_and_cover_every_workflow() {
    let now = Utc.with_ymd_and_hms(2026, 7, 14, 12, 0, 0).unwrap();
    let presets = [
        WorkflowPreset::QuickWins,
        WorkflowPreset::CiRescue,
        WorkflowPreset::ReviewClosure,
        WorkflowPreset::ConflictRecovery,
        WorkflowPreset::DependencyChain,
        WorkflowPreset::SecurityFirst,
        WorkflowPreset::ReleaseTrain,
        WorkflowPreset::StalePullRequestTriage,
        WorkflowPreset::DraftCompletion,
        WorkflowPreset::PostMergeWatch,
        WorkflowPreset::ReviewLoadBalancing,
        WorkflowPreset::DuplicateOverlapDetection,
    ];
    for preset in presets {
        let first = assess_queue(&[candidate(2), candidate(1)], preset, now).unwrap();
        let second = assess_queue(&[candidate(1), candidate(2)], preset, now).unwrap();
        assert_eq!(first, second, "{preset:?}");
        assert!(first.iter().all(|decision| !decision.reasons.is_empty()));
    }
}

#[test]
fn security_dependency_overlap_manual_and_unknown_inputs_remain_visible() {
    let now = Utc.with_ymd_and_hms(2026, 7, 14, 12, 0, 0).unwrap();
    let mut security = candidate(1);
    security.labels = vec!["security".into()];
    security.changed_paths = vec!["src/auth.rs".into()];
    let mut dependent = candidate(2);
    dependent.dependency_numbers = vec![1];
    dependent.changed_paths = vec!["src/auth.rs".into()];
    dependent.pinned = true;
    dependent.manual_priority = Some(99);
    let mut unknown = candidate(3);
    unknown.ci_health = None;
    unknown.review_decision = None;
    unknown.has_conflicts = None;
    unknown.updated_at = now - Duration::days(45);

    let queue = assess_queue(
        &[dependent, unknown, security],
        WorkflowPreset::SecurityFirst,
        now,
    )
    .unwrap();
    assert_eq!(queue[0].number, 1);
    assert_eq!(queue[0].tier, QueueTier::Critical);
    let blocked = queue.iter().find(|item| item.number == 2).unwrap();
    assert_eq!(blocked.tier, QueueTier::Blocked);
    assert!(
        blocked
            .reasons
            .iter()
            .any(|reason| reason == "Blocked by #1")
    );
    assert!(
        blocked
            .reasons
            .iter()
            .any(|reason| reason.contains("src/auth.rs"))
    );
    let unknown = queue.iter().find(|item| item.number == 3).unwrap();
    assert_eq!(unknown.tier, QueueTier::Blocked);
    assert!(
        unknown
            .reasons
            .iter()
            .any(|reason| reason.contains("Unknown"))
    );
    assert!(
        unknown
            .reasons
            .iter()
            .any(|reason| reason.contains("Stale"))
    );
}

#[test]
fn invalid_duplicate_identity_and_dependency_fail_closed() {
    let now = Utc.with_ymd_and_hms(2026, 7, 14, 12, 0, 0).unwrap();
    assert!(
        assess_queue(
            &[candidate(1), candidate(1)],
            WorkflowPreset::QuickWins,
            now
        )
        .is_err()
    );
    let mut invalid = candidate(2);
    invalid.dependency_numbers = vec![2];
    assert!(assess_queue(&[invalid], WorkflowPreset::DependencyChain, now).is_err());
}
