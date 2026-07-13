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

    server.abort();
}
