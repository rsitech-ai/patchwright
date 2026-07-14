use chrono::{Duration, TimeZone, Utc};
use patchwright_core::{
    Capability, CredentialHealth, GitHubAction, GitHubActionPreview, InstructionDigest,
    RemoteIdentity, RemotePrecondition, RepositoryBinding, RepositoryBindingDraft,
    RepositoryPermissionSnapshot, RiskClass, Task, TaskContract, TaskContractDraft, TaskSource,
};
use patchwright_engine::{
    CIState, EventStore, Mergeability, MonitorRecord, MonitorState, RemoteObservation, ReviewState,
    approve_delivery, preview_delivery,
};

fn fixture(store: &EventStore) -> Task {
    let binding = RepositoryBinding::try_from(RepositoryBindingDraft {
        github_repository_id: 42,
        full_name: "octocat/hello".into(),
        installation_id: 84,
        clone_url: "https://github.com/octocat/hello.git".into(),
        html_url: "https://github.com/octocat/hello".into(),
        default_branch: "main".into(),
        user_checkout: Some("/tmp/hello".into()),
        managed_clone: None,
        state_root: "/tmp/patchwright/state".into(),
        worktree_root: "/tmp/patchwright/worktrees".into(),
        default_branch_sha: Some("a".repeat(40)),
        default_branch_committed_at: Some(Utc.with_ymd_and_hms(2026, 7, 14, 8, 0, 0).unwrap()),
        permissions: RepositoryPermissionSnapshot::read_only(),
        credential_health: CredentialHealth::Healthy,
    })
    .unwrap();
    store.save_repository_binding(&binding).unwrap();
    let mut task = Task::new("Monitor delivery", "/tmp/hello").unwrap();
    task.repository_binding_id = Some(binding.id());
    store.save_task(&task, "task created").unwrap();
    store
        .save_task_contract(
            &TaskContract::try_from(TaskContractDraft {
                task_id: task.id,
                source: TaskSource::LocalRequest,
                repository_binding_id: binding.id(),
                goal: "Monitor one delivered pull request".into(),
                acceptance_criteria: vec!["CI and review converge".into()],
                base_sha: Some("a".repeat(40)),
                head_sha: Some("b".repeat(40)),
                instruction_digests: vec![
                    InstructionDigest::new("AGENTS.md", "d".repeat(64), 1).unwrap(),
                ],
                verification_commands: Vec::new(),
                required_capabilities: vec![Capability::PostComment],
                risk: RiskClass::Moderate,
                sensitive_paths: Vec::new(),
                dependencies: Vec::new(),
            })
            .unwrap(),
        )
        .unwrap();
    task
}

fn observation(at: chrono::DateTime<Utc>) -> RemoteObservation {
    RemoteObservation {
        observed_at: at,
        head_sha: "b".repeat(40),
        base_sha: "a".repeat(40),
        ci: CIState::Pending,
        review: ReviewState::Pending,
        mergeability: Mergeability::Mergeable,
        repository_accessible: true,
        network_available: true,
        rate_limited_until: None,
    }
}

#[test]
fn pending_success_and_transient_failures_use_durable_bounded_backoff() {
    let now = Utc.with_ymd_and_hms(2026, 7, 14, 9, 0, 0).unwrap();
    let mut monitor = MonitorRecord::new(
        patchwright_core::TaskId::new(),
        "octocat/hello",
        7,
        "b".repeat(40),
        "a".repeat(40),
        now,
        2,
    )
    .unwrap();

    let pending = monitor.observe(observation(now), now).unwrap();
    assert_eq!(pending.state, MonitorState::Pending);
    let first_attempt = monitor.next_attempt_at.unwrap();
    assert!(first_attempt >= now + Duration::seconds(30));
    assert!(first_attempt <= now + Duration::seconds(40));

    let mut offline = observation(first_attempt);
    offline.network_available = false;
    let transient = monitor.observe(offline, first_attempt).unwrap();
    assert_eq!(transient.state, MonitorState::Pending);
    assert_eq!(monitor.repair_iteration, 0);
    let second_attempt = monitor.next_attempt_at.unwrap();
    assert!(second_attempt >= first_attempt + Duration::seconds(60));
    assert!(second_attempt <= first_attempt + Duration::seconds(70));

    let mut success = observation(second_attempt);
    success.ci = CIState::Success;
    success.review = ReviewState::Approved;
    let complete = monitor.observe(success, second_attempt).unwrap();
    assert_eq!(complete.state, MonitorState::Succeeded);
    assert_eq!(monitor.next_attempt_at, None);
}

#[test]
fn actionable_failures_are_bounded_and_remote_identity_changes_block() {
    let now = Utc.with_ymd_and_hms(2026, 7, 14, 9, 0, 0).unwrap();
    let task_id = patchwright_core::TaskId::new();
    let mut monitor = MonitorRecord::new(
        task_id,
        "octocat/hello",
        7,
        "b".repeat(40),
        "a".repeat(40),
        now,
        1,
    )
    .unwrap();
    let mut failed = observation(now);
    failed.ci = CIState::Failure;
    let repair = monitor.observe(failed.clone(), now).unwrap();
    assert_eq!(repair.state, MonitorState::RepairNeeded);
    assert!(repair.invalidate_approvals);
    assert_eq!(monitor.repair_iteration, 1);

    let exhausted = monitor
        .observe(failed, now + Duration::seconds(30))
        .unwrap();
    assert_eq!(exhausted.state, MonitorState::Blocked);
    assert!(exhausted.summary.contains("repair budget exhausted"));

    let mut changed = MonitorRecord::new(
        task_id,
        "octocat/hello",
        7,
        "b".repeat(40),
        "a".repeat(40),
        now,
        2,
    )
    .unwrap();
    let mut new_head = observation(now);
    new_head.head_sha = "c".repeat(40);
    let blocked = changed.observe(new_head, now).unwrap();
    assert_eq!(blocked.state, MonitorState::Blocked);
    assert!(blocked.summary.contains("head SHA changed"));
    assert!(blocked.invalidate_approvals);
}

#[test]
fn monitor_and_approval_invalidation_survive_restart() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("engine.sqlite3");
    let store = EventStore::open(&database).unwrap();
    let task = fixture(&store);
    let action = GitHubActionPreview::new(
        RemoteIdentity::new(42, 84, "octocat/hello").unwrap(),
        GitHubAction::comment(7, "approved body").unwrap(),
        RemotePrecondition::new(Some(&"b".repeat(40)), Some(&"a".repeat(40)), 1).unwrap(),
    )
    .unwrap();
    let preview = preview_delivery(&store, task.id, action).unwrap();
    let approval = approve_delivery(&store, &preview, "owner").unwrap();
    let now = Utc.with_ymd_and_hms(2026, 7, 14, 9, 0, 0).unwrap();
    let monitor = MonitorRecord::new(
        task.id,
        "octocat/hello",
        7,
        "b".repeat(40),
        "a".repeat(40),
        now,
        2,
    )
    .unwrap();
    store.save_monitor(&monitor).unwrap();
    assert_eq!(store.invalidate_task_approvals(task.id).unwrap(), 1);
    assert!(store.approval(approval.id()).unwrap().is_none());
    drop(store);

    let reopened = EventStore::open(&database).unwrap();
    assert_eq!(reopened.monitor(monitor.id).unwrap(), Some(monitor));
    assert!(reopened.approval(approval.id()).unwrap().is_none());
}
