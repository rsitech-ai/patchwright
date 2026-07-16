use chrono::{TimeZone, Utc};
use patchwright_core::{
    CredentialHealth, GitHubIssueSourceInput, GitHubPullRequestSourceInput, InstructionDigest,
    RepositoryBinding, RepositoryBindingDraft, RepositoryPermissionSnapshot, RiskClass,
    SensitivePath, Task, TaskContract, TaskContractDraft, TaskId, TaskSource, VerificationCommand,
};
use uuid::Uuid;

fn snapshot_time() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 13, 10, 0, 0)
        .single()
        .unwrap()
}

fn repository_binding() -> RepositoryBinding {
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
        default_branch_committed_at: Some(snapshot_time()),
        permissions: RepositoryPermissionSnapshot::read_only(),
        credential_health: CredentialHealth::Healthy,
    })
    .unwrap()
}

#[test]
fn task_sources_retain_immutable_issue_and_pull_request_identity() {
    let issue = TaskSource::github_issue(GitHubIssueSourceInput {
        repository_id: 42,
        repository_full_name: "octocat/hello".into(),
        number: 17,
        html_url: "https://github.com/octocat/hello/issues/17".into(),
        snapshot_at: snapshot_time(),
    })
    .unwrap();
    assert_eq!(serde_json::to_value(&issue).unwrap()["kind"], "githubIssue");
    assert_eq!(issue.repository_id(), Some(42));
    assert_eq!(issue.item_number(), Some(17));
    assert_eq!(
        issue.html_url(),
        Some("https://github.com/octocat/hello/issues/17")
    );

    let pull_request = TaskSource::github_pull_request(GitHubPullRequestSourceInput {
        repository_id: 42,
        repository_full_name: "octocat/hello".into(),
        number: 18,
        html_url: "https://github.com/octocat/hello/pull/18".into(),
        snapshot_at: snapshot_time(),
        base_ref: "main".into(),
        base_sha: "b".repeat(40),
        head_ref: "feature".into(),
        head_sha: "c".repeat(40),
    })
    .unwrap();
    assert_eq!(pull_request.base_ref(), Some("main"));
    assert_eq!(pull_request.base_sha(), Some("b".repeat(40).as_str()));
    assert_eq!(pull_request.head_ref(), Some("feature"));
    assert_eq!(pull_request.head_sha(), Some("c".repeat(40).as_str()));
    assert_eq!(pull_request.snapshot_at(), Some(snapshot_time()));
}

#[test]
fn task_source_boundaries_reject_unusable_github_identity() {
    let base = GitHubIssueSourceInput {
        repository_id: 42,
        repository_full_name: "octocat/hello".into(),
        number: 17,
        html_url: "https://github.com/octocat/hello/issues/17".into(),
        snapshot_at: snapshot_time(),
    };
    assert!(
        TaskSource::github_issue(GitHubIssueSourceInput {
            repository_id: 0,
            ..base.clone()
        })
        .is_err()
    );
    assert!(
        TaskSource::github_issue(GitHubIssueSourceInput {
            repository_full_name: String::new(),
            ..base.clone()
        })
        .is_err()
    );
    assert!(
        TaskSource::github_issue(GitHubIssueSourceInput {
            html_url: "http://github.com/octocat/hello/issues/17".into(),
            ..base
        })
        .is_err()
    );

    let pull_request = GitHubPullRequestSourceInput {
        repository_id: 42,
        repository_full_name: "octocat/hello".into(),
        number: 18,
        html_url: "https://github.com/octocat/hello/pull/18".into(),
        snapshot_at: snapshot_time(),
        base_ref: "main".into(),
        base_sha: String::new(),
        head_ref: "feature".into(),
        head_sha: "c".repeat(40),
    };
    assert!(TaskSource::github_pull_request(pull_request).is_err());
}

#[test]
fn repository_binding_rejects_ids_urls_and_relative_roots() {
    let valid = RepositoryBindingDraft {
        github_repository_id: 42,
        full_name: "octocat/hello".into(),
        installation_id: 84,
        clone_url: "https://github.com/octocat/hello.git".into(),
        html_url: "https://github.com/octocat/hello".into(),
        default_branch: "main".into(),
        user_checkout: None,
        managed_clone: Some("/tmp/managed/hello".into()),
        state_root: "/tmp/patchwright/state".into(),
        worktree_root: "/tmp/patchwright/worktrees".into(),
        default_branch_sha: None,
        default_branch_committed_at: None,
        permissions: RepositoryPermissionSnapshot::read_only(),
        credential_health: CredentialHealth::Unknown,
    };
    assert!(
        RepositoryBinding::try_from(RepositoryBindingDraft {
            installation_id: 0,
            ..valid.clone()
        })
        .is_err()
    );
    assert!(
        RepositoryBinding::try_from(RepositoryBindingDraft {
            clone_url: "ssh://git@github.com/octocat/hello.git".into(),
            ..valid.clone()
        })
        .is_err()
    );
    assert!(
        RepositoryBinding::try_from(RepositoryBindingDraft {
            worktree_root: "relative/worktrees".into(),
            ..valid
        })
        .is_err()
    );
}

#[test]
fn task_contract_rejects_empty_acceptance_and_duplicate_dependencies() {
    let binding = repository_binding();
    let task = Task::new("Fix issue", "/tmp/hello").unwrap();
    let dependency = TaskId::new();
    let draft = TaskContractDraft {
        task_id: task.id,
        source: TaskSource::LocalRequest,
        repository_binding_id: binding.id(),
        goal: "Fix the observed failure".into(),
        acceptance_criteria: vec!["Focused test passes".into()],
        base_sha: Some("a".repeat(40)),
        head_sha: None,
        instruction_digests: vec![InstructionDigest::new("AGENTS.md", "d".repeat(64), 10).unwrap()],
        verification_commands: vec![VerificationCommand::new("cargo", ["test"]).unwrap()],
        required_capabilities: Vec::new(),
        risk: RiskClass::Moderate,
        sensitive_paths: vec![SensitivePath::new("Cargo.lock", "dependency surface").unwrap()],
        dependencies: vec![dependency],
    };
    let contract = TaskContract::try_from(draft.clone()).unwrap();
    assert_eq!(contract.version(), 1);
    assert_eq!(contract.repository_binding_id(), binding.id());

    assert!(
        TaskContract::try_from(TaskContractDraft {
            acceptance_criteria: vec!["  ".into()],
            ..draft.clone()
        })
        .is_err()
    );
    assert!(
        TaskContract::try_from(TaskContractDraft {
            dependencies: vec![dependency, dependency],
            ..draft
        })
        .is_err()
    );
}

#[test]
fn validated_contract_components_reject_unsafe_boundaries() {
    assert!(InstructionDigest::new("AGENTS.md", "not-a-sha", 1).is_err());
    assert!(VerificationCommand::new(" ", ["test"]).is_err());
    assert!(VerificationCommand::new("cargo", ["test\0secret"]).is_err());
    assert!(SensitivePath::new("/etc/passwd", "outside repository").is_err());
    assert!(SensitivePath::new("Sources/../Secrets", "path traversal").is_err());
}

#[test]
fn github_identity_rejects_control_characters() {
    assert!(
        TaskSource::github_issue(GitHubIssueSourceInput {
            repository_id: 42,
            repository_full_name: "octo\ncat/hello".into(),
            number: 17,
            html_url: "https://github.com/octocat/hello/issues/17".into(),
            snapshot_at: snapshot_time(),
        })
        .is_err()
    );
    assert!(
        TaskSource::github_pull_request(GitHubPullRequestSourceInput {
            repository_id: 42,
            repository_full_name: "octocat/hello".into(),
            number: 18,
            html_url: "https://github.com/octocat/hello/pull/18".into(),
            snapshot_at: snapshot_time(),
            base_ref: "main\0hidden".into(),
            base_sha: "b".repeat(40),
            head_ref: "feature".into(),
            head_sha: "c".repeat(40),
        })
        .is_err()
    );
}

#[test]
fn legacy_task_payload_defaults_to_a_local_unbound_contract_summary() {
    let id = Uuid::new_v4();
    let payload = format!(
        r#"{{"id":"{id}","title":"Legacy","repositoryPath":"/tmp/repo","state":"discovered","createdAt":"2026-07-13T10:00:00Z","updatedAt":"2026-07-13T10:00:00Z"}}"#
    );
    let task: Task = serde_json::from_str(&payload).unwrap();
    assert_eq!(task.source, TaskSource::LocalRequest);
    assert!(task.repository_binding_id.is_none());
    assert_eq!(task.contract_version, 1);
    assert!(task.checkpoint_id.is_none());
}
