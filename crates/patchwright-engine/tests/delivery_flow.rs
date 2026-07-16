use chrono::{TimeZone, Utc};
use patchwright_core::{
    ApprovalClass, Capability, CredentialHealth, GitHubAction, GitHubActionPreview,
    GitHubPullRequestSourceInput, InstructionDigest, MergeMethod, RemoteIdentity,
    RemotePrecondition, RepositoryBinding, RepositoryBindingDraft, RepositoryPermissionSnapshot,
    RiskClass, Task, TaskContract, TaskContractDraft, TaskSource,
};
use patchwright_engine::{
    DeliveryError, EventStore, GitHubRepository, GitHubRepositoryPermissions,
    GitHubRepositorySnapshot, GitHubWorkItem, WorkItemKind, approve_delivery, authorize_execution,
    complete_successful_delivery, preview_delivery, reconcile_completed_task_from_snapshot,
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
        default_branch_committed_at: Some(Utc.with_ymd_and_hms(2026, 7, 13, 10, 0, 0).unwrap()),
        permissions: RepositoryPermissionSnapshot::read_only(),
        credential_health: CredentialHealth::Healthy,
    })
    .unwrap();
    store.save_repository_binding(&binding).unwrap();
    let mut task = Task::new("Deliver comment", "/tmp/hello").unwrap();
    task.repository_binding_id = Some(binding.id());
    store.save_task(&task, "task created").unwrap();
    let contract = TaskContract::try_from(TaskContractDraft {
        task_id: task.id,
        source: TaskSource::LocalRequest,
        repository_binding_id: binding.id(),
        goal: "Deliver one approved GitHub comment".into(),
        acceptance_criteria: vec!["Exact comment is delivered".into()],
        base_sha: Some("a".repeat(40)),
        head_sha: None,
        instruction_digests: vec![InstructionDigest::new("AGENTS.md", "d".repeat(64), 1).unwrap()],
        verification_commands: Vec::new(),
        required_capabilities: vec![Capability::PostComment],
        risk: RiskClass::Moderate,
        sensitive_paths: Vec::new(),
        dependencies: Vec::new(),
    })
    .unwrap();
    store.save_task_contract(&contract).unwrap();
    task
}

#[test]
fn exact_preview_approval_claim_is_single_use_and_stale_safe() {
    let directory = tempfile::tempdir().unwrap();
    let store = EventStore::open(&directory.path().join("engine.sqlite3")).unwrap();
    let task = fixture(&store);
    let action = GitHubActionPreview::new(
        RemoteIdentity::new(42, 84, "octocat/hello").unwrap(),
        GitHubAction::comment(7, "Patchwright verified this change.").unwrap(),
        RemotePrecondition::new(None, Some(&"a".repeat(40)), 3).unwrap(),
    )
    .unwrap();
    let preview = preview_delivery(&store, task.id, action).unwrap();
    let approval = approve_delivery(&store, &preview, "owner").unwrap();
    let key = authorize_execution(&store, &preview, approval.id()).unwrap();
    assert_eq!(key, preview.action.idempotency_sha256());
    assert_eq!(
        authorize_execution(&store, &preview, approval.id()),
        Err(DeliveryError::AlreadyClaimed)
    );
    store
        .complete_delivery(&key, r#"{"state":"failed","error":"definitive rejection"}"#)
        .unwrap();
    assert_eq!(
        authorize_execution(&store, &preview, approval.id()).unwrap(),
        key
    );
    store
        .complete_delivery(&key, r#"{"state":"succeeded","result":{}}"#)
        .unwrap();
    assert_eq!(
        authorize_execution(&store, &preview, approval.id()),
        Err(DeliveryError::AlreadyClaimed)
    );

    let changed = GitHubActionPreview::new(
        RemoteIdentity::new(42, 84, "octocat/hello").unwrap(),
        GitHubAction::comment(7, "Different body").unwrap(),
        RemotePrecondition::new(None, Some(&"a".repeat(40)), 3).unwrap(),
    )
    .unwrap();
    let changed_preview = preview_delivery(&store, task.id, changed).unwrap();
    assert_eq!(
        authorize_execution(&store, &changed_preview, approval.id()),
        Err(DeliveryError::ApprovalInvalid)
    );
}

#[test]
fn remote_identity_and_sha_mismatches_fail_before_approval() {
    let directory = tempfile::tempdir().unwrap();
    let store = EventStore::open(&directory.path().join("engine.sqlite3")).unwrap();
    let task = fixture(&store);
    let wrong_remote = GitHubActionPreview::new(
        RemoteIdentity::new(43, 84, "octocat/other").unwrap(),
        GitHubAction::comment(7, "body").unwrap(),
        RemotePrecondition::new(None, Some(&"a".repeat(40)), 3).unwrap(),
    )
    .unwrap();
    assert_eq!(
        preview_delivery(&store, task.id, wrong_remote),
        Err(DeliveryError::RemoteMismatch)
    );
    let wrong_sha = GitHubActionPreview::new(
        RemoteIdentity::new(42, 84, "octocat/hello").unwrap(),
        GitHubAction::comment(7, "body").unwrap(),
        RemotePrecondition::new(None, Some(&"b".repeat(40)), 3).unwrap(),
    )
    .unwrap();
    assert_eq!(
        preview_delivery(&store, task.id, wrong_sha),
        Err(DeliveryError::PreconditionMismatch)
    );
}

#[test]
fn action_must_be_declared_by_the_typed_task_contract() {
    let directory = tempfile::tempdir().unwrap();
    let store = EventStore::open(&directory.path().join("engine.sqlite3")).unwrap();
    let task = fixture(&store);
    let undeclared = GitHubActionPreview::new(
        RemoteIdentity::new(42, 84, "octocat/hello").unwrap(),
        GitHubAction::check_run("Patchwright", &"a".repeat(40), "completed", Some("success"))
            .unwrap(),
        RemotePrecondition::new(None, Some(&"a".repeat(40)), 3).unwrap(),
    )
    .unwrap();

    assert_eq!(
        preview_delivery(&store, task.id, undeclared),
        Err(DeliveryError::CapabilityNotDeclared)
    );
}

#[test]
fn merge_uses_a_separate_merge_class_and_exact_head_sha() {
    let directory = tempfile::tempdir().unwrap();
    let store = EventStore::open(&directory.path().join("engine.sqlite3")).unwrap();
    let mut task = fixture(&store);
    let original = store.task_contract(task.id).unwrap().unwrap();
    let contract = TaskContract::try_from(TaskContractDraft {
        task_id: task.id,
        source: TaskSource::LocalRequest,
        repository_binding_id: original.repository_binding_id(),
        goal: original.goal().into(),
        acceptance_criteria: original.acceptance_criteria().to_vec(),
        base_sha: Some("a".repeat(40)),
        head_sha: Some("b".repeat(40)),
        instruction_digests: original.instruction_digests().to_vec(),
        verification_commands: Vec::new(),
        required_capabilities: vec![Capability::MergePullRequest],
        risk: RiskClass::High,
        sensitive_paths: Vec::new(),
        dependencies: Vec::new(),
    })
    .unwrap();
    store.save_task_contract(&contract).unwrap();
    task.contract_version = contract.version();
    let action = GitHubActionPreview::new(
        RemoteIdentity::new(42, 84, "octocat/hello").unwrap(),
        GitHubAction::merge_pull_request(7, &"b".repeat(40), MergeMethod::Squash).unwrap(),
        RemotePrecondition::new(Some(&"b".repeat(40)), Some(&"a".repeat(40)), 4).unwrap(),
    )
    .unwrap();
    let preview = preview_delivery(&store, task.id, action).unwrap();
    let approval = approve_delivery(&store, &preview, "owner").unwrap();
    assert_eq!(approval.class(), ApprovalClass::Merge);
    assert_eq!(approval.capability(), Capability::MergePullRequest);

    let changed = GitHubActionPreview::new(
        RemoteIdentity::new(42, 84, "octocat/hello").unwrap(),
        GitHubAction::merge_pull_request(7, &"c".repeat(40), MergeMethod::Squash).unwrap(),
        RemotePrecondition::new(Some(&"c".repeat(40)), Some(&"a".repeat(40)), 5).unwrap(),
    )
    .unwrap();
    assert_eq!(
        preview_delivery(&store, task.id, changed),
        Err(DeliveryError::PreconditionMismatch)
    );
}

#[test]
fn successful_merge_atomically_completes_delivery_and_task_lifecycle() {
    let directory = tempfile::tempdir().unwrap();
    let store = EventStore::open(&directory.path().join("engine.sqlite3")).unwrap();
    let mut task = fixture(&store);
    let original = store.task_contract(task.id).unwrap().unwrap();
    let contract = TaskContract::try_from(TaskContractDraft {
        task_id: task.id,
        source: TaskSource::LocalRequest,
        repository_binding_id: original.repository_binding_id(),
        goal: original.goal().into(),
        acceptance_criteria: original.acceptance_criteria().to_vec(),
        base_sha: Some("a".repeat(40)),
        head_sha: Some("b".repeat(40)),
        instruction_digests: original.instruction_digests().to_vec(),
        verification_commands: Vec::new(),
        required_capabilities: vec![Capability::MergePullRequest],
        risk: RiskClass::High,
        sensitive_paths: Vec::new(),
        dependencies: Vec::new(),
    })
    .unwrap();
    store.save_task_contract(&contract).unwrap();
    for state in [
        patchwright_core::TaskState::Assessing,
        patchwright_core::TaskState::Planned,
        patchwright_core::TaskState::AwaitingPreparationApproval,
        patchwright_core::TaskState::Preparing,
        patchwright_core::TaskState::Implementing,
        patchwright_core::TaskState::Verifying,
        patchwright_core::TaskState::Reviewing,
        patchwright_core::TaskState::AwaitingDeliveryApproval,
    ] {
        task.transition(state).unwrap();
    }
    store.save_task(&task, "ready for delivery").unwrap();
    let action = GitHubActionPreview::new(
        RemoteIdentity::new(42, 84, "octocat/hello").unwrap(),
        GitHubAction::merge_pull_request(7, &"b".repeat(40), MergeMethod::Squash).unwrap(),
        RemotePrecondition::new(Some(&"b".repeat(40)), Some(&"a".repeat(40)), 4).unwrap(),
    )
    .unwrap();
    let preview = preview_delivery(&store, task.id, action).unwrap();
    let approval = approve_delivery(&store, &preview, "owner").unwrap();
    let key = authorize_execution(&store, &preview, approval.id()).unwrap();
    let result = r#"{"state":"succeeded","result":{"merged":true,"sha":"cccccccccccccccccccccccccccccccccccccccc"}}"#;

    complete_successful_delivery(&store, &preview, &key, result, true).unwrap();

    assert_eq!(
        store.load_task(task.id).unwrap().unwrap().state,
        patchwright_core::TaskState::Completed
    );
    assert_eq!(
        store.delivery_result(&key).unwrap().as_deref(),
        Some(result)
    );
    let timeline = store.timeline(task.id).unwrap();
    assert!(timeline.iter().any(|event| event.contains("deliver")));
    assert!(timeline.iter().any(|event| event.contains("completed")));
}

fn completed_pull_snapshot(head_sha: &str, merged: bool) -> GitHubRepositorySnapshot {
    GitHubRepositorySnapshot {
        repository: GitHubRepository {
            id: 42,
            full_name: "octocat/hello".into(),
            description: None,
            private: true,
            archived: false,
            default_branch: "main".into(),
            html_url: "https://github.com/octocat/hello".into(),
            updated_at: "2026-07-14T12:00:00Z".into(),
            pushed_at: Some("2026-07-14T12:00:00Z".into()),
            open_issues_count: 0,
            open_pull_request_count: 0,
            failing_check_count: 0,
            default_branch_sha: Some("c".repeat(40)),
            default_branch_committed_at: Some("2026-07-14T12:00:00Z".into()),
            installation_id: Some(84),
            permissions: GitHubRepositoryPermissions::default(),
        },
        work_items: vec![GitHubWorkItem {
            id: 7,
            repository_full_name: "octocat/hello".into(),
            number: 7,
            kind: WorkItemKind::PullRequest,
            title: "Repair CI".into(),
            state: "closed".into(),
            state_reason: None,
            body: None,
            author: "octocat".into(),
            html_url: "https://github.com/octocat/hello/pull/7".into(),
            draft: false,
            comments_count: 0,
            base_ref: Some("main".into()),
            base_sha: Some("a".repeat(40)),
            head_ref: Some("repair-ci".into()),
            head_sha: Some(head_sha.into()),
            merged: Some(merged),
            merge_commit_sha: merged.then(|| "c".repeat(40)),
            created_at: None,
            head_committed_at: None,
            latest_review_at: None,
            updated_at: "2026-07-14T12:00:00Z".into(),
            review_decision: Some("approved".into()),
            ci_health: Some("passing".into()),
            mergeable: Some(false),
            mergeable_state: Some("unknown".into()),
            rebaseable: Some(false),
            has_conflicts: Some(false),
            head_repository_full_name: Some("octocat/hello".into()),
            head_repository_fork: false,
            maintainer_can_modify: true,
            additions: 1,
            deletions: 0,
            changed_files: 1,
            labels: vec![],
            assignees: vec![],
            milestone: None,
        }],
        discussions: vec![],
        checks: vec![],
        workflow_runs: vec![],
    }
}

#[test]
fn fresh_exact_merged_pull_reconciles_a_pre_fix_task_to_completed() {
    let directory = tempfile::tempdir().unwrap();
    let store = EventStore::open(&directory.path().join("engine.sqlite3")).unwrap();
    let mut task = fixture(&store);
    task.source = TaskSource::github_pull_request(GitHubPullRequestSourceInput {
        repository_id: 42,
        repository_full_name: "octocat/hello".into(),
        number: 7,
        html_url: "https://github.com/octocat/hello/pull/7".into(),
        snapshot_at: Utc.with_ymd_and_hms(2026, 7, 14, 11, 0, 0).unwrap(),
        base_ref: "main".into(),
        base_sha: "a".repeat(40),
        head_ref: "repair-ci".into(),
        head_sha: "b".repeat(40),
    })
    .unwrap();
    for state in [
        patchwright_core::TaskState::Assessing,
        patchwright_core::TaskState::Planned,
        patchwright_core::TaskState::AwaitingPreparationApproval,
        patchwright_core::TaskState::Preparing,
        patchwright_core::TaskState::Implementing,
        patchwright_core::TaskState::Verifying,
        patchwright_core::TaskState::Reviewing,
        patchwright_core::TaskState::AwaitingDeliveryApproval,
    ] {
        task.transition(state).unwrap();
    }
    store
        .save_task(&task, "legacy task awaiting delivery")
        .unwrap();

    let completed = reconcile_completed_task_from_snapshot(
        &store,
        task.id,
        &completed_pull_snapshot(&"b".repeat(40), true),
    )
    .unwrap();
    assert_eq!(completed.state, patchwright_core::TaskState::Completed);
    assert_eq!(
        store.load_task(task.id).unwrap().unwrap().state,
        completed.state
    );
}

#[test]
fn reconciliation_rejects_unmerged_or_changed_head_pull_requests() {
    let directory = tempfile::tempdir().unwrap();
    let store = EventStore::open(&directory.path().join("engine.sqlite3")).unwrap();
    let mut task = fixture(&store);
    task.source = TaskSource::github_pull_request(GitHubPullRequestSourceInput {
        repository_id: 42,
        repository_full_name: "octocat/hello".into(),
        number: 7,
        html_url: "https://github.com/octocat/hello/pull/7".into(),
        snapshot_at: Utc::now(),
        base_ref: "main".into(),
        base_sha: "a".repeat(40),
        head_ref: "repair-ci".into(),
        head_sha: "b".repeat(40),
    })
    .unwrap();
    store
        .save_task(&task, "GitHub task source captured")
        .unwrap();

    assert_eq!(
        reconcile_completed_task_from_snapshot(
            &store,
            task.id,
            &completed_pull_snapshot(&"b".repeat(40), false),
        ),
        Err(DeliveryError::RemoteNotCompleted)
    );
    assert_eq!(
        reconcile_completed_task_from_snapshot(
            &store,
            task.id,
            &completed_pull_snapshot(&"d".repeat(40), true),
        ),
        Err(DeliveryError::PreconditionMismatch)
    );
}
