use patchwright_engine::{
    EventStore, GitHubAccount, GitHubRepository, GitHubRepositoryPermissions,
    GitHubRepositorySnapshot, GitHubWorkItem, WorkItemKind,
};
use std::os::unix::fs::PermissionsExt;

fn snapshot(title: &str) -> GitHubRepositorySnapshot {
    GitHubRepositorySnapshot {
        repository: GitHubRepository {
            id: 1,
            full_name: "octocat/hello".into(),
            description: None,
            private: false,
            archived: false,
            default_branch: "main".into(),
            html_url: "https://github.com/octocat/hello".into(),
            updated_at: "2026-07-13T10:00:00Z".into(),
            pushed_at: None,
            open_issues_count: 1,
            open_pull_request_count: 0,
            failing_check_count: 0,
            default_branch_sha: None,
            default_branch_committed_at: None,
            installation_id: None,
            permissions: GitHubRepositoryPermissions::default(),
        },
        work_items: vec![GitHubWorkItem {
            id: 10,
            repository_full_name: "octocat/hello".into(),
            number: 1,
            kind: WorkItemKind::Issue,
            title: title.into(),
            state: "open".into(),
            state_reason: None,
            body: None,
            author: "octocat".into(),
            html_url: "https://github.com/octocat/hello/issues/1".into(),
            draft: false,
            comments_count: 0,
            base_ref: None,
            base_sha: None,
            head_ref: None,
            head_sha: None,
            merged: None,
            merge_commit_sha: None,
            created_at: None,
            head_committed_at: None,
            latest_review_at: None,
            updated_at: "2026-07-13T10:00:00Z".into(),
            review_decision: None,
            ci_health: None,
            mergeable: None,
            mergeable_state: None,
            rebaseable: None,
            has_conflicts: None,
            head_repository_full_name: None,
            head_repository_fork: false,
            maintainer_can_modify: false,
            additions: 0,
            deletions: 0,
            changed_files: 0,
            labels: vec!["bug".into()],
            assignees: vec!["octocat".into()],
            milestone: Some("v1".into()),
        }],
        discussions: vec![],
        checks: vec![],
        workflow_runs: vec![],
    }
}

#[test]
fn github_snapshot_replaces_atomically_and_survives_restart() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("state.sqlite3");
    {
        let store = EventStore::open(&database).unwrap();
        store
            .save_github_account(&GitHubAccount {
                login: "octocat".into(),
                avatar_url: "https://example/avatar".into(),
                html_url: "https://github.com/octocat".into(),
            })
            .unwrap();
        store
            .replace_github_snapshot(&snapshot("First title"))
            .unwrap();
        store
            .replace_github_snapshot(&snapshot("Updated title"))
            .unwrap();
    }
    let store = EventStore::open(&database).unwrap();
    assert_eq!(
        std::fs::metadata(&database).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(store.github_account().unwrap().unwrap().login, "octocat");
    assert_eq!(store.github_repositories().unwrap().len(), 1);
    assert!(store.github_last_synced_at().unwrap().is_some());
    let loaded = store.github_repository("octocat/hello").unwrap().unwrap();
    assert_eq!(loaded.work_items.len(), 1);
    assert_eq!(loaded.work_items[0].title, "Updated title");
}
