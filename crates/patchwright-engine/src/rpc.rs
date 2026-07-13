use crate::EventStore;
use anyhow::{Context, Result};
use patchwright_core::{Task, TaskId};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    path::Path,
    str::FromStr,
    sync::{Arc, Mutex},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
};

#[derive(Deserialize)]
struct Request {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Value,
    method: String,
    #[serde(default)]
    params: Value,
}

pub async fn serve(socket_path: &Path, database_path: &Path) -> Result<()> {
    if socket_path.exists() {
        std::fs::remove_file(socket_path).context("remove stale engine socket")?;
    }
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent).context("create socket directory")?;
    }
    let listener = UnixListener::bind(socket_path).context("bind engine socket")?;
    let store = Arc::new(Mutex::new(EventStore::open(database_path)?));
    loop {
        let (stream, _) = listener.accept().await.context("accept engine client")?;
        let store = Arc::clone(&store);
        tokio::spawn(async move {
            if let Err(error) = handle_connection(stream, store).await {
                tracing::warn!(error = %error, "engine client disconnected with error");
            }
        });
    }
}

async fn handle_connection(stream: UnixStream, store: Arc<Mutex<EventStore>>) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    while let Some(line) = lines.next_line().await? {
        let response = match serde_json::from_str::<Request>(&line) {
            Ok(request) => dispatch(request, &store),
            Err(error) => rpc_error(Value::Null, -32700, "parse error", Some(error.to_string())),
        };
        writer.write_all(&serde_json::to_vec(&response)?).await?;
        writer.write_all(b"\n").await?;
    }
    Ok(())
}

fn dispatch(request: Request, store: &Mutex<EventStore>) -> Value {
    match request.method.as_str() {
        "system.health" => rpc_result(
            request.id,
            json!({"status":"ok","version":env!("CARGO_PKG_VERSION")}),
        ),
        "task.create" => {
            let title = request
                .params
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let repository = request
                .params
                .get("repositoryPath")
                .and_then(Value::as_str)
                .unwrap_or_default();
            match Task::new(title, repository) {
                Ok(task) => match store
                    .lock()
                    .expect("event store lock poisoned")
                    .save_task(&task, "task created")
                {
                    Ok(()) => rpc_result(
                        request.id,
                        serde_json::to_value(task).expect("task serialization"),
                    ),
                    Err(error) => rpc_error(
                        request.id,
                        -32000,
                        "persistence failure",
                        Some(error.to_string()),
                    ),
                },
                Err(error) => rpc_error(
                    request.id,
                    -32602,
                    "invalid parameters",
                    Some(error.to_string()),
                ),
            }
        }
        "task.timeline" => {
            let task_id = request
                .params
                .get("taskId")
                .and_then(Value::as_str)
                .and_then(|value| TaskId::from_str(value).ok());
            match task_id {
                Some(task_id) => match store
                    .lock()
                    .expect("event store lock poisoned")
                    .timeline(task_id)
                {
                    Ok(values) => {
                        let decoded = values
                            .into_iter()
                            .filter_map(|value| serde_json::from_str::<Value>(&value).ok())
                            .collect::<Vec<_>>();
                        rpc_result(request.id, json!(decoded))
                    }
                    Err(error) => rpc_error(
                        request.id,
                        -32000,
                        "persistence failure",
                        Some(error.to_string()),
                    ),
                },
                None => rpc_error(
                    request.id,
                    -32602,
                    "invalid parameters",
                    Some("taskId must be a UUID".into()),
                ),
            }
        }
        _ => rpc_error(request.id, -32601, "method not found", None),
    }
}

fn rpc_result(id: Value, result: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":result})
}

fn rpc_error(id: Value, code: i64, message: &str, detail: Option<String>) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message,"data":detail}})
}
