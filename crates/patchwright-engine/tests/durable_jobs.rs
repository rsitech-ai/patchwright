use chrono::{Duration, TimeZone, Utc};
use patchwright_core::{
    ActionFingerprint, ActionFingerprintDraft, Approval, ApprovalClass, Capability,
    CredentialHealth, InstructionDigest, RepositoryBinding, RepositoryBindingDraft,
    RepositoryPermissionSnapshot, RiskClass, SensitivePath, Task, TaskContract, TaskContractDraft,
    TaskSource, TaskState, VerificationCommand,
};
use patchwright_engine::{
    CancellationState, EventStore, Job, JobCheckpoint, JobKind, JobState, TaskCheckpoint,
};
use rusqlite::{Connection, params};
use tempfile::tempdir;

fn now() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 13, 10, 0, 0)
        .single()
        .unwrap()
}

fn binding() -> RepositoryBinding {
    RepositoryBinding::try_from(RepositoryBindingDraft {
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
        default_branch_committed_at: Some(now()),
        permissions: RepositoryPermissionSnapshot::read_only(),
        credential_health: CredentialHealth::Healthy,
    })
    .unwrap()
}

#[test]
fn legacy_database_migrates_without_losing_existing_tasks() {
    let directory = tempdir().unwrap();
    let database = directory.path().join("events.sqlite");
    let task_id = uuid::Uuid::new_v4();
    let payload = format!(
        r#"{{"id":"{task_id}","title":"Legacy","repositoryPath":"/tmp/repo","state":"awaitingApproval","createdAt":"2026-07-13T10:00:00Z","updatedAt":"2026-07-13T10:00:00Z"}}"#
    );
    let connection = Connection::open(&database).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE tasks (id TEXT PRIMARY KEY, payload TEXT NOT NULL, updated_at TEXT NOT NULL);
             CREATE TABLE task_events (sequence INTEGER PRIMARY KEY AUTOINCREMENT, task_id TEXT NOT NULL, summary TEXT NOT NULL, payload TEXT NOT NULL, occurred_at TEXT NOT NULL);
             CREATE TABLE github_snapshots (full_name TEXT PRIMARY KEY, repository_payload TEXT NOT NULL, snapshot_payload TEXT NOT NULL, synced_at TEXT NOT NULL);",
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO github_snapshots(full_name, repository_payload, snapshot_payload, synced_at)
             VALUES ('octocat/hello', 'repository-before-migration', 'snapshot-before-migration', ?1)",
            [now().to_rfc3339()],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO tasks(id, payload, updated_at) VALUES (?1, ?2, ?3)",
            params![task_id.to_string(), payload, now().to_rfc3339()],
        )
        .unwrap();
    drop(connection);

    let store = EventStore::open(&database).unwrap();
    let loaded = store
        .load_task(task_id.to_string().parse().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(loaded.state, TaskState::AwaitingPreparationApproval);
    assert_eq!(store.schema_versions().unwrap(), vec![1, 2, 3, 4]);
    drop(store);

    let connection = Connection::open(database).unwrap();
    let retained_snapshot: (String, String) = connection
        .query_row(
            "SELECT repository_payload, snapshot_payload FROM github_snapshots WHERE full_name='octocat/hello'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        retained_snapshot,
        (
            "repository-before-migration".into(),
            "snapshot-before-migration".into()
        )
    );
    for table in [
        "repository_bindings",
        "task_contracts",
        "approvals",
        "jobs",
        "job_events",
        "task_checkpoints",
        "codex_sessions",
        "codex_events",
    ] {
        let present: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert!(present, "missing migrated table {table}");
    }
}

#[test]
fn task_checkpoint_failure_rolls_back_task_event_and_checkpoint_together() {
    let directory = tempdir().unwrap();
    let store = EventStore::open(&directory.path().join("events.sqlite")).unwrap();
    let mut task = Task::new("Durable task", "/tmp/repo").unwrap();
    task.transition(TaskState::Assessing).unwrap();
    let checkpoint = TaskCheckpoint::new(task.id, task.state, "assessment started").unwrap();
    store
        .save_task_with_checkpoint(&task, "assessment started", &checkpoint)
        .unwrap();

    task.transition(TaskState::Planned).unwrap();
    assert!(
        store
            .save_task_with_checkpoint(&task, "plan completed", &checkpoint)
            .is_err()
    );

    let loaded = store.load_task(task.id).unwrap().unwrap();
    assert_eq!(loaded.state, TaskState::Assessing);
    assert_eq!(store.timeline(task.id).unwrap().len(), 1);
    assert_eq!(store.task_checkpoints(task.id).unwrap(), vec![checkpoint]);
}

#[test]
fn bindings_contracts_and_approval_action_digests_survive_restart() {
    let directory = tempdir().unwrap();
    let database = directory.path().join("events.sqlite");
    let store = EventStore::open(&database).unwrap();
    let binding = binding();
    store.save_repository_binding(&binding).unwrap();
    let task = Task::new("Durable task", "/tmp/repo").unwrap();
    let contract = TaskContract::try_from(TaskContractDraft {
        task_id: task.id,
        source: TaskSource::LocalRequest,
        repository_binding_id: binding.id(),
        goal: "Make persistence durable".into(),
        acceptance_criteria: vec!["Restart passes".into()],
        base_sha: Some("a".repeat(40)),
        head_sha: None,
        instruction_digests: vec![InstructionDigest::new("AGENTS.md", "d".repeat(64), 1).unwrap()],
        verification_commands: vec![VerificationCommand::new("cargo", ["test"]).unwrap()],
        required_capabilities: vec![Capability::PushBranch],
        risk: RiskClass::Moderate,
        sensitive_paths: vec![SensitivePath::new("Cargo.lock", "dependencies").unwrap()],
        dependencies: Vec::new(),
    })
    .unwrap();
    store.save_task_contract(&contract).unwrap();
    let fingerprint = ActionFingerprint::try_from(ActionFingerprintDraft {
        task_id: task.id,
        github_repository_id: 42,
        repository_full_name: "octocat/hello".into(),
        action_kind: "pushBranch".into(),
        pull_request_number: None,
        branch: Some("patchwright/task".into()),
        head_sha: Some("a".repeat(40)),
        base_sha: Some("b".repeat(40)),
        payload_sha256: "c".repeat(64),
        policy_sha256: "d".repeat(64),
        instruction_sha256: "e".repeat(64),
        invalidation_generation: 1,
    })
    .unwrap();
    let approval = Approval::new(
        ApprovalClass::GitHubDelivery,
        Capability::PushBranch,
        fingerprint.clone(),
        "owner",
        now(),
        now() + Duration::minutes(5),
    )
    .unwrap();
    store.save_approval(&approval).unwrap();
    drop(store);

    let store = EventStore::open(&database).unwrap();
    assert_eq!(
        store.repository_binding(binding.id()).unwrap(),
        Some(binding)
    );
    assert_eq!(store.task_contract(task.id).unwrap(), Some(contract));
    let persisted = store.approval(approval.id()).unwrap().unwrap();
    assert_eq!(persisted, approval);
    assert_eq!(
        store.approval_action_digest(approval.id()).unwrap(),
        Some(fingerprint.digest_sha256())
    );
}

#[test]
fn job_compare_and_set_transitions_append_checkpoints_and_events() {
    let directory = tempdir().unwrap();
    let store = EventStore::open(&directory.path().join("events.sqlite")).unwrap();
    let job = Job::new(JobKind::GitHubSync, None, "sync queued").unwrap();
    store.create_job(&job).unwrap();
    let checkpoint = JobCheckpoint::new(1, "repositories", "10 discovered").unwrap();
    assert!(
        store
            .transition_job(
                job.id(),
                JobState::Queued,
                JobState::Running,
                CancellationState::NotRequested,
                "sync running",
                Some(&checkpoint),
            )
            .unwrap()
    );
    assert!(
        !store
            .transition_job(
                job.id(),
                JobState::Queued,
                JobState::Failed,
                CancellationState::NotRequested,
                "stale writer",
                None,
            )
            .unwrap()
    );
    let loaded = store.job(job.id()).unwrap().unwrap();
    assert_eq!(loaded.state(), JobState::Running);
    assert_eq!(loaded.checkpoint(), Some(&checkpoint));
    assert_eq!(store.job_timeline(job.id()).unwrap().len(), 2);
}

#[test]
fn restart_marks_running_and_cancelling_jobs_interrupted_but_retains_terminal_states() {
    let directory = tempdir().unwrap();
    let database = directory.path().join("events.sqlite");
    let store = EventStore::open(&database).unwrap();
    let mut jobs = Vec::new();
    let queued = Job::new(JobKind::GitHubSync, None, "queued").unwrap();
    store.create_job(&queued).unwrap();
    jobs.push((queued.id(), JobState::Queued));
    for terminal in [JobState::Cancelled, JobState::Succeeded, JobState::Failed] {
        let job = Job::new(JobKind::TaskExecution, None, "queued").unwrap();
        store.create_job(&job).unwrap();
        let next = if terminal == JobState::Cancelled {
            JobState::Cancelled
        } else {
            JobState::Running
        };
        store
            .transition_job(
                job.id(),
                JobState::Queued,
                next,
                if terminal == JobState::Cancelled {
                    CancellationState::Acknowledged
                } else {
                    CancellationState::NotRequested
                },
                "first transition",
                None,
            )
            .unwrap();
        if next != terminal {
            store
                .transition_job(
                    job.id(),
                    next,
                    terminal,
                    CancellationState::NotRequested,
                    "terminal transition",
                    None,
                )
                .unwrap();
        }
        jobs.push((job.id(), terminal));
    }
    for state in [JobState::Running, JobState::Cancelling] {
        let job = Job::new(JobKind::GitHubSync, None, "queued").unwrap();
        store.create_job(&job).unwrap();
        store
            .transition_job(
                job.id(),
                JobState::Queued,
                JobState::Running,
                CancellationState::NotRequested,
                "running",
                None,
            )
            .unwrap();
        if state == JobState::Cancelling {
            store
                .transition_job(
                    job.id(),
                    JobState::Running,
                    JobState::Cancelling,
                    CancellationState::Requested,
                    "cancelling",
                    None,
                )
                .unwrap();
        }
        jobs.push((job.id(), JobState::Interrupted));
    }
    drop(store);

    let store = EventStore::open(&database).unwrap();
    for (id, expected) in jobs {
        assert_eq!(store.job(id).unwrap().unwrap().state(), expected);
    }
}

#[test]
fn job_summary_rejects_multiline_output_and_credential_material() {
    assert!(Job::new(JobKind::GitHubSync, None, "line one\nline two").is_err());
    assert!(Job::new(JobKind::GitHubSync, None, "Bearer secret").is_err());
    assert!(Job::new(JobKind::GitHubSync, None, "gho_secret").is_err());
    assert!(Job::new(JobKind::GitHubSync, None, "x".repeat(257)).is_err());
}
