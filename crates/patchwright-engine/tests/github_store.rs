use patchwright_engine::{
    EventStore, GitHubAccount, GitHubRepository, GitHubRepositorySnapshot, GitHubWorkItem,
    WorkItemKind,
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
            open_issues_count: 1,
        },
        work_items: vec![GitHubWorkItem {
            id: 10,
            repository_full_name: "octocat/hello".into(),
            number: 1,
            kind: WorkItemKind::Issue,
            title: title.into(),
            state: "open".into(),
            body: None,
            author: "octocat".into(),
            html_url: "https://github.com/octocat/hello/issues/1".into(),
            draft: false,
            comments_count: 0,
            head_sha: None,
            updated_at: "2026-07-13T10:00:00Z".into(),
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
