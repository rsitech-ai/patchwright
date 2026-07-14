use crate::{
    CancellationState, ConversionError, ConversionRequest, EventStore, GhCliCredentialBroker,
    GitHubAccount, GitHubRepository, GitHubSource, GitHubSyncSummary, Job, JobId, JobKind,
    JobState, TaskConversionService,
    codex::{
        process::{CodexExecutable, CodexProcessConfig, CodexProcessFactory},
        service::{CodexRuntimeStatus, CodexService, CodexServiceError, CodexServiceState},
    },
};
use anyhow::{Context, Result, bail};
use patchwright_core::{
    CredentialHealth, RepositoryBinding, RepositoryBindingDraft, RepositoryPermissionLevel,
    RepositoryPermissionSnapshot, Task, TaskId,
};
use serde::Deserialize;
use serde_json::{Value, json};
#[cfg(unix)]
use std::os::unix::fs::FileTypeExt;
use std::{
    collections::HashMap,
    path::Path,
    str::FromStr,
    sync::{Arc, Mutex},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::{Mutex as AsyncMutex, watch},
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
    let listener = prepare_listener(socket_path).await?;
    let codex = match CodexExecutable::discover(None).await {
        Ok(executable) => {
            let version = executable.version().to_owned();
            Some(CodexService::new(
                CodexProcessFactory::new(executable, CodexProcessConfig::default()),
                version,
            ))
        }
        Err(error) => {
            tracing::warn!(error = %error, "embedded Codex is unavailable");
            None
        }
    };
    serve_with_state(listener, database_path, codex).await
}

pub async fn serve_with_codex(
    socket_path: &Path,
    database_path: &Path,
    factory: CodexProcessFactory,
    executable_version: String,
) -> Result<()> {
    let listener = prepare_listener(socket_path).await?;
    serve_with_state(
        listener,
        database_path,
        Some(CodexService::new(factory, executable_version)),
    )
    .await
}

async fn serve_with_state(
    listener: UnixListener,
    database_path: &Path,
    codex: Option<CodexService>,
) -> Result<()> {
    let state = ServerState {
        store: Arc::new(Mutex::new(EventStore::open(database_path)?)),
        codex: Arc::new(AsyncMutex::new(codex)),
        github_syncs: Arc::new(AsyncMutex::new(HashMap::new())),
    };
    loop {
        let (stream, _) = listener.accept().await.context("accept engine client")?;
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(error) = handle_connection(stream, state).await {
                tracing::warn!(error = %error, "engine client disconnected with error");
            }
        });
    }
}

async fn prepare_listener(socket_path: &Path) -> Result<UnixListener> {
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
    UnixListener::bind(socket_path).context("bind engine socket")
}

#[derive(Clone)]
struct ServerState {
    store: Arc<Mutex<EventStore>>,
    codex: Arc<AsyncMutex<Option<CodexService>>>,
    github_syncs: Arc<AsyncMutex<HashMap<JobId, watch::Sender<bool>>>>,
}

async fn handle_connection(stream: UnixStream, state: ServerState) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    while let Some(line) = lines.next_line().await? {
        let response = match serde_json::from_str::<Request>(&line) {
            Ok(request) => dispatch(request, &state).await,
            Err(error) => rpc_error(Value::Null, -32700, "parse error", Some(error.to_string())),
        };
        writer.write_all(&serde_json::to_vec(&response)?).await?;
        writer.write_all(b"\n").await?;
    }
    Ok(())
}

async fn dispatch(request: Request, state: &ServerState) -> Value {
    let store = state.store.as_ref();
    match request.method.as_str() {
        "system.health" => rpc_result(
            request.id,
            json!({"status":"ok","version":env!("CARGO_PKG_VERSION")}),
        ),
        "task.create" => create_task(request.id, &request.params, store),
        "task.list" => list_tasks(request.id, store),
        "task.timeline" => task_timeline(request.id, &request.params, store),
        "task.previewFromGitHub" => preview_task_from_github(request.id, &request.params, store),
        "task.createFromGitHub" => create_task_from_github(request.id, &request.params, store),
        "repository.bind" => bind_repository(request.id, &request.params, store),
        "github.status" => github_status(request.id, store),
        "github.repositories" => github_repositories(request.id, store),
        "github.queue" => github_queue(request.id, store),
        "github.repository" => github_repository(request.id, &request.params, store),
        "github.sync" => sync_github(request.id, &request.params, store).await,
        "github.sync.start" => github_sync_start(request.id, &request.params, state).await,
        "github.sync.status" => github_sync_status(request.id, &request.params, store),
        "github.sync.cancel" => github_sync_cancel(request.id, &request.params, state).await,
        "codex.status" => codex_status(request.id, &request.params, state).await,
        "codex.start" => codex_start(request.id, &request.params, state).await,
        "codex.events" => codex_events(request.id, &request.params, state).await,
        "codex.turn.start" => codex_turn_start(request.id, &request.params, state).await,
        "codex.turn.steer" => codex_turn_steer(request.id, &request.params, state).await,
        "codex.approvals" => codex_approvals(request.id, &request.params, state).await,
        "codex.approval.resolve" => {
            codex_approval_resolve(request.id, &request.params, state).await
        }
        "codex.pause" => codex_interrupt(request.id, &request.params, state, false).await,
        "codex.cancel" => codex_interrupt(request.id, &request.params, state, true).await,
        _ => rpc_error(request.id, -32601, "method not found", None),
    }
}

fn list_tasks(id: Value, store: &Mutex<EventStore>) -> Value {
    match store.lock().expect("event store lock poisoned").tasks() {
        Ok(tasks) => rpc_result(id, json!(tasks)),
        Err(error) => rpc_error(id, -32000, "persistence failure", Some(error.to_string())),
    }
}

fn github_queue(id: Value, store: &Mutex<EventStore>) -> Value {
    match store
        .lock()
        .expect("event store lock poisoned")
        .github_work_items()
    {
        Ok(items) => rpc_result(id, json!(items)),
        Err(error) => rpc_error(id, -32000, "persistence failure", Some(error.to_string())),
    }
}

fn bind_repository(id: Value, params: &Value, store: &Mutex<EventStore>) -> Value {
    let binding_params = match binding_parameters(params) {
        Ok(binding_params) => binding_params,
        Err(detail) => return rpc_error(id, -32602, "invalid parameters", Some(detail)),
    };
    let full_name = &binding_params.full_name;
    let installation_id = binding_params.installation_id;
    let store = store.lock().expect("event store lock poisoned");
    let snapshot = match store.github_repository(full_name) {
        Ok(Some(snapshot)) => snapshot,
        Ok(None) => {
            return rpc_error(
                id,
                -32020,
                "repository snapshot missing",
                Some("sync the repository before binding it".into()),
            );
        }
        Err(error) => {
            return rpc_error(id, -32000, "persistence failure", Some(error.to_string()));
        }
    };
    if snapshot
        .repository
        .installation_id
        .is_some_and(|expected| expected != installation_id)
    {
        return rpc_error(
            id,
            -32602,
            "invalid parameters",
            Some("installationId does not match the ingested repository".into()),
        );
    }
    match store.repository_binding_by_full_name(full_name) {
        Ok(Some(binding)) if binding.installation_id() == installation_id => {
            return rpc_result(id, json!(binding));
        }
        Ok(Some(_)) => {
            return rpc_error(
                id,
                -32021,
                "repository binding failed",
                Some("repository is already bound to a different installation".into()),
            );
        }
        Ok(None) => {}
        Err(error) => {
            return rpc_error(id, -32000, "persistence failure", Some(error.to_string()));
        }
    }
    let permissions = binding_permissions(snapshot.repository.permissions);
    let draft = RepositoryBindingDraft {
        github_repository_id: snapshot.repository.id,
        full_name: snapshot.repository.full_name.clone(),
        installation_id,
        clone_url: format!("{}.git", snapshot.repository.html_url),
        html_url: snapshot.repository.html_url,
        default_branch: snapshot.repository.default_branch,
        user_checkout: binding_params.user_checkout,
        managed_clone: binding_params.managed_clone,
        state_root: binding_params.state_root,
        worktree_root: binding_params.worktree_root,
        default_branch_sha: snapshot.repository.default_branch_sha,
        default_branch_committed_at: match snapshot.repository.default_branch_committed_at {
            Some(value) => match value.parse() {
                Ok(timestamp) => Some(timestamp),
                Err(error) => {
                    return rpc_error(
                        id,
                        -32021,
                        "repository binding failed",
                        Some(format!("invalid default branch commit timestamp: {error}")),
                    );
                }
            },
            None => None,
        },
        permissions,
        credential_health: CredentialHealth::Healthy,
    };
    let binding = match RepositoryBinding::try_from(draft) {
        Ok(binding) => binding,
        Err(error) => {
            return rpc_error(id, -32602, "invalid parameters", Some(error.to_string()));
        }
    };
    match store.save_repository_binding(&binding) {
        Ok(()) => rpc_result(id, json!(binding)),
        Err(error) => rpc_error(id, -32000, "persistence failure", Some(error.to_string())),
    }
}

struct BindingParameters {
    full_name: String,
    installation_id: u64,
    user_checkout: Option<String>,
    managed_clone: Option<String>,
    state_root: String,
    worktree_root: String,
}

fn binding_parameters(params: &Value) -> std::result::Result<BindingParameters, String> {
    let full_name = required_string(params, "repositoryFullName")
        .ok_or_else(|| "repositoryFullName is required".to_owned())?;
    let installation_id = params
        .get("installationId")
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| "installationId must be a positive integer".to_owned())?;
    let state_root =
        required_string(params, "stateRoot").ok_or_else(|| "stateRoot is required".to_owned())?;
    let worktree_root = required_string(params, "worktreeRoot")
        .ok_or_else(|| "worktreeRoot is required".to_owned())?;
    Ok(BindingParameters {
        full_name,
        installation_id,
        user_checkout: optional_string(params, "userCheckout"),
        managed_clone: optional_string(params, "managedClone"),
        state_root,
        worktree_root,
    })
}

fn preview_task_from_github(id: Value, params: &Value, store: &Mutex<EventStore>) -> Value {
    let request = match conversion_request(params) {
        Ok(request) => request,
        Err(detail) => return rpc_error(id, -32602, "invalid parameters", Some(detail)),
    };
    let store = store.lock().expect("event store lock poisoned");
    match TaskConversionService::new(&store).preview(request) {
        Ok(preview) => rpc_result(id, json!(preview)),
        Err(error) => conversion_rpc_error(id, error),
    }
}

fn create_task_from_github(id: Value, params: &Value, store: &Mutex<EventStore>) -> Value {
    let request = match conversion_request(params) {
        Ok(request) => request,
        Err(detail) => return rpc_error(id, -32602, "invalid parameters", Some(detail)),
    };
    let store = store.lock().expect("event store lock poisoned");
    match TaskConversionService::new(&store).create(request) {
        Ok(outcome) => rpc_result(id, json!(outcome)),
        Err(error) => conversion_rpc_error(id, error),
    }
}

fn conversion_request(params: &Value) -> std::result::Result<ConversionRequest, String> {
    let repository_full_name = required_string(params, "repositoryFullName")
        .ok_or_else(|| "repositoryFullName is required".to_owned())?;
    let item_number = params
        .get("itemNumber")
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| "itemNumber must be a positive integer".to_owned())?;
    let expected_updated_at = required_string(params, "expectedUpdatedAt")
        .ok_or_else(|| "expectedUpdatedAt is required".to_owned())?;
    Ok(ConversionRequest {
        repository_full_name,
        item_number,
        expected_updated_at,
    })
}

fn conversion_rpc_error(id: Value, error: ConversionError) -> Value {
    let (code, message) = match error {
        ConversionError::SnapshotMissing => (-32030, "repository snapshot missing"),
        ConversionError::ItemMissing => (-32031, "GitHub item missing"),
        ConversionError::SnapshotStale => (-32032, "GitHub item snapshot stale"),
        ConversionError::RepositoryBindingMissing => (-32033, "repository binding missing"),
        ConversionError::RepositoryBindingMismatch => (-32034, "repository binding mismatch"),
        ConversionError::ForkInaccessible => (-32035, "pull request fork inaccessible"),
        ConversionError::IncompletePullRequest => (-32036, "pull request snapshot incomplete"),
        ConversionError::IncompleteRepository => (-32037, "repository snapshot incomplete"),
        ConversionError::InvalidRequest(_) | ConversionError::InvalidContract(_) => {
            (-32602, "invalid parameters")
        }
        ConversionError::Persistence(_) => (-32000, "persistence failure"),
    };
    rpc_error(id, code, message, Some(error.to_string()))
}

fn binding_permissions(
    permissions: crate::GitHubRepositoryPermissions,
) -> RepositoryPermissionSnapshot {
    let write = if permissions.push.is_granted() {
        RepositoryPermissionLevel::Write
    } else {
        RepositoryPermissionLevel::Read
    };
    RepositoryPermissionSnapshot {
        metadata: RepositoryPermissionLevel::Read,
        contents: write,
        issues: write,
        pull_requests: write,
        checks: RepositoryPermissionLevel::Read,
        administration: RepositoryPermissionLevel::None,
    }
}

fn required_string(params: &Value, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() <= 4_096)
        .map(str::to_owned)
}

fn optional_string(params: &Value, key: &str) -> Option<String> {
    required_string(params, key)
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

async fn codex_status(id: Value, params: &Value, state: &ServerState) -> Value {
    let task_id = match codex_task_id(params) {
        Ok(task_id) => task_id,
        Err(detail) => return rpc_error(id, -32602, "invalid parameters", Some(detail)),
    };
    let codex = state.codex.lock().await;
    let result = if let Some(service) = codex.as_ref() {
        service.status(task_id, state.store.as_ref())
    } else {
        Ok(CodexRuntimeStatus {
            task_id,
            state: CodexServiceState::Unavailable,
            process_generation: None,
            account_state: None,
            thread_id: None,
            turn_id: None,
            last_sequence: 0,
            can_start: false,
            can_send: false,
            can_steer: false,
        })
    };
    match result {
        Ok(status) => rpc_result(id, json!(status)),
        Err(error) => codex_rpc_error(id, error),
    }
}

async fn codex_start(id: Value, params: &Value, state: &ServerState) -> Value {
    let task_id = match codex_task_id(params) {
        Ok(task_id) => task_id,
        Err(detail) => return rpc_error(id, -32602, "invalid parameters", Some(detail)),
    };
    let mut codex = state.codex.lock().await;
    let Some(service) = codex.as_mut() else {
        return rpc_error(id, -32050, "Codex unavailable", None);
    };
    match service.start(task_id, state.store.as_ref()).await {
        Ok(status) => rpc_result(id, json!(status)),
        Err(error) => codex_rpc_error(id, error),
    }
}

async fn codex_events(id: Value, params: &Value, state: &ServerState) -> Value {
    let task_id = match codex_task_id(params) {
        Ok(task_id) => task_id,
        Err(detail) => return rpc_error(id, -32602, "invalid parameters", Some(detail)),
    };
    let after = params
        .get("after")
        .and_then(Value::as_str)
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let limit = params
        .get("limit")
        .and_then(Value::as_str)
        .and_then(|value| value.parse().ok())
        .unwrap_or(100);
    let mut codex = state.codex.lock().await;
    let result = if let Some(service) = codex.as_mut() {
        service
            .events(task_id, after, limit, state.store.as_ref())
            .await
    } else if limit == 0 || limit > 500 {
        Err(CodexServiceError::InvalidEventLimit)
    } else {
        state
            .store
            .lock()
            .map_err(|_| CodexServiceError::StoreLock)
            .and_then(|store| {
                let mut events = store.codex_events(task_id, after)?;
                events.truncate(limit);
                Ok(events)
            })
    };
    match result {
        Ok(events) => rpc_result(id, json!(events)),
        Err(error) => codex_rpc_error(id, error),
    }
}

async fn codex_turn_start(id: Value, params: &Value, state: &ServerState) -> Value {
    codex_turn_input(id, params, state, false).await
}

async fn codex_turn_steer(id: Value, params: &Value, state: &ServerState) -> Value {
    codex_turn_input(id, params, state, true).await
}

async fn codex_approvals(id: Value, params: &Value, state: &ServerState) -> Value {
    let task_id = match codex_task_id(params) {
        Ok(value) => value,
        Err(detail) => return rpc_error(id, -32602, "invalid parameters", Some(detail)),
    };
    let mut codex = state.codex.lock().await;
    let Some(service) = codex.as_mut() else {
        return rpc_error(id, -32050, "Codex unavailable", None);
    };
    match service.approvals(task_id, state.store.as_ref()).await {
        Ok(value) => rpc_result(id, json!(value)),
        Err(error) => codex_rpc_error(id, error),
    }
}

async fn codex_approval_resolve(id: Value, params: &Value, state: &ServerState) -> Value {
    let task_id = match codex_task_id(params) {
        Ok(value) => value,
        Err(detail) => return rpc_error(id, -32602, "invalid parameters", Some(detail)),
    };
    let approval_id = params
        .get("approvalId")
        .and_then(Value::as_str)
        .and_then(|value| uuid::Uuid::parse_str(value).ok());
    let generation = params
        .get("processGeneration")
        .and_then(Value::as_str)
        .and_then(|value| uuid::Uuid::parse_str(value).ok());
    let approve = params.get("approve").and_then(|value| {
        value
            .as_bool()
            .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
    });
    let (Some(approval_id), Some(generation), Some(approve)) = (approval_id, generation, approve)
    else {
        return rpc_error(
            id,
            -32602,
            "invalid parameters",
            Some("approvalId, processGeneration, and approve are required".into()),
        );
    };
    let mut codex = state.codex.lock().await;
    let Some(service) = codex.as_mut() else {
        return rpc_error(id, -32050, "Codex unavailable", None);
    };
    match service
        .resolve_approval(
            task_id,
            approval_id,
            generation,
            approve,
            state.store.as_ref(),
        )
        .await
    {
        Ok(value) => rpc_result(id, json!(value)),
        Err(error) => codex_rpc_error(id, error),
    }
}

async fn codex_interrupt(id: Value, params: &Value, state: &ServerState, cancel: bool) -> Value {
    let task_id = match codex_task_id(params) {
        Ok(value) => value,
        Err(detail) => return rpc_error(id, -32602, "invalid parameters", Some(detail)),
    };
    let mut codex = state.codex.lock().await;
    let Some(service) = codex.as_mut() else {
        return rpc_error(id, -32050, "Codex unavailable", None);
    };
    match service
        .interrupt(task_id, cancel, state.store.as_ref())
        .await
    {
        Ok(value) => rpc_result(id, json!(value)),
        Err(error) => codex_rpc_error(id, error),
    }
}

async fn codex_turn_input(id: Value, params: &Value, state: &ServerState, steer: bool) -> Value {
    let task_id = match codex_task_id(params) {
        Ok(task_id) => task_id,
        Err(detail) => return rpc_error(id, -32602, "invalid parameters", Some(detail)),
    };
    let Some(client_message_id) = required_string(params, "clientMessageId") else {
        return rpc_error(
            id,
            -32602,
            "invalid parameters",
            Some("clientMessageId is required".into()),
        );
    };
    let Some(input) = params
        .get("input")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        return rpc_error(
            id,
            -32602,
            "invalid parameters",
            Some("input is required".into()),
        );
    };
    let mut codex = state.codex.lock().await;
    let Some(service) = codex.as_mut() else {
        return rpc_error(id, -32050, "Codex unavailable", None);
    };
    let result = if steer {
        service
            .steer_turn(task_id, &client_message_id, &input, state.store.as_ref())
            .await
    } else {
        service
            .start_turn(task_id, &client_message_id, &input, state.store.as_ref())
            .await
    };
    match result {
        Ok(receipt) => rpc_result(id, json!(receipt)),
        Err(error) => codex_rpc_error(id, error),
    }
}

fn codex_task_id(params: &Value) -> std::result::Result<TaskId, String> {
    params
        .get("taskId")
        .and_then(Value::as_str)
        .and_then(|value| TaskId::from_str(value).ok())
        .ok_or_else(|| "taskId must be a UUID".to_owned())
}

fn codex_rpc_error(id: Value, error: CodexServiceError) -> Value {
    let (code, message) = match error {
        CodexServiceError::TaskNotFound => (-32040, "task not found"),
        CodexServiceError::InvalidTaskState(_) => (-32041, "invalid task state"),
        CodexServiceError::DuplicateClientMessageId => (-32042, "duplicate client message"),
        CodexServiceError::ProcessNotActive
        | CodexServiceError::SessionNotReady
        | CodexServiceError::NoActiveTurn => (-32043, "Codex task is not ready"),
        CodexServiceError::InvalidInput | CodexServiceError::InvalidEventLimit => {
            (-32602, "invalid parameters")
        }
        CodexServiceError::ApprovalNotFound => (-32044, "Codex approval not found"),
        CodexServiceError::ApprovalInvalid => (-32045, "Codex approval expired or invalidated"),
        _ => (-32051, "Codex operation failed"),
    };
    rpc_error(id, code, message, Some(error.to_string()))
}

#[derive(Clone, Copy)]
struct GitHubSyncParameters {
    repository_limit: usize,
    resource_limit: usize,
}

impl GitHubSyncParameters {
    fn from_json(params: &Value) -> Self {
        Self {
            repository_limit: bounded_limit(params, "repositoryLimit", 100, 100),
            resource_limit: bounded_limit(params, "resourceLimit", 1000, 1000),
        }
    }
}

fn bounded_limit(params: &Value, key: &str, default: usize, maximum: usize) -> usize {
    params
        .get(key)
        .and_then(|value| {
            value
                .as_u64()
                .and_then(|value| usize::try_from(value).ok())
                .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
        })
        .unwrap_or(default)
        .clamp(1, maximum)
}

async fn github_sync_start(id: Value, params: &Value, state: &ServerState) -> Value {
    let parameters = GitHubSyncParameters::from_json(params);
    let mut active = state.github_syncs.lock().await;
    if let Some((job_id, _)) = active.iter().next() {
        return rpc_error(
            id,
            -32014,
            "GitHub sync already active",
            Some(job_id.to_string()),
        );
    }
    let job = match Job::new(JobKind::GitHubSync, None, "GitHub sync queued") {
        Ok(job) => job,
        Err(error) => return rpc_error(id, -32000, "job creation failed", Some(error.to_string())),
    };
    if let Err(error) = state
        .store
        .lock()
        .expect("event store lock poisoned")
        .create_job(&job)
    {
        return rpc_error(id, -32000, "persistence failure", Some(error.to_string()));
    }
    let (cancel, receiver) = watch::channel(false);
    active.insert(job.id(), cancel);
    drop(active);
    let background = state.clone();
    let job_id = job.id();
    tokio::spawn(async move {
        run_github_sync_job(background, job_id, parameters, receiver).await;
    });
    rpc_result(id, json!(job))
}

fn github_sync_status(id: Value, params: &Value, store: &Mutex<EventStore>) -> Value {
    let Some(job_id) = params
        .get("jobId")
        .and_then(Value::as_str)
        .and_then(|value| JobId::from_str(value).ok())
    else {
        return rpc_error(
            id,
            -32602,
            "invalid parameters",
            Some("jobId must be a UUID".into()),
        );
    };
    match store.lock().expect("event store lock poisoned").job(job_id) {
        Ok(Some(job)) if job.kind() == JobKind::GitHubSync => rpc_result(id, json!(job)),
        Ok(_) => rpc_error(id, -32015, "GitHub sync job not found", None),
        Err(error) => rpc_error(id, -32000, "persistence failure", Some(error.to_string())),
    }
}

async fn github_sync_cancel(id: Value, params: &Value, state: &ServerState) -> Value {
    let Some(job_id) = params
        .get("jobId")
        .and_then(Value::as_str)
        .and_then(|value| JobId::from_str(value).ok())
    else {
        return rpc_error(
            id,
            -32602,
            "invalid parameters",
            Some("jobId must be a UUID".into()),
        );
    };
    let job = match state
        .store
        .lock()
        .expect("event store lock poisoned")
        .job(job_id)
    {
        Ok(Some(job)) if job.kind() == JobKind::GitHubSync => job,
        Ok(_) => return rpc_error(id, -32015, "GitHub sync job not found", None),
        Err(error) => {
            return rpc_error(id, -32000, "persistence failure", Some(error.to_string()));
        }
    };
    let transition = match job.state() {
        JobState::Queued => Some((
            JobState::Cancelled,
            CancellationState::Acknowledged,
            "GitHub sync cancelled before start",
        )),
        JobState::Running => Some((
            JobState::Cancelling,
            CancellationState::Requested,
            "GitHub sync cancellation requested",
        )),
        _ => None,
    };
    if let Some((next, cancellation, summary)) = transition
        && let Err(error) = state
            .store
            .lock()
            .expect("event store lock poisoned")
            .transition_job(job_id, job.state(), next, cancellation, summary, None)
    {
        return rpc_error(id, -32000, "persistence failure", Some(error.to_string()));
    }
    if let Some(cancel) = state.github_syncs.lock().await.get(&job_id) {
        let _ = cancel.send(true);
    }
    github_sync_status(
        id,
        &json!({"jobId":job_id.to_string()}),
        state.store.as_ref(),
    )
}

async fn run_github_sync_job(
    state: ServerState,
    job_id: JobId,
    parameters: GitHubSyncParameters,
    mut cancel: watch::Receiver<bool>,
) {
    let started = state
        .store
        .lock()
        .expect("event store lock poisoned")
        .transition_job(
            job_id,
            JobState::Queued,
            JobState::Running,
            CancellationState::NotRequested,
            "GitHub sync running via gh read-only fallback",
            None,
        )
        .unwrap_or(false);
    if !started {
        state.github_syncs.lock().await.remove(&job_id);
        return;
    }
    let result = run_cancellable_github_sync(parameters, &mut cancel, state.store.as_ref()).await;
    let current = state
        .store
        .lock()
        .expect("event store lock poisoned")
        .job(job_id)
        .ok()
        .flatten();
    if let Some(current) = current {
        let cancelled = *cancel.borrow() || matches!(result, Ok((_, true)));
        let (next, cancellation, summary) = if cancelled {
            (
                JobState::Cancelled,
                CancellationState::Acknowledged,
                result
                    .as_ref()
                    .map_or("GitHub sync cancelled".to_owned(), |(summary, _)| {
                        format!(
                            "GitHub sync cancelled after {} repositories",
                            summary.repositories_synced
                        )
                    }),
            )
        } else {
            match result {
                Ok((summary, _)) => (
                    JobState::Succeeded,
                    CancellationState::NotRequested,
                    format!(
                        "GitHub sync completed: {} repositories, {} work items",
                        summary.repositories_synced, summary.work_items
                    ),
                ),
                Err(_) => (
                    JobState::Failed,
                    CancellationState::NotRequested,
                    "GitHub sync failed; inspect engine logs".to_owned(),
                ),
            }
        };
        let _ = state
            .store
            .lock()
            .expect("event store lock poisoned")
            .transition_job(job_id, current.state(), next, cancellation, &summary, None);
    }
    state.github_syncs.lock().await.remove(&job_id);
}

async fn run_cancellable_github_sync(
    parameters: GitHubSyncParameters,
    cancel: &mut watch::Receiver<bool>,
    store: &Mutex<EventStore>,
) -> Result<(GitHubSyncSummary, bool)> {
    let source = github_source_from_environment()?;
    let Some(account) = cancellable(cancel, source.account()).await? else {
        return Ok((empty_sync_summary(), true));
    };
    let Some(repositories) =
        cancellable(cancel, source.repositories(parameters.repository_limit)).await?
    else {
        return Ok((empty_sync_summary(), true));
    };
    store
        .lock()
        .expect("event store lock poisoned")
        .save_github_account(&account)?;
    Ok(sync_repositories_cancellable(
        account,
        repositories,
        source,
        parameters.resource_limit,
        store,
        cancel,
    )
    .await)
}

async fn cancellable<T>(
    cancel: &mut watch::Receiver<bool>,
    future: impl std::future::Future<Output = Result<T>>,
) -> Result<Option<T>> {
    if *cancel.borrow() {
        return Ok(None);
    }
    tokio::select! {
        result = future => result.map(Some),
        changed = cancel.changed() => {
            changed.context("GitHub sync cancellation channel closed")?;
            Ok(None)
        }
    }
}

fn github_source_from_environment() -> Result<GitHubSource> {
    let gh_path = std::env::var("PATCHWRIGHT_GH_PATH").unwrap_or_else(|_| {
        if std::path::Path::new("/opt/homebrew/bin/gh").is_file() {
            "/opt/homebrew/bin/gh".into()
        } else {
            "gh".into()
        }
    });
    let api_url = std::env::var("PATCHWRIGHT_GITHUB_API_URL")
        .unwrap_or_else(|_| "https://api.github.com".into());
    let token = GhCliCredentialBroker::new(gh_path).token()?;
    GitHubSource::new(api_url, token)
}

fn empty_sync_summary() -> GitHubSyncSummary {
    GitHubSyncSummary {
        account: GitHubAccount {
            login: "cancelled".into(),
            avatar_url: String::new(),
            html_url: String::new(),
        },
        repositories_discovered: 0,
        repositories_synced: 0,
        work_items: 0,
        discussions: 0,
        checks: 0,
        workflow_runs: 0,
        failures: Vec::new(),
    }
}

async fn sync_repositories_cancellable(
    account: GitHubAccount,
    repositories: Vec<GitHubRepository>,
    source: GitHubSource,
    resource_limit: usize,
    store: &Mutex<EventStore>,
    cancel: &mut watch::Receiver<bool>,
) -> (GitHubSyncSummary, bool) {
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
        if *cancel.borrow() {
            return (summary, true);
        }
        let source = Arc::clone(&source);
        let concurrency = Arc::clone(&concurrency);
        let mut worker_cancel = cancel.clone();
        jobs.spawn(async move {
            let permit = tokio::select! {
                permit = concurrency.acquire_owned() => permit.ok(),
                _ = worker_cancel.changed() => None,
            };
            let Some(_permit) = permit else {
                return (repository, None);
            };
            let result = tokio::select! {
                result = source.repository_snapshot(&repository, resource_limit) => Some(result),
                _ = worker_cancel.changed() => None,
            };
            (repository, result)
        });
    }
    loop {
        tokio::select! {
            changed = cancel.changed(), if !*cancel.borrow() => {
                let _ = changed;
                jobs.abort_all();
                while jobs.join_next().await.is_some() {}
                return (summary, true);
            }
            job = jobs.join_next() => {
                let Some(job) = job else {
                    return (summary, false);
                };
                let (repository, result) = match job {
                    Ok(value) => value,
                    Err(error) => {
                        if !error.is_cancelled() {
                            summary.failures.push("repository worker failed".into());
                        }
                        continue;
                    }
                };
                let Some(result) = result else {
                    jobs.abort_all();
                    while jobs.join_next().await.is_some() {}
                    return (summary, true);
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
                            Err(_) => summary.failures.push(format!(
                                "{}: local persistence failed",
                                repository.full_name
                            )),
                        }
                    }
                    Err(_) => summary
                        .failures
                        .push(format!("{}: synchronization failed", repository.full_name)),
                }
            }
        }
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
