use patchwright_core::{
    Capability, CredentialHealth, RepositoryBinding, RepositoryBindingDraft,
    RepositoryPermissionSnapshot, TaskSource,
};
use patchwright_engine::{
    ConversionError, ConversionRequest, EventStore, GitHubRepository, GitHubRepositoryPermissions,
    GitHubRepositorySnapshot, GitHubWorkItem, TaskConversionService, WorkItemKind,
};

const BASE_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HEAD_SHA: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn repository() -> GitHubRepository {
    GitHubRepository {
        id: 42,
        full_name: "acme/widget".into(),
        description: None,
        private: true,
        archived: false,
        default_branch: "main".into(),
        html_url: "https://github.com/acme/widget".into(),
        updated_at: "2026-07-13T12:00:00Z".into(),
        pushed_at: Some("2026-07-13T11:00:00Z".into()),
        open_issues_count: 2,
        open_pull_request_count: 1,
        failing_check_count: 0,
        default_branch_sha: Some(BASE_SHA.into()),
        default_branch_committed_at: Some("2026-07-13T10:00:00Z".into()),
        installation_id: Some(99),
        permissions: GitHubRepositoryPermissions::default(),
    }
}

fn item(number: u64, kind: WorkItemKind) -> GitHubWorkItem {
    let pull_request = kind == WorkItemKind::PullRequest;
    GitHubWorkItem {
        id: 100 + number,
        repository_full_name: "acme/widget".into(),
        number,
        kind,
        title: if pull_request {
            "Repair CI"
        } else {
            "Fix login"
        }
        .into(),
        state: "open".into(),
        body: Some("Untrusted GitHub body".into()),
        author: "octocat".into(),
        html_url: format!(
            "https://github.com/acme/widget/{}/{}",
            if pull_request { "pull" } else { "issues" },
            number
        ),
        draft: false,
        comments_count: 0,
        base_ref: pull_request.then(|| "main".into()),
        base_sha: pull_request.then(|| BASE_SHA.into()),
        head_ref: pull_request.then(|| "repair-ci".into()),
        head_sha: pull_request.then(|| HEAD_SHA.into()),
        created_at: Some("2026-07-13T08:00:00Z".into()),
        head_committed_at: pull_request.then(|| "2026-07-13T11:00:00Z".into()),
        latest_review_at: None,
        updated_at: "2026-07-13T12:00:00Z".into(),
        review_decision: None,
        ci_health: Some("passing".into()),
        mergeable: Some(true),
        mergeable_state: Some("clean".into()),
        rebaseable: Some(true),
        has_conflicts: Some(false),
        head_repository_full_name: Some("acme/widget".into()),
        head_repository_fork: false,
        maintainer_can_modify: true,
        additions: 12,
        deletions: 3,
        changed_files: 2,
        labels: vec!["bug".into()],
        assignees: vec![],
        milestone: None,
    }
}

fn snapshot(items: Vec<GitHubWorkItem>) -> GitHubRepositorySnapshot {
    GitHubRepositorySnapshot {
        repository: repository(),
        work_items: items,
        discussions: vec![],
        checks: vec![],
        workflow_runs: vec![],
    }
}

fn binding() -> RepositoryBinding {
    RepositoryBinding::try_from(RepositoryBindingDraft {
        github_repository_id: 42,
        full_name: "acme/widget".into(),
        installation_id: 99,
        clone_url: "https://github.com/acme/widget.git".into(),
        html_url: "https://github.com/acme/widget".into(),
        default_branch: "main".into(),
        user_checkout: Some("/tmp/acme-widget".into()),
        managed_clone: Some("/tmp/patchwright/repos/acme-widget".into()),
        state_root: "/tmp/patchwright/state/acme-widget".into(),
        worktree_root: "/tmp/patchwright/worktrees/acme-widget".into(),
        default_branch_sha: Some(BASE_SHA.into()),
        default_branch_committed_at: Some("2026-07-13T10:00:00Z".parse().unwrap()),
        permissions: RepositoryPermissionSnapshot::read_only(),
        credential_health: CredentialHealth::Healthy,
    })
    .unwrap()
}

fn request(number: u64) -> ConversionRequest {
    ConversionRequest {
        repository_full_name: "acme/widget".into(),
        item_number: number,
        expected_updated_at: "2026-07-13T12:00:00Z".into(),
    }
}

#[test]
fn issue_and_pull_request_convert_to_typed_tasks_with_exact_source_identity() {
    let directory = tempfile::tempdir().unwrap();
    let store = EventStore::open(&directory.path().join("events.sqlite3")).unwrap();
    store
        .replace_github_snapshot(&snapshot(vec![
            item(7, WorkItemKind::Issue),
            item(8, WorkItemKind::PullRequest),
        ]))
        .unwrap();
    store.save_repository_binding(&binding()).unwrap();
    let service = TaskConversionService::new(&store);

    let issue = service.create(request(7)).unwrap();
    assert!(issue.created);
    assert_eq!(issue.preview.item_number, 7);
    assert!(issue.preview.requires_confirmation);
    assert!(matches!(issue.task.source, TaskSource::GitHubIssue(_)));
    let issue_contract = store.task_contract(issue.task.id).unwrap().unwrap();
    assert_eq!(issue_contract.source().item_number(), Some(7));
    assert_eq!(issue_contract.base_sha(), Some(BASE_SHA));
    assert_eq!(
        issue_contract.required_capabilities(),
        &[
            Capability::CreateBranch,
            Capability::PushBranch,
            Capability::CreatePullRequest,
            Capability::PostComment,
            Capability::CreateCheckRun,
            Capability::CloseIssue,
        ]
    );

    let pull_request = service.create(request(8)).unwrap();
    assert!(matches!(
        pull_request.task.source,
        TaskSource::GitHubPullRequest(_)
    ));
    assert_eq!(pull_request.task.source.base_sha(), Some(BASE_SHA));
    assert_eq!(pull_request.task.source.head_sha(), Some(HEAD_SHA));
    let contract = store.task_contract(pull_request.task.id).unwrap().unwrap();
    assert_eq!(contract.base_sha(), Some(BASE_SHA));
    assert_eq!(contract.head_sha(), Some(HEAD_SHA));
    assert_eq!(
        contract.required_capabilities(),
        &[
            Capability::PushBranch,
            Capability::PostComment,
            Capability::PostReview,
            Capability::CreateCheckRun,
            Capability::UpdatePullRequestBranch,
            Capability::ReadyPullRequest,
            Capability::ClosePullRequest,
            Capability::EnqueuePullRequest,
            Capability::MergePullRequest,
        ]
    );
}

#[test]
fn duplicate_conversion_is_idempotent() {
    let directory = tempfile::tempdir().unwrap();
    let store = EventStore::open(&directory.path().join("events.sqlite3")).unwrap();
    store
        .replace_github_snapshot(&snapshot(vec![item(7, WorkItemKind::Issue)]))
        .unwrap();
    store.save_repository_binding(&binding()).unwrap();
    let service = TaskConversionService::new(&store);

    let first = service.create(request(7)).unwrap();
    let second = service.create(request(7)).unwrap();
    assert!(first.created);
    assert!(!second.created);
    assert_eq!(first.task.id, second.task.id);
    assert_eq!(store.timeline(first.task.id).unwrap().len(), 1);
}

#[test]
fn conversion_rejects_missing_stale_or_unbound_snapshots() {
    let directory = tempfile::tempdir().unwrap();
    let store = EventStore::open(&directory.path().join("events.sqlite3")).unwrap();
    let service = TaskConversionService::new(&store);
    assert_eq!(
        service.preview(request(7)).unwrap_err(),
        ConversionError::SnapshotMissing
    );

    store
        .replace_github_snapshot(&snapshot(vec![item(7, WorkItemKind::Issue)]))
        .unwrap();
    assert_eq!(
        service.preview(request(7)).unwrap_err(),
        ConversionError::RepositoryBindingMissing
    );
    store.save_repository_binding(&binding()).unwrap();
    let mut stale = request(7);
    stale.expected_updated_at = "2026-07-12T12:00:00Z".into();
    assert_eq!(
        service.preview(stale).unwrap_err(),
        ConversionError::SnapshotStale
    );
    assert_eq!(
        service.preview(request(999)).unwrap_err(),
        ConversionError::ItemMissing
    );
}

#[test]
fn inaccessible_fork_and_incomplete_pull_request_fail_closed() {
    let directory = tempfile::tempdir().unwrap();
    let store = EventStore::open(&directory.path().join("events.sqlite3")).unwrap();
    store.save_repository_binding(&binding()).unwrap();
    let mut fork = item(8, WorkItemKind::PullRequest);
    fork.head_repository_full_name = Some("contributor/widget".into());
    fork.head_repository_fork = true;
    fork.maintainer_can_modify = false;
    store
        .replace_github_snapshot(&snapshot(vec![fork]))
        .unwrap();
    assert_eq!(
        TaskConversionService::new(&store)
            .preview(request(8))
            .unwrap_err(),
        ConversionError::ForkInaccessible
    );

    let mut incomplete = item(8, WorkItemKind::PullRequest);
    incomplete.head_sha = None;
    store
        .replace_github_snapshot(&snapshot(vec![incomplete]))
        .unwrap();
    assert_eq!(
        TaskConversionService::new(&store)
            .preview(request(8))
            .unwrap_err(),
        ConversionError::IncompletePullRequest
    );
}
