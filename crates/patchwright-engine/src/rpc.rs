use crate::{
    EventStore, GhCliCredentialBroker, GitHubAccount, GitHubRepository, GitHubSource,
    GitHubSyncSummary,
};
use anyhow::{Context, Result, bail};
use patchwright_core::{Task, TaskId};
use serde::Deserialize;
use serde_json::{Value, json};
#[cfg(unix)]
use std::os::unix::fs::FileTypeExt;
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
        if !std::fs::symlink_metadata(socket_path)?
            .file_type()
            .is_socket()
        {
            bail!("socket path exists and is not a Unix socket");
        }
        if UnixStream::connect(socket_path).await.is_ok() {
            bail!("Patchwright engine is already running on this socket");
        }
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
            Ok(request) => dispatch(request, &store).await,
            Err(error) => rpc_error(Value::Null, -32700, "parse error", Some(error.to_string())),
        };
        writer.write_all(&serde_json::to_vec(&response)?).await?;
        writer.write_all(b"\n").await?;
    }
    Ok(())
}

async fn dispatch(request: Request, store: &Mutex<EventStore>) -> Value {
    match request.method.as_str() {
        "system.health" => rpc_result(
            request.id,
            json!({"status":"ok","version":env!("CARGO_PKG_VERSION")}),
        ),
        "task.create" => create_task(request.id, &request.params, store),
        "task.timeline" => task_timeline(request.id, &request.params, store),
        "github.status" => github_status(request.id, store),
        "github.repositories" => github_repositories(request.id, store),
        "github.repository" => github_repository(request.id, &request.params, store),
        "github.sync" => sync_github(request.id, &request.params, store).await,
        _ => rpc_error(request.id, -32601, "method not found", None),
    }
}

fn create_task(id: Value, params: &Value, store: &Mutex<EventStore>) -> Value {
    let title = params
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let repository = params
        .get("repositoryPath")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match Task::new(title, repository) {
        Ok(task) => match store
            .lock()
            .expect("event store lock poisoned")
            .save_task(&task, "task created")
        {
            Ok(()) => rpc_result(id, serde_json::to_value(task).expect("task serialization")),
            Err(error) => rpc_error(id, -32000, "persistence failure", Some(error.to_string())),
        },
        Err(error) => rpc_error(id, -32602, "invalid parameters", Some(error.to_string())),
    }
}

fn task_timeline(id: Value, params: &Value, store: &Mutex<EventStore>) -> Value {
    let task_id = params
        .get("taskId")
        .and_then(Value::as_str)
        .and_then(|value| TaskId::from_str(value).ok());
    let Some(task_id) = task_id else {
        return rpc_error(
            id,
            -32602,
            "invalid parameters",
            Some("taskId must be a UUID".into()),
        );
    };
    match store
        .lock()
        .expect("event store lock poisoned")
        .timeline(task_id)
    {
        Ok(values) => rpc_result(
            id,
            json!(
                values
                    .into_iter()
                    .filter_map(|value| serde_json::from_str::<Value>(&value).ok())
                    .collect::<Vec<_>>()
            ),
        ),
        Err(error) => rpc_error(id, -32000, "persistence failure", Some(error.to_string())),
    }
}

fn github_status(id: Value, store: &Mutex<EventStore>) -> Value {
    let store = store.lock().expect("event store lock poisoned");
    match (
        store.github_account(),
        store.github_repositories(),
        store.github_last_synced_at(),
    ) {
        (Ok(account), Ok(repositories), Ok(last_synced_at)) => rpc_result(
            id,
            json!({
                "connected": account.is_some(), "account": account,
                "repositoryCount": repositories.len(), "lastSyncedAt": last_synced_at
            }),
        ),
        (Err(error), _, _) | (_, Err(error), _) | (_, _, Err(error)) => {
            rpc_error(id, -32000, "persistence failure", Some(error.to_string()))
        }
    }
}

fn github_repositories(id: Value, store: &Mutex<EventStore>) -> Value {
    match store
        .lock()
        .expect("event store lock poisoned")
        .github_repositories()
    {
        Ok(repositories) => rpc_result(id, json!(repositories)),
        Err(error) => rpc_error(id, -32000, "persistence failure", Some(error.to_string())),
    }
}

fn github_repository(id: Value, params: &Value, store: &Mutex<EventStore>) -> Value {
    let full_name = params
        .get("fullName")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if full_name.is_empty() {
        return rpc_error(
            id,
            -32602,
            "invalid parameters",
            Some("fullName is required".into()),
        );
    }
    match store
        .lock()
        .expect("event store lock poisoned")
        .github_repository(full_name)
    {
        Ok(snapshot) => rpc_result(id, json!(snapshot)),
        Err(error) => rpc_error(id, -32000, "persistence failure", Some(error.to_string())),
    }
}

async fn sync_github(id: Value, params: &Value, store: &Mutex<EventStore>) -> Value {
    let repository_limit = params
        .get("repositoryLimit")
        .and_then(Value::as_str)
        .and_then(|value| value.parse().ok())
        .unwrap_or(100)
        .clamp(1, 100);
    let resource_limit = params
        .get("resourceLimit")
        .and_then(Value::as_str)
        .and_then(|value| value.parse().ok())
        .unwrap_or(1000)
        .clamp(1, 1000);
    let gh_path = std::env::var("PATCHWRIGHT_GH_PATH").unwrap_or_else(|_| {
        if std::path::Path::new("/opt/homebrew/bin/gh").is_file() {
            "/opt/homebrew/bin/gh".into()
        } else {
            "gh".into()
        }
    });
    let api_url = std::env::var("PATCHWRIGHT_GITHUB_API_URL")
        .unwrap_or_else(|_| "https://api.github.com".into());
    let token = match GhCliCredentialBroker::new(gh_path).token() {
        Ok(token) => token,
        Err(error) => {
            return rpc_error(
                id,
                -32010,
                "GitHub authentication unavailable",
                Some(error.to_string()),
            );
        }
    };
    let source = match GitHubSource::new(api_url, token) {
        Ok(source) => source,
        Err(error) => {
            return rpc_error(
                id,
                -32011,
                "GitHub source unavailable",
                Some(error.to_string()),
            );
        }
    };
    let account = match source.account().await {
        Ok(account) => account,
        Err(error) => {
            return rpc_error(
                id,
                -32012,
                "GitHub account lookup failed",
                Some(error.to_string()),
            );
        }
    };
    let repositories = match source.repositories(repository_limit).await {
        Ok(repositories) => repositories,
        Err(error) => {
            return rpc_error(
                id,
                -32013,
                "GitHub repository discovery failed",
                Some(error.to_string()),
            );
        }
    };
    if let Err(error) = store
        .lock()
        .expect("event store lock poisoned")
        .save_github_account(&account)
    {
        return rpc_error(id, -32000, "persistence failure", Some(error.to_string()));
    }
    let summary = sync_repositories(account, repositories, source, resource_limit, store).await;
    rpc_result(id, json!(summary))
}

async fn sync_repositories(
    account: GitHubAccount,
    repositories: Vec<GitHubRepository>,
    source: GitHubSource,
    resource_limit: usize,
    store: &Mutex<EventStore>,
) -> GitHubSyncSummary {
    let mut summary = GitHubSyncSummary {
        account,
        repositories_discovered: repositories.len(),
        repositories_synced: 0,
        work_items: 0,
        discussions: 0,
        checks: 0,
        workflow_runs: 0,
        failures: Vec::new(),
    };
    let source = Arc::new(source);
    let concurrency = Arc::new(tokio::sync::Semaphore::new(4));
    let mut jobs = tokio::task::JoinSet::new();
    for repository in repositories {
        let source = Arc::clone(&source);
        let concurrency = Arc::clone(&concurrency);
        jobs.spawn(async move {
            let permit = concurrency.acquire_owned().await;
            let result = match permit {
                Ok(_permit) => {
                    source
                        .repository_snapshot(&repository, resource_limit)
                        .await
                }
                Err(error) => Err(anyhow::anyhow!("sync concurrency closed: {error}")),
            };
            (repository, result)
        });
    }
    while let Some(job) = jobs.join_next().await {
        let (repository, result) = match job {
            Ok(value) => value,
            Err(error) => {
                summary
                    .failures
                    .push(format!("repository worker failed: {error}"));
                continue;
            }
        };
        match result {
            Ok(snapshot) => {
                summary.work_items += snapshot.work_items.len();
                summary.discussions += snapshot.discussions.len();
                summary.checks += snapshot.checks.len();
                summary.workflow_runs += snapshot.workflow_runs.len();
                match store
                    .lock()
                    .expect("event store lock poisoned")
                    .replace_github_snapshot(&snapshot)
                {
                    Ok(()) => summary.repositories_synced += 1,
                    Err(error) => summary.failures.push(format!(
                        "{}: local persistence failed ({error})",
                        repository.full_name
                    )),
                }
            }
            Err(error) => summary
                .failures
                .push(format!("{}: {error}", repository.full_name)),
        }
    }
    summary
}

fn rpc_result(id: Value, result: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"result":result})
}

fn rpc_error(id: Value, code: i64, message: &str, detail: Option<String>) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message,"data":detail}})
}
