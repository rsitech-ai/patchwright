use patchwright_engine::{
    EventStore, GitHubRepository, GitHubRepositoryPermissions, GitHubRepositorySnapshot,
    GitHubWorkItem, WorkItemKind, serve,
};
use serde_json::{Value, json};
use std::process::Command;
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
    snapshot_with_base(BASE_SHA)
}

fn snapshot_with_base(base_sha: &str) -> GitHubRepositorySnapshot {
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
            default_branch_sha: Some(base_sha.into()),
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
            state_reason: None,
            body: None,
            author: "octocat".into(),
            html_url: "https://github.com/acme/widget/issues/7".into(),
            draft: false,
            comments_count: 0,
            base_ref: None,
            base_sha: None,
            head_ref: None,
            head_sha: None,
            merged: None,
            merge_commit_sha: None,
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

fn git(repository: &std::path::Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
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

#[tokio::test]
async fn rpc_plans_and_prepares_an_isolated_worktree_before_codex() {
    let directory = tempfile::tempdir().unwrap();
    let repository = directory.path().join("managed-repository");
    std::fs::create_dir_all(&repository).unwrap();
    git(&repository, &["init", "-b", "main"]);
    std::fs::write(repository.join("README.md"), "sandbox\n").unwrap();
    git(&repository, &["add", "README.md"]);
    git(
        &repository,
        &[
            "-c",
            "user.name=Patchwright Test",
            "-c",
            "user.email=test@patchwright.local",
            "commit",
            "-m",
            "fixture",
        ],
    );
    let base_sha = git(&repository, &["rev-parse", "HEAD"]);
    let socket = directory.path().join("engine.sock");
    let database = directory.path().join("engine.sqlite3");
    {
        let store = EventStore::open(&database).unwrap();
        store
            .replace_github_snapshot(&snapshot_with_base(&base_sha))
            .unwrap();
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
    let state_root = directory.path().join("state");
    let worktree_root = directory.path().join("worktrees");
    let bound = call(
        &mut stream,
        json!({"jsonrpc":"2.0","id":1,"method":"repository.bind","params":{
            "repositoryFullName":"acme/widget","installationId":"99",
            "managedClone":repository.to_string_lossy(),
            "stateRoot":state_root.to_string_lossy(),
            "worktreeRoot":worktree_root.to_string_lossy()
        }}),
    )
    .await;
    assert_eq!(bound["result"]["fullName"], "acme/widget");
    let conversion = json!({
        "repositoryFullName":"acme/widget","itemNumber":"7",
        "expectedUpdatedAt":"2026-07-13T12:00:00Z"
    });
    let created = call(
        &mut stream,
        json!({"jsonrpc":"2.0","id":2,"method":"task.createFromGitHub","params":conversion}),
    )
    .await;
    let task_id = created["result"]["task"]["id"].as_str().unwrap();

    let planned = call(
        &mut stream,
        json!({"jsonrpc":"2.0","id":3,"method":"task.plan","params":{"taskId":task_id}}),
    )
    .await;
    assert_eq!(planned["result"]["state"], "awaitingPreparationApproval");

    let prepared = call(
        &mut stream,
        json!({"jsonrpc":"2.0","id":4,"method":"task.prepare","params":{"taskId":task_id}}),
    )
    .await;
    assert_eq!(prepared["result"]["state"], "preparing");
    let worktree = std::path::PathBuf::from(prepared["result"]["repositoryPath"].as_str().unwrap());
    assert!(worktree.join("README.md").exists());
    assert_eq!(git(&worktree, &["rev-parse", "HEAD"]), base_sha);
    assert_eq!(
        git(&worktree, &["branch", "--show-current"]),
        format!("patchwright/{task_id}")
    );
    let inspection = call(
        &mut stream,
        json!({"jsonrpc":"2.0","id":5,"method":"task.worktree","params":{"taskId":task_id}}),
    )
    .await;
    assert_eq!(inspection["result"]["headSha"], base_sha);
    assert_eq!(inspection["result"]["dirty"], false);
    server.abort();
}
