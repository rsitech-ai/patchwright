use axum::{Json, Router, routing::get};
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, process::Command, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};

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

#[tokio::test]
async fn sync_start_status_cancel_is_durable_and_single_flight() {
    let api = Router::new()
        .route(
            "/user",
            get(|| async {
                Json(json!({"login":"fixture","avatar_url":"https://example.invalid/avatar","html_url":"https://example.invalid/fixture"}))
            }),
        )
        .route(
            "/user/repos",
            get(|| async {
                tokio::time::sleep(Duration::from_secs(10)).await;
                Json(json!([]))
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let api_address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, api).await.unwrap() });

    let directory = tempfile::tempdir().unwrap();
    let gh = directory.path().join("gh-fixture");
    std::fs::write(&gh, "#!/bin/sh\nprintf '%s\\n' 'fixture-token'\n").unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = directory.path().join("engine.sock");
    let database = directory.path().join("engine.sqlite3");
    let mut engine = Command::new(env!("CARGO_BIN_EXE_patchwright-engine"))
        .args([
            "serve",
            "--socket",
            socket.to_str().unwrap(),
            "--database",
            database.to_str().unwrap(),
        ])
        .env("PATCHWRIGHT_GH_PATH", &gh)
        .env(
            "PATCHWRIGHT_GITHUB_API_URL",
            format!("http://{api_address}"),
        )
        .spawn()
        .unwrap();
    for _ in 0..200 {
        if socket.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let mut stream = BufReader::new(UnixStream::connect(&socket).await.unwrap());
    let started = call(
        &mut stream,
        json!({"jsonrpc":"2.0","id":1,"method":"github.sync.start","params":{}}),
    )
    .await;
    let job_id = started["result"]["id"].as_str().unwrap();
    assert!(matches!(
        started["result"]["state"].as_str(),
        Some("queued" | "running")
    ));

    let duplicate = call(
        &mut stream,
        json!({"jsonrpc":"2.0","id":2,"method":"github.sync.start","params":{}}),
    )
    .await;
    assert_eq!(duplicate["error"]["code"], -32014);

    let cancelled = call(
        &mut stream,
        json!({"jsonrpc":"2.0","id":3,"method":"github.sync.cancel","params":{"jobId":job_id}}),
    )
    .await;
    assert!(matches!(
        cancelled["result"]["state"].as_str(),
        Some("cancelling" | "cancelled")
    ));
    let mut terminal = cancelled;
    for request_id in 4..100 {
        if terminal["result"]["state"] == "cancelled" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
        terminal = call(
            &mut stream,
            json!({"jsonrpc":"2.0","id":request_id,"method":"github.sync.status","params":{"jobId":job_id}}),
        )
        .await;
    }
    assert_eq!(terminal["result"]["state"], "cancelled");
    assert_eq!(terminal["result"]["cancellation"], "acknowledged");

    engine.kill().unwrap();
    let _ = engine.wait();
}
