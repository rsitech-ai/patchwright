use patchwright_engine::{serve, serve_until};
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
    assert_eq!(started["result"]["state"], "pending");
    let monitor_id = started["result"]["id"].as_str().unwrap();
    let observed = call(
        stream,
        json!({
            "jsonrpc":"2.0","id":6,"method":"monitor.observe",
            "params":{
                "monitorId":monitor_id,
                "observation":json!({
                    "observedAt":"2026-07-14T09:00:00Z",
                    "headSha":"b".repeat(40),
                    "baseSha":"a".repeat(40),
                    "ci":"success",
                    "review":"approved",
                    "mergeability":"mergeable",
                    "repositoryAccessible":true,
                    "networkAvailable":true,
                    "rateLimitedUntil":null
                }).to_string()
            }
        }),
    )
    .await;
    assert_eq!(observed["result"]["outcome"]["state"], "succeeded");
    let status = call(
        stream,
        json!({
            "jsonrpc":"2.0","id":7,"method":"monitor.status",
            "params":{"monitorId":monitor_id}
        }),
    )
    .await;
    assert_eq!(status["result"]["state"], "succeeded");
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

fn owner_only_tempdir() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    directory
}
