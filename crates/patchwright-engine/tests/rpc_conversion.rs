use patchwright_engine::{
    EventStore, GitHubRepository, GitHubRepositoryPermissions, GitHubRepositorySnapshot,
    GitHubWorkItem, WorkItemKind, serve,
};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};

const BASE_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

async fn call(stream: &mut BufReader<UnixStream>, request: Value) -> Value {
    stream
        .get_mut()
        .write_all(&serde_json::to_vec(&request).unwrap())
        .await
        .unwrap();
    stream.get_mut().write_all(b"\n").await.unwrap();
    let mut line = String::new();
    stream.read_line(&mut line).await.unwrap();
    serde_json::from_str(&line).unwrap()
}

fn snapshot() -> GitHubRepositorySnapshot {
    GitHubRepositorySnapshot {
        repository: GitHubRepository {
            id: 42,
            full_name: "acme/widget".into(),
            description: None,
            private: true,
            archived: false,
            default_branch: "main".into(),
            html_url: "https://github.com/acme/widget".into(),
            updated_at: "2026-07-13T12:00:00Z".into(),
            pushed_at: None,
            open_issues_count: 1,
            open_pull_request_count: 0,
            failing_check_count: 0,
            default_branch_sha: Some(BASE_SHA.into()),
            default_branch_committed_at: Some("2026-07-13T10:00:00Z".into()),
            installation_id: Some(99),
            permissions: GitHubRepositoryPermissions::default(),
        },
        work_items: vec![GitHubWorkItem {
            id: 107,
            repository_full_name: "acme/widget".into(),
            number: 7,
            kind: WorkItemKind::Issue,
            title: "Fix login".into(),
            state: "open".into(),
            body: None,
            author: "octocat".into(),
            html_url: "https://github.com/acme/widget/issues/7".into(),
            draft: false,
            comments_count: 0,
            base_ref: None,
            base_sha: None,
            head_ref: None,
            head_sha: None,
            created_at: Some("2026-07-13T08:00:00Z".into()),
            head_committed_at: None,
            latest_review_at: None,
            updated_at: "2026-07-13T12:00:00Z".into(),
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
            labels: vec![],
            assignees: vec![],
            milestone: None,
        }],
        discussions: vec![],
        checks: vec![],
        workflow_runs: vec![],
    }
}

#[tokio::test]
async fn rpc_binds_repository_then_previews_and_creates_idempotently() {
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("engine.sock");
    let database = directory.path().join("engine.sqlite3");
    {
        let store = EventStore::open(&database).unwrap();
        store.replace_github_snapshot(&snapshot()).unwrap();
    }
    let server_socket = socket.clone();
    let server_database = database.clone();
    let server = tokio::spawn(async move { serve(&server_socket, &server_database).await });
    for _ in 0..100 {
        if socket.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let mut stream = BufReader::new(UnixStream::connect(&socket).await.unwrap());
    let bound = call(
        &mut stream,
        json!({"jsonrpc":"2.0","id":1,"method":"repository.bind","params":{
            "repositoryFullName":"acme/widget","installationId":"99",
            "userCheckout":"/tmp/acme-widget","managedClone":"/tmp/patchwright/repos/acme-widget",
            "stateRoot":"/tmp/patchwright/state/acme-widget",
            "worktreeRoot":"/tmp/patchwright/worktrees/acme-widget"
        }}),
    )
    .await;
    assert_eq!(bound["result"]["fullName"], "acme/widget");

    let params = json!({
        "repositoryFullName":"acme/widget","itemNumber":"7",
        "expectedUpdatedAt":"2026-07-13T12:00:00Z"
    });
    let preview = call(
        &mut stream,
        json!({"jsonrpc":"2.0","id":2,"method":"task.previewFromGitHub","params":params}),
    )
    .await;
    assert_eq!(preview["result"]["itemNumber"], 7);
    assert_eq!(preview["result"]["requiresConfirmation"], true);

    let created = call(
        &mut stream,
        json!({"jsonrpc":"2.0","id":3,"method":"task.createFromGitHub","params":params}),
    )
    .await;
    let repeated = call(
        &mut stream,
        json!({"jsonrpc":"2.0","id":4,"method":"task.createFromGitHub","params":params}),
    )
    .await;
    assert_eq!(created["result"]["created"], true);
    assert_eq!(repeated["result"]["created"], false);
    assert_eq!(
        created["result"]["task"]["id"],
        repeated["result"]["task"]["id"]
    );
    server.abort();
}
