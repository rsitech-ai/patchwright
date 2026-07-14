use patchwright_engine::serve;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};

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
    let directory = tempfile::tempdir().unwrap();
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
    let directory = tempfile::tempdir().unwrap();
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
