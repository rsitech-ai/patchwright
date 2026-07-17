use chrono::Utc;
use patchwright_core::{
    Capability, CredentialHealth, GitHubPullRequestSourceInput, RepositoryBinding,
    RepositoryBindingDraft, RepositoryPermissionSnapshot, RiskClass, Task, TaskContract,
    TaskContractDraft, TaskSource, TaskState, VerificationCommand,
};
use patchwright_engine::{
    EventStore, GitHubRepository, GitHubRepositoryPermissions, GitHubRepositorySnapshot,
    GitHubWorkItem, WorkItemKind, serve, serve_until,
};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};

const MAX_RPC_FRAME_BYTES: usize = 1024 * 1024;

async fn call(stream: &mut BufReader<UnixStream>, request: Value) -> Value {
    let bytes = serde_json::to_vec(&request).unwrap();
    stream.get_mut().write_all(&bytes).await.unwrap();
    stream.get_mut().write_all(b"\n").await.unwrap();
    let mut line = String::new();
    stream.read_line(&mut line).await.unwrap();
    serde_json::from_str(&line).unwrap()
}

#[tokio::test]
async fn socket_supports_health_create_and_timeline() {
    let directory = owner_only_tempdir();
    let socket = directory.path().join("engine.sock");
    let database = directory.path().join("engine.sqlite3");
    let server_socket = socket.clone();
    let server = tokio::spawn(async move { serve(&server_socket, &database).await });

    for _ in 0..100 {
        if socket.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let mut stream = BufReader::new(UnixStream::connect(&socket).await.unwrap());
    let health = call(
        &mut stream,
        json!({"jsonrpc":"2.0","id":1,"method":"system.health","params":{}}),
    )
    .await;
    assert_eq!(health["result"]["status"], "ok");

    let github = call(
        &mut stream,
        json!({"jsonrpc":"2.0","id":11,"method":"github.status","params":{}}),
    )
    .await;
    assert_eq!(github["result"]["connected"], false);
    assert_eq!(github["result"]["repositoryCount"], 0);

    let tasks = call(
        &mut stream,
        json!({"jsonrpc":"2.0","id":13,"method":"task.list","params":{}}),
    )
    .await;
    assert_eq!(tasks["result"].as_array().unwrap().len(), 0);
    let queue = call(
        &mut stream,
        json!({"jsonrpc":"2.0","id":14,"method":"github.queue","params":{}}),
    )
    .await;
    assert_eq!(queue["result"].as_array().unwrap().len(), 0);

    let second_database = directory.path().join("second.sqlite3");
    let second = tokio::time::timeout(
        std::time::Duration::from_millis(250),
        serve(&socket, &second_database),
    )
    .await
    .expect("a second server should fail instead of replacing the live socket")
    .unwrap_err();
    assert!(second.to_string().contains("already running"));

    let still_healthy = call(
        &mut stream,
        json!({"jsonrpc":"2.0","id":12,"method":"system.health","params":{}}),
    )
    .await;
    assert_eq!(still_healthy["result"]["status"], "ok");

    let invalid = call(
        &mut stream,
        json!({"jsonrpc":"2.0","id":2,"method":"task.create","params":{"title":""}}),
    )
    .await;
    assert_eq!(invalid["error"]["code"], -32602);

    let created = call(
        &mut stream,
        json!({
            "jsonrpc":"2.0","id":3,"method":"task.create",
            "params":{"title":"Fix issue 184","repositoryPath":"/tmp/repository"}
        }),
    )
    .await;
    let task_id = created["result"]["id"].as_str().unwrap();
    let timeline = call(
        &mut stream,
        json!({
            "jsonrpc":"2.0","id":4,"method":"task.timeline","params":{"taskId":task_id}
        }),
    )
    .await;
    assert_eq!(timeline["result"].as_array().unwrap().len(), 1);

    assert_monitor_rpc(&mut stream, task_id).await;

    server.abort();
}

async fn assert_monitor_rpc(stream: &mut BufReader<UnixStream>, task_id: &str) {
    let monitor_request = json!({
        "taskId": task_id,
        "repositoryFullName": "octocat/hello",
        "pullRequestNumber": 7,
        "expectedHeadSha": "b".repeat(40),
        "expectedBaseSha": "a".repeat(40),
        "repairBudget": 2
    });
    let started = call(
        stream,
        json!({
            "jsonrpc":"2.0","id":5,"method":"monitor.start",
            "params":{"monitor":monitor_request.to_string()}
        }),
    )
    .await;
    assert_eq!(started["error"]["code"], -32070);
    assert_eq!(
        started["error"]["data"],
        "task is not in the monitoring state"
    );
}

#[tokio::test]
async fn serve_never_deletes_a_non_socket_path() {
    let directory = owner_only_tempdir();
    let socket = directory.path().join("engine.sock");
    let database = directory.path().join("engine.sqlite3");
    std::fs::write(&socket, "keep me").unwrap();

    let error = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        serve(&socket, &database),
    )
    .await
    .expect("serve should reject a non-socket path promptly")
    .unwrap_err();

    assert!(error.to_string().contains("not a Unix socket"));
    assert_eq!(std::fs::read_to_string(&socket).unwrap(), "keep me");
}

#[tokio::test]
async fn engine_socket_parent_and_socket_are_owner_only() {
    let directory = owner_only_tempdir();
    let state = directory.path().join("state");
    std::fs::create_dir(&state).unwrap();
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = state.join("engine.sock");
    let database = state.join("engine.sqlite3");
    let server_socket = socket.clone();
    let server = tokio::spawn(async move { serve(&server_socket, &database).await });

    wait_for_socket(&socket).await;
    assert_eq!(
        std::fs::metadata(&state).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(&socket).unwrap().permissions().mode() & 0o777,
        0o600
    );

    server.abort();
}

#[tokio::test]
async fn engine_rejects_an_insecure_existing_socket_directory() {
    let directory = owner_only_tempdir();
    let state = directory.path().join("shared-state");
    std::fs::create_dir(&state).unwrap();
    std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o777)).unwrap();
    let error = serve(&state.join("engine.sock"), &state.join("engine.sqlite3"))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("owner-only"));
    assert_eq!(
        std::fs::metadata(&state).unwrap().permissions().mode() & 0o777,
        0o777
    );
}

#[tokio::test]
async fn oversized_rpc_frame_is_rejected_without_stopping_the_server() {
    let directory = owner_only_tempdir();
    let socket = directory.path().join("engine.sock");
    let database = directory.path().join("engine.sqlite3");
    let server_socket = socket.clone();
    let server = tokio::spawn(async move { serve(&server_socket, &database).await });
    wait_for_socket(&socket).await;

    let mut oversized = UnixStream::connect(&socket).await.unwrap();
    oversized
        .write_all(&vec![b'x'; MAX_RPC_FRAME_BYTES + 1])
        .await
        .unwrap();
    oversized.write_all(b"\n").await.unwrap();
    let mut response = String::new();
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        BufReader::new(oversized).read_line(&mut response),
    )
    .await
    .expect("oversized connection should be closed or rejected promptly");

    let mut healthy = BufReader::new(UnixStream::connect(&socket).await.unwrap());
    assert_eq!(
        call(
            &mut healthy,
            json!({"jsonrpc":"2.0","id":1,"method":"system.health","params":{}}),
        )
        .await["result"]["status"],
        "ok"
    );
    server.abort();
}

#[tokio::test]
async fn different_sockets_cannot_serve_the_same_database_concurrently() {
    let directory = owner_only_tempdir();
    let first_socket = directory.path().join("first.sock");
    let second_socket = directory.path().join("second.sock");
    let database = directory.path().join("engine.sqlite3");
    let first_server_socket = first_socket.clone();
    let first_database = database.clone();
    let server = tokio::spawn(async move { serve(&first_server_socket, &first_database).await });
    wait_for_socket(&first_socket).await;

    let error = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        serve(&second_socket, &database),
    )
    .await
    .expect("database lease conflict should fail promptly")
    .unwrap_err();
    assert!(error.to_string().contains("database is already in use"));
    assert!(!second_socket.exists());
    server.abort();
}

#[tokio::test]
async fn graceful_shutdown_removes_the_owned_socket_and_releases_database_lease() {
    let directory = owner_only_tempdir();
    let socket = directory.path().join("engine.sock");
    let database = directory.path().join("engine.sqlite3");
    let server_socket = socket.clone();
    let server_database = database.clone();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        serve_until(&server_socket, &server_database, async move {
            let _ = shutdown_rx.await;
        })
        .await
    });
    wait_for_socket(&socket).await;

    shutdown_tx.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(2), server)
        .await
        .expect("server should shut down promptly")
        .unwrap()
        .unwrap();
    assert!(!socket.exists());

    let (second_tx, second_rx) = tokio::sync::oneshot::channel::<()>();
    let second_socket = socket.clone();
    let second_database = database.clone();
    let second = tokio::spawn(async move {
        serve_until(&second_socket, &second_database, async move {
            let _ = second_rx.await;
        })
        .await
    });
    wait_for_socket(&socket).await;
    second_tx.send(()).unwrap();
    second.await.unwrap().unwrap();
}

async fn wait_for_socket(socket: &std::path::Path) {
    for _ in 0..200 {
        if socket.exists() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("engine socket was not created");
}

#[tokio::test]
async fn rpc_rejects_a_forged_action_preview_before_delivery_lookup() {
    let directory = owner_only_tempdir();
    let socket = directory.path().join("engine.sock");
    let database = directory.path().join("engine.sqlite3");
    let server_socket = socket.clone();
    let server = tokio::spawn(async move { serve(&server_socket, &database).await });
    wait_for_socket(&socket).await;

    let task_id = "2bbf3d95-7774-4883-b915-c10c061e03cd";
    let sha = "a".repeat(40);
    let mut stream = BufReader::new(UnixStream::connect(&socket).await.unwrap());
    let response = call(
        &mut stream,
        json!({
            "jsonrpc":"2.0",
            "id":31,
            "method":"delivery.approve",
            "params":{
                "approvedBy":"owner",
                "preview":{
                    "taskId":task_id,
                    "action":{
                        "remote":{
                            "repositoryId":42,
                            "installationId":84,
                            "repositoryFullName":"octocat/hello"
                        },
                        "action":{"kind":"comment","issueNumber":7,"body":"body"},
                        "precondition":{
                            "expectedHeadSha":null,
                            "expectedBaseSha":sha,
                            "snapshotGeneration":3
                        },
                        "payloadSha256":"0".repeat(64),
                        "idempotencySha256":"1".repeat(64),
                        "requiredPermissions":["administration:write"]
                    },
                    "fingerprint":{
                        "taskId":task_id,
                        "githubRepositoryId":42,
                        "repositoryFullName":"octocat/hello",
                        "actionKind":"postComment",
                        "pullRequestNumber":7,
                        "branch":null,
                        "headSha":null,
                        "baseSha":sha,
                        "payloadSha256":"0".repeat(64),
                        "policySha256":"2".repeat(64),
                        "instructionSha256":"3".repeat(64),
                        "invalidationGeneration":3
                    }
                }
            }
        }),
    )
    .await;

    assert_eq!(response["error"]["code"], -32602);
    server.abort();
}

#[tokio::test]
async fn monitoring_is_bound_to_the_task_and_uses_engine_synced_github_evidence() {
    let directory = owner_only_tempdir();
    let socket = directory.path().join("engine.sock");
    let database = directory.path().join("engine.sqlite3");
    let (task_id, head_sha, base_sha) = seed_monitoring_task(&database);
    let server_socket = socket.clone();
    let server = tokio::spawn(async move { serve(&server_socket, &database).await });
    wait_for_socket(&socket).await;
    let mut stream = BufReader::new(UnixStream::connect(&socket).await.unwrap());

    let wrong_target = call(
        &mut stream,
        json!({
            "jsonrpc":"2.0","id":40,"method":"monitor.start",
            "params":{"monitor":json!({
                "taskId":task_id,
                "repositoryFullName":"octocat/hello",
                "pullRequestNumber":8,
                "expectedHeadSha":head_sha,
                "expectedBaseSha":base_sha,
                "repairBudget":2
            }).to_string()}
        }),
    )
    .await;
    assert_eq!(wrong_target["error"]["code"], -32070);

    let started = call(
        &mut stream,
        json!({
            "jsonrpc":"2.0","id":41,"method":"monitor.start",
            "params":{"monitor":json!({
                "taskId":task_id,
                "repositoryFullName":"octocat/hello",
                "pullRequestNumber":7,
                "expectedHeadSha":head_sha,
                "expectedBaseSha":base_sha,
                "repairBudget":2
            }).to_string()}
        }),
    )
    .await;
    assert_eq!(started["result"]["state"], "pending");
    let monitor_id = started["result"]["id"].as_str().unwrap();

    let observed = call(
        &mut stream,
        json!({
            "jsonrpc":"2.0","id":42,"method":"monitor.observe",
            "params":{
                "monitorId":monitor_id,
                "observation":json!({
                    "observedAt":"2020-01-01T00:00:00Z",
                    "headSha":"f".repeat(40),
                    "baseSha":"e".repeat(40),
                    "ci":"failure",
                    "review":"changesRequested",
                    "mergeability":"conflicting",
                    "repositoryAccessible":false,
                    "networkAvailable":false,
                    "rateLimitedUntil":null
                }).to_string()
            }
        }),
    )
    .await;
    assert_eq!(observed["result"]["outcome"]["state"], "succeeded");
    assert_eq!(
        observed["result"]["monitor"]["latestObservation"]["headSha"],
        head_sha
    );
    let tasks = call(
        &mut stream,
        json!({"jsonrpc":"2.0","id":43,"method":"task.list","params":{}}),
    )
    .await;
    assert_eq!(tasks["result"][0]["state"], "awaitingMergeApproval");
    server.abort();
}

fn seed_monitoring_task(database: &std::path::Path) -> (String, String, String) {
    let store = EventStore::open(database).unwrap();
    let head_sha = "b".repeat(40);
    let base_sha = "a".repeat(40);
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
        default_branch_sha: Some(base_sha.clone()),
        default_branch_committed_at: Some(Utc::now()),
        permissions: RepositoryPermissionSnapshot::read_only(),
        credential_health: CredentialHealth::Healthy,
    })
    .unwrap();
    store.save_repository_binding(&binding).unwrap();
    let mut task = Task::new("Monitor exact pull request", "/tmp/hello").unwrap();
    task.repository_binding_id = Some(binding.id());
    task.source = TaskSource::github_pull_request(GitHubPullRequestSourceInput {
        repository_id: 42,
        repository_full_name: "octocat/hello".into(),
        number: 7,
        html_url: "https://github.com/octocat/hello/pull/7".into(),
        snapshot_at: Utc::now(),
        base_ref: "main".into(),
        base_sha: base_sha.clone(),
        head_ref: "repair-ci".into(),
        head_sha: head_sha.clone(),
    })
    .unwrap();
    store.save_task(&task, "task created").unwrap();
    store
        .save_task_contract(
            &TaskContract::try_from(TaskContractDraft {
                task_id: task.id,
                source: task.source.clone(),
                repository_binding_id: binding.id(),
                goal: "Monitor exact pull request".into(),
                acceptance_criteria: vec!["CI and review pass".into()],
                base_sha: Some(base_sha.clone()),
                head_sha: Some(head_sha.clone()),
                source_sha256: "c".repeat(64),
                repository_sha256: "d".repeat(64),
                instruction_digests: Vec::new(),
                verification_commands: vec![VerificationCommand::new("cargo", ["test"]).unwrap()],
                required_capabilities: vec![Capability::MergePullRequest],
                risk: RiskClass::Moderate,
                sensitive_paths: Vec::new(),
                dependencies: Vec::new(),
            })
            .unwrap(),
        )
        .unwrap();
    for state in [
        TaskState::Assessing,
        TaskState::Planned,
        TaskState::AwaitingPreparationApproval,
        TaskState::Preparing,
        TaskState::Implementing,
        TaskState::Verifying,
        TaskState::Reviewing,
        TaskState::AwaitingDeliveryApproval,
        TaskState::Delivering,
        TaskState::Monitoring,
    ] {
        task.transition(state).unwrap();
    }
    store.save_task(&task, "monitoring delivery").unwrap();
    store
        .replace_github_snapshot(&monitoring_snapshot(&head_sha, &base_sha))
        .unwrap();
    (task.id.to_string(), head_sha, base_sha)
}

fn monitoring_snapshot(head_sha: &str, base_sha: &str) -> GitHubRepositorySnapshot {
    GitHubRepositorySnapshot {
        repository: GitHubRepository {
            id: 42,
            full_name: "octocat/hello".into(),
            description: None,
            private: true,
            archived: false,
            default_branch: "main".into(),
            html_url: "https://github.com/octocat/hello".into(),
            updated_at: Utc::now().to_rfc3339(),
            pushed_at: Some(Utc::now().to_rfc3339()),
            open_issues_count: 0,
            open_pull_request_count: 1,
            failing_check_count: 0,
            default_branch_sha: Some(base_sha.into()),
            default_branch_committed_at: Some(Utc::now().to_rfc3339()),
            installation_id: Some(84),
            permissions: GitHubRepositoryPermissions::default(),
        },
        work_items: vec![GitHubWorkItem {
            id: 700,
            repository_full_name: "octocat/hello".into(),
            number: 7,
            kind: WorkItemKind::PullRequest,
            title: "Repair CI".into(),
            state: "open".into(),
            state_reason: None,
            body: None,
            author: "octocat".into(),
            html_url: "https://github.com/octocat/hello/pull/7".into(),
            draft: false,
            comments_count: 0,
            base_ref: Some("main".into()),
            base_sha: Some(base_sha.into()),
            head_ref: Some("repair-ci".into()),
            head_sha: Some(head_sha.into()),
            merged: Some(false),
            merge_commit_sha: None,
            created_at: Some(Utc::now().to_rfc3339()),
            head_committed_at: Some(Utc::now().to_rfc3339()),
            latest_review_at: Some(Utc::now().to_rfc3339()),
            updated_at: Utc::now().to_rfc3339(),
            review_decision: Some("approved".into()),
            ci_health: Some("passing".into()),
            mergeable: Some(true),
            mergeable_state: Some("clean".into()),
            rebaseable: Some(true),
            has_conflicts: Some(false),
            head_repository_full_name: Some("octocat/hello".into()),
            head_repository_fork: false,
            maintainer_can_modify: true,
            additions: 1,
            deletions: 0,
            changed_files: 1,
            labels: Vec::new(),
            assignees: Vec::new(),
            milestone: None,
        }],
        discussions: Vec::new(),
        checks: Vec::new(),
        workflow_runs: Vec::new(),
    }
}

fn owner_only_tempdir() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    directory
}
