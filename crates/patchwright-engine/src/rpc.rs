use crate::{
    CIState, CancellationState, ConversionError, ConversionRequest, EventStore,
    GhCliCredentialBroker, GitHubAccount, GitHubRepository, GitHubSource, GitHubSyncSummary,
    GitHubToken, GitHubWorkItem, Job, JobId, JobKind, JobState, Mergeability, MonitorRecord,
    MonitorState, RemoteObservation, RepositoryPlanner, ReviewState, TaskConversionService,
    WorkItemKind, approve_delivery, approve_preparation, authorize_execution,
    authorize_preparation,
    codex::{
        process::{CodexExecutable, CodexProcessConfig, CodexProcessFactory},
        service::{CodexRuntimeStatus, CodexService, CodexServiceError, CodexServiceState},
    },
    lease::DatabaseLease,
    preview_delivery, preview_preparation,
};
use anyhow::{Context, Result, bail};
use patchwright_core::{
    CredentialHealth, GitHubAction, GitHubActionPreview, QueueCandidate, RemoteIdentity,
    RemotePrecondition, RepositoryBinding, RepositoryBindingDraft, RepositoryPermissionLevel,
    RepositoryPermissionSnapshot, Task, TaskId, TaskSource, TaskState, WorkflowPreset,
    assess_queue,
};
use patchwright_relay::{
    AppAuthenticator, ConfiguredKeyProvider, ForwardedWebhook, GitHubAppConfiguration,
    GitHubMutationClient, InstallationBroker, InstallationPermissions, InstallationToken,
    KeyReference, MutationError,
};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
#[cfg(unix)]
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::{
    collections::HashMap,
    future::Future,
    path::{Path, PathBuf},
    str::FromStr,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::{Mutex as AsyncMutex, Semaphore, watch},
    task::JoinSet,
};

const MAX_RPC_FRAME_BYTES: usize = 1024 * 1024;
const MAX_RPC_CONNECTIONS: usize = 32;
const CONNECTION_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_MONITOR_SNAPSHOT_AGE: chrono::Duration = chrono::Duration::minutes(5);

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
    serve_until(socket_path, database_path, std::future::pending()).await
}

pub async fn serve_until<F>(socket_path: &Path, database_path: &Path, shutdown: F) -> Result<()>
where
    F: Future<Output = ()> + Send,
{
    tracing::debug!(database = %database_path.display(), "acquire engine database lease");
    let lease = DatabaseLease::acquire(database_path)?;
    tracing::debug!(socket = %socket_path.display(), "prepare engine socket");
    let (listener, socket_guard) = prepare_listener(socket_path).await?;
    tracing::debug!(socket = %socket_path.display(), "engine socket is ready");
    tokio::pin!(shutdown);
    let discovered = tokio::select! {
        () = &mut shutdown => return Ok(()),
        result = CodexExecutable::discover(None) => result,
    };
    let codex = match discovered {
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
    serve_with_state(
        listener,
        socket_guard,
        lease,
        database_path,
        codex,
        &mut shutdown,
    )
    .await
}

pub async fn serve_with_codex(
    socket_path: &Path,
    database_path: &Path,
    factory: CodexProcessFactory,
    executable_version: String,
) -> Result<()> {
    let lease = DatabaseLease::acquire(database_path)?;
    let (listener, socket_guard) = prepare_listener(socket_path).await?;
    serve_with_state(
        listener,
        socket_guard,
        lease,
        database_path,
        Some(CodexService::new(factory, executable_version)),
        std::future::pending(),
    )
    .await
}

async fn serve_with_state<F>(
    listener: UnixListener,
    _socket_guard: SocketGuard,
    _lease: DatabaseLease,
    database_path: &Path,
    codex: Option<CodexService>,
    shutdown: F,
) -> Result<()>
where
    F: Future<Output = ()> + Send,
{
    let state = ServerState {
        store: Arc::new(Mutex::new(EventStore::open(database_path)?)),
        codex: Arc::new(AsyncMutex::new(codex)),
        github_syncs: Arc::new(AsyncMutex::new(HashMap::new())),
    };
    let permits = Arc::new(Semaphore::new(MAX_RPC_CONNECTIONS));
    let mut connections = JoinSet::new();
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            () = &mut shutdown => break,
            accepted = accept_with_permit(&listener, Arc::clone(&permits)) => {
                let (stream, permit) = accepted?;
                if let Err(error) = verify_peer(&stream) {
                    tracing::warn!(error = %error, "reject unauthorized RPC peer");
                    continue;
                }
                let state = state.clone();
                connections.spawn(async move {
                    let _permit = permit;
                    if let Err(error) = handle_connection(stream, state).await {
                        tracing::warn!(error = %error, "engine client disconnected with error");
                    }
                });
            }
        }
    }

    for sender in state.github_syncs.lock().await.values() {
        let _ = sender.send(true);
    }
    state.github_syncs.lock().await.clear();
    let codex_shutdown = if let Some(service) = state.codex.lock().await.as_mut() {
        service
            .shutdown(state.store.as_ref())
            .await
            .map_err(Into::into)
    } else {
        Ok(())
    };
    connections.abort_all();
    let _ = tokio::time::timeout(CONNECTION_DRAIN_TIMEOUT, async {
        while connections.join_next().await.is_some() {}
    })
    .await;
    codex_shutdown
}

async fn accept_with_permit(
    listener: &UnixListener,
    permits: Arc<Semaphore>,
) -> Result<(UnixStream, tokio::sync::OwnedSemaphorePermit)> {
    let permit = permits
        .acquire_owned()
        .await
        .context("RPC connection semaphore closed")?;
    let (stream, _) = listener.accept().await.context("accept engine client")?;
    Ok((stream, permit))
}

async fn prepare_listener(socket_path: &Path) -> Result<(UnixListener, SocketGuard)> {
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
        if parent.exists() {
            validate_socket_parent(parent)?;
        } else {
            std::fs::create_dir_all(parent).context("create socket directory")?;
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
                .context("restrict socket directory permissions")?;
        }
    }
    let listener = UnixListener::bind(socket_path).context("bind engine socket")?;
    std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o600))
        .context("restrict engine socket permissions")?;
    let guard = SocketGuard::new(socket_path)?;
    Ok((listener, guard))
}

fn validate_socket_parent(parent: &Path) -> Result<()> {
    let metadata = std::fs::symlink_metadata(parent).context("inspect socket directory")?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        bail!("socket parent must be a real directory")
    }
    if metadata.uid() != nix::unistd::geteuid().as_raw() || metadata.mode() & 0o077 != 0 {
        bail!("socket parent must be owned by the current user and owner-only")
    }
    Ok(())
}

fn verify_peer(stream: &UnixStream) -> Result<()> {
    let (peer_uid, _) = nix::unistd::getpeereid(stream).context("inspect RPC peer credentials")?;
    if peer_uid != nix::unistd::geteuid() {
        bail!("RPC peer is not owned by the current user")
    }
    Ok(())
}

struct SocketGuard {
    path: PathBuf,
    device: u64,
    inode: u64,
}

impl SocketGuard {
    fn new(path: &Path) -> Result<Self> {
        let metadata = std::fs::symlink_metadata(path).context("inspect owned engine socket")?;
        Ok(Self {
            path: path.to_owned(),
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
}

impl Drop for SocketGuard {
    fn drop(&mut self) {
        let Ok(metadata) = std::fs::symlink_metadata(&self.path) else {
            return;
        };
        if metadata.file_type().is_socket()
            && metadata.dev() == self.device
            && metadata.ino() == self.inode
        {
            if let Err(error) = std::fs::remove_file(&self.path) {
                tracing::warn!(error = %error, path = %self.path.display(), "remove engine socket");
            }
        }
    }
}

#[derive(Clone)]
struct ServerState {
    store: Arc<Mutex<EventStore>>,
    codex: Arc<AsyncMutex<Option<CodexService>>>,
    github_syncs: Arc<AsyncMutex<HashMap<JobId, watch::Sender<bool>>>>,
}

async fn handle_connection(stream: UnixStream, state: ServerState) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    loop {
        let mut frame = Vec::with_capacity(4096);
        let read = (&mut reader)
            .take((MAX_RPC_FRAME_BYTES + 1) as u64)
            .read_until(b'\n', &mut frame)
            .await?;
        if read == 0 {
            break;
        }
        if frame.len() > MAX_RPC_FRAME_BYTES || frame.last() != Some(&b'\n') {
            let response = rpc_error(Value::Null, -32600, "request frame is too large", None);
            writer.write_all(&serde_json::to_vec(&response)?).await?;
            writer.write_all(b"\n").await?;
            break;
        }
        frame.pop();
        let response = match serde_json::from_slice::<Request>(&frame) {
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
        "task.worktree" => task_worktree(request.id, &request.params, store),
        "task.plan" => task_plan(request.id, &request.params, store).await,
        "task.contract" => task_contract(request.id, &request.params, store),
        "task.preparation.preview" => task_preparation_preview(request.id, &request.params, store),
        "task.preparation.approve" => task_preparation_approve(request.id, &request.params, store),
        "task.prepare" => task_prepare(request.id, &request.params, store).await,
        "task.readyForDelivery" => {
            task_ready_for_delivery(request.id, &request.params, store).await
        }
        "task.previewFromGitHub" => preview_task_from_github(request.id, &request.params, store),
        "task.createFromGitHub" => create_task_from_github(request.id, &request.params, store),
        "task.reconcileGitHub" => task_reconcile_github(request.id, &request.params, store).await,
        "repository.bind" => bind_repository(request.id, &request.params, store),
        "github.status" => github_status(request.id, store),
        "github.repositories" => github_repositories(request.id, store),
        "github.queue" => github_queue(request.id, store),
        "github.queue.assess" => github_queue_assess(request.id, &request.params, store),
        "github.queue.decisions" => github_queue_decisions(request.id, store),
        "github.repository" => github_repository(request.id, &request.params, store),
        "github.sync" => sync_github(request.id, &request.params, store).await,
        "github.sync.repository" => {
            sync_github_repository(request.id, &request.params, store).await
        }
        "github.sync.start" => github_sync_start(request.id, &request.params, state).await,
        "github.sync.status" => github_sync_status(request.id, &request.params, store),
        "github.sync.cancel" => github_sync_cancel(request.id, &request.params, state).await,
        "github.webhook.ingest" => github_webhook_ingest(request.id, &request.params, store),
        "delivery.preview" => delivery_preview(request.id, &request.params, store),
        "delivery.approve" => delivery_approve(request.id, &request.params, store),
        "delivery.execute" => delivery_execute(request.id, &request.params, store).await,
        "delivery.status" => delivery_status(request.id, &request.params, store),
        "monitor.start" => monitor_start(request.id, &request.params, store),
        "monitor.status" => monitor_status(request.id, &request.params, store),
        "monitor.observe" => monitor_observe(request.id, &request.params, store),
        "monitor.wake" => monitor_wake(request.id, &request.params, store),
        "monitor.cancel" => monitor_cancel(request.id, &request.params, store),
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

fn github_webhook_ingest(id: Value, params: &Value, store: &Mutex<EventStore>) -> Value {
    let delivery: ForwardedWebhook = match serde_json::from_value(params.clone()) {
        Ok(delivery) => delivery,
        Err(error) => {
            return rpc_error(id, -32602, "invalid parameters", Some(error.to_string()));
        }
    };
    match store
        .lock()
        .expect("event store lock poisoned")
        .ingest_github_webhook(&delivery)
    {
        Ok(outcome) => rpc_result(id, json!(outcome)),
        Err(error) => rpc_error(
            id,
            -32072,
            "webhook ingestion failed",
            Some(error.to_string()),
        ),
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

fn github_queue_decisions(id: Value, store: &Mutex<EventStore>) -> Value {
    match store
        .lock()
        .expect("event store lock poisoned")
        .queue_decisions()
    {
        Ok(decisions) => rpc_result(id, json!(decisions)),
        Err(error) => rpc_error(id, -32000, "persistence failure", Some(error.to_string())),
    }
}

fn github_queue_assess(id: Value, params: &Value, store: &Mutex<EventStore>) -> Value {
    let preset_value = params
        .get("preset")
        .and_then(Value::as_str)
        .unwrap_or("quickWins");
    let Ok(preset) = serde_json::from_value::<WorkflowPreset>(Value::String(preset_value.into()))
    else {
        return rpc_error(
            id,
            -32602,
            "invalid parameters",
            Some("preset is not a supported workflow".into()),
        );
    };
    let store = store.lock().expect("event store lock poisoned");
    let work_items = match store.github_work_items() {
        Ok(items) => items,
        Err(error) => {
            return rpc_error(id, -32000, "persistence failure", Some(error.to_string()));
        }
    };
    let candidates: Vec<QueueCandidate> = work_items
        .into_iter()
        .filter(|item| item.kind == crate::WorkItemKind::PullRequest && item.state == "open")
        .filter_map(|item| {
            let updated_at = chrono::DateTime::parse_from_rfc3339(&item.updated_at)
                .ok()?
                .with_timezone(&chrono::Utc);
            Some(QueueCandidate {
                repository_full_name: item.repository_full_name,
                number: item.number,
                title: item.title,
                draft: item.draft,
                ci_health: item.ci_health,
                review_decision: item.review_decision,
                has_conflicts: item.has_conflicts,
                updated_at,
                dependency_numbers: dependency_labels(&item.labels),
                labels: item.labels,
                changed_paths: Vec::new(),
                manual_priority: None,
                pinned: false,
            })
        })
        .collect();
    let decisions = match assess_queue(&candidates, preset, chrono::Utc::now()) {
        Ok(decisions) => decisions,
        Err(error) => {
            return rpc_error(
                id,
                -32030,
                "queue assessment failed",
                Some(error.to_string()),
            );
        }
    };
    if let Err(error) = store.replace_queue_decisions(&decisions) {
        return rpc_error(id, -32000, "persistence failure", Some(error.to_string()));
    }
    rpc_result(id, json!(decisions))
}

fn dependency_labels(labels: &[String]) -> Vec<u64> {
    labels
        .iter()
        .filter_map(|label| {
            label
                .strip_prefix("depends-on:#")
                .and_then(|value| value.parse().ok())
        })
        .collect()
}

fn delivery_preview(id: Value, params: &Value, store: &Mutex<EventStore>) -> Value {
    let Some(task_id) = params
        .get("taskId")
        .and_then(Value::as_str)
        .and_then(|value| TaskId::from_str(value).ok())
    else {
        return rpc_error(
            id,
            -32602,
            "invalid parameters",
            Some("taskId is required".into()),
        );
    };
    let request: ActionPreviewRequest = match encoded_parameter(params, "actionPreview") {
        Ok(request) => request,
        Err(detail) => return rpc_error(id, -32602, "invalid parameters", Some(detail)),
    };
    let store = store.lock().expect("event store lock poisoned");
    let contract = match store.task_contract(task_id) {
        Ok(Some(contract)) => contract,
        Ok(None) => {
            return rpc_error(
                id,
                -32060,
                "delivery preview failed",
                Some("task contract is missing".into()),
            );
        }
        Err(error) => return rpc_error(id, -32000, "persistence failure", Some(error.to_string())),
    };
    let precondition = match RemotePrecondition::new(
        request.expected_head_sha.as_deref().or(contract.head_sha()),
        request.expected_base_sha.as_deref().or(contract.base_sha()),
        request.snapshot_generation,
    ) {
        Ok(precondition) => precondition,
        Err(error) => return rpc_error(id, -32602, "invalid parameters", Some(error.to_string())),
    };
    let action = match GitHubActionPreview::new(request.remote, request.action, precondition) {
        Ok(action) => action,
        Err(error) => return rpc_error(id, -32602, "invalid parameters", Some(error.to_string())),
    };
    match preview_delivery(&store, task_id, action) {
        Ok(preview) => rpc_result(id, json!(preview)),
        Err(error) => rpc_error(
            id,
            -32060,
            "delivery preview failed",
            Some(error.to_string()),
        ),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActionPreviewRequest {
    remote: RemoteIdentity,
    action: GitHubAction,
    expected_head_sha: Option<String>,
    expected_base_sha: Option<String>,
    snapshot_generation: u64,
}

fn delivery_approve(id: Value, params: &Value, store: &Mutex<EventStore>) -> Value {
    let preview = match encoded_parameter(params, "preview") {
        Ok(preview) => preview,
        Err(detail) => return rpc_error(id, -32602, "invalid parameters", Some(detail)),
    };
    let approved_by = params
        .get("approvedBy")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match approve_delivery(
        &store.lock().expect("event store lock poisoned"),
        &preview,
        approved_by,
    ) {
        Ok(approval) => rpc_result(id, json!(approval)),
        Err(error) => rpc_error(
            id,
            -32061,
            "delivery approval failed",
            Some(error.to_string()),
        ),
    }
}

async fn delivery_execute(id: Value, params: &Value, store: &Mutex<EventStore>) -> Value {
    let preview: crate::DeliveryPreview = match encoded_parameter(params, "preview") {
        Ok(preview) => preview,
        Err(detail) => return rpc_error(id, -32602, "invalid parameters", Some(detail)),
    };
    let Some(approval_id) = params
        .get("approvalId")
        .and_then(Value::as_str)
        .and_then(|value| uuid::Uuid::parse_str(value).ok())
    else {
        return rpc_error(
            id,
            -32602,
            "invalid parameters",
            Some("approvalId is required".into()),
        );
    };
    let key = match authorize_execution(
        &store.lock().expect("event store lock poisoned"),
        &preview,
        approval_id,
    ) {
        Ok(key) => key,
        Err(error) => {
            return rpc_error(
                id,
                -32062,
                "delivery authorization failed",
                Some(error.to_string()),
            );
        }
    };
    let result = execute_github_action(&preview, store).await;
    match result {
        Ok(result) => {
            let encoded = serde_json::to_string(&json!({"state":"succeeded","result":result}))
                .expect("mutation result serializes");
            if let Err(error) = crate::complete_successful_delivery(
                &store.lock().expect("event store lock poisoned"),
                &preview,
                &key,
                &encoded,
                result.merged == Some(true),
            ) {
                return rpc_error(id, -32000, "persistence failure", Some(error.to_string()));
            }
            rpc_result(
                id,
                json!({"idempotencyKey":key,"state":"succeeded","result":result}),
            )
        }
        Err(error @ MutationError::AmbiguousTransport) => rpc_error(
            id,
            -32063,
            "delivery outcome is ambiguous",
            Some(error.to_string()),
        ),
        Err(error) => {
            let encoded =
                serde_json::to_string(&json!({"state":"failed","error":error.to_string()}))
                    .expect("failure result serializes");
            if let Err(persistence) = crate::complete_failed_delivery(
                &store.lock().expect("event store lock poisoned"),
                &preview,
                &key,
                &encoded,
            ) {
                return rpc_error(
                    id,
                    -32000,
                    "persistence failure",
                    Some(persistence.to_string()),
                );
            }
            rpc_error(id, -32064, "delivery failed", Some(error.to_string()))
        }
    }
}

fn delivery_status(id: Value, params: &Value, store: &Mutex<EventStore>) -> Value {
    let key = params
        .get("idempotencyKey")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if key.len() != 64 || !key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return rpc_error(
            id,
            -32602,
            "invalid parameters",
            Some("idempotencyKey must be a SHA-256 value".into()),
        );
    }
    match store
        .lock()
        .expect("event store lock poisoned")
        .delivery_result(key)
    {
        Ok(Some(result)) => match serde_json::from_str::<Value>(&result) {
            Ok(result) => rpc_result(
                id,
                json!({"idempotencyKey":key,"claimed":true,"result":result}),
            ),
            Err(error) => rpc_error(id, -32000, "persistence failure", Some(error.to_string())),
        },
        Ok(None) => match store
            .lock()
            .expect("event store lock poisoned")
            .delivery_claimed(key)
        {
            Ok(claimed) => rpc_result(
                id,
                json!({"idempotencyKey":key,"claimed":claimed,"result":null}),
            ),
            Err(error) => rpc_error(id, -32000, "persistence failure", Some(error.to_string())),
        },
        Err(error) => rpc_error(id, -32000, "persistence failure", Some(error.to_string())),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MonitorStartRequest {
    task_id: TaskId,
    repository_full_name: String,
    pull_request_number: u64,
    expected_head_sha: String,
    expected_base_sha: String,
    #[serde(default = "default_repair_budget")]
    repair_budget: u32,
}

const fn default_repair_budget() -> u32 {
    3
}

fn monitor_start(id: Value, params: &Value, store: &Mutex<EventStore>) -> Value {
    let request: MonitorStartRequest = match encoded_parameter(params, "monitor") {
        Ok(request) => request,
        Err(detail) => return rpc_error(id, -32602, "invalid parameters", Some(detail)),
    };
    let store = store.lock().expect("event store lock poisoned");
    let monitor = match create_verified_monitor(&store, request) {
        Ok(monitor) => monitor,
        Err(MonitorStartFailure::Rejected(detail)) => {
            return rpc_error(id, -32070, "monitor start failed", Some(detail));
        }
        Err(MonitorStartFailure::Invalid(detail)) => {
            return rpc_error(id, -32602, "invalid parameters", Some(detail));
        }
        Err(MonitorStartFailure::Persistence(detail)) => {
            return rpc_error(id, -32000, "persistence failure", Some(detail));
        }
    };
    match store.save_monitor(&monitor) {
        Ok(()) => rpc_result(id, json!(monitor)),
        Err(error) => rpc_error(id, -32000, "persistence failure", Some(error.to_string())),
    }
}

enum MonitorStartFailure {
    Rejected(String),
    Invalid(String),
    Persistence(String),
}

fn create_verified_monitor(
    store: &EventStore,
    request: MonitorStartRequest,
) -> std::result::Result<MonitorRecord, MonitorStartFailure> {
    let task = store
        .load_task(request.task_id)
        .map_err(|error| MonitorStartFailure::Persistence(error.to_string()))?
        .ok_or_else(|| MonitorStartFailure::Rejected("task is missing".into()))?;
    if task.state != TaskState::Monitoring {
        return Err(MonitorStartFailure::Rejected(
            "task is not in the monitoring state".into(),
        ));
    }
    let contract = store
        .task_contract(task.id)
        .map_err(|error| MonitorStartFailure::Persistence(error.to_string()))?
        .ok_or_else(|| MonitorStartFailure::Rejected("task contract is missing".into()))?;
    if contract.source() != &task.source {
        return Err(MonitorStartFailure::Rejected(
            "task source no longer matches its contract".into(),
        ));
    }
    if task.repository_binding_id != Some(contract.repository_binding_id()) {
        return Err(MonitorStartFailure::Rejected(
            "task repository binding no longer matches its contract".into(),
        ));
    }
    let binding = store
        .repository_binding(contract.repository_binding_id())
        .map_err(|error| MonitorStartFailure::Persistence(error.to_string()))?
        .ok_or_else(|| {
            MonitorStartFailure::Rejected("task repository binding is missing".into())
        })?;
    if binding.full_name() != request.repository_full_name {
        return Err(MonitorStartFailure::Rejected(
            "monitor repository does not match the task binding".into(),
        ));
    }
    let (snapshot, snapshot_at) =
        fresh_github_snapshot(store, &request.repository_full_name, chrono::Utc::now())
            .map_err(MonitorStartFailure::Rejected)?;
    let Some(item) = snapshot.work_items.iter().find(|item| {
        item.kind == WorkItemKind::PullRequest
            && item.repository_full_name == request.repository_full_name
            && item.number == request.pull_request_number
    }) else {
        return Err(MonitorStartFailure::Rejected(
            "pull request is missing from the fresh GitHub snapshot".into(),
        ));
    };
    validate_monitor_target(&task, item, &request).map_err(MonitorStartFailure::Rejected)?;
    match MonitorRecord::new(
        request.task_id,
        request.repository_full_name,
        request.pull_request_number,
        request.expected_head_sha,
        request.expected_base_sha,
        snapshot_at,
        request.repair_budget,
    ) {
        Ok(monitor) => Ok(monitor),
        Err(error) => Err(MonitorStartFailure::Invalid(error.to_string())),
    }
}

fn monitor_status(id: Value, params: &Value, store: &Mutex<EventStore>) -> Value {
    let Some(monitor_id) = monitor_id(params) else {
        return rpc_error(
            id,
            -32602,
            "invalid parameters",
            Some("monitorId is required".into()),
        );
    };
    match store
        .lock()
        .expect("event store lock poisoned")
        .monitor(monitor_id)
    {
        Ok(Some(monitor)) => rpc_result(id, json!(monitor)),
        Ok(None) => rpc_error(id, -32071, "monitor is missing", None),
        Err(error) => rpc_error(id, -32000, "persistence failure", Some(error.to_string())),
    }
}

fn monitor_observe(id: Value, params: &Value, store: &Mutex<EventStore>) -> Value {
    let Some(monitor_id) = monitor_id(params) else {
        return rpc_error(
            id,
            -32602,
            "invalid parameters",
            Some("monitorId is required".into()),
        );
    };
    let store = store.lock().expect("event store lock poisoned");
    let mut monitor = match store.monitor(monitor_id) {
        Ok(Some(monitor)) => monitor,
        Ok(None) => return rpc_error(id, -32071, "monitor is missing", None),
        Err(error) => return rpc_error(id, -32000, "persistence failure", Some(error.to_string())),
    };
    let now = chrono::Utc::now();
    let (snapshot, snapshot_at) =
        match fresh_github_snapshot(&store, &monitor.repository_full_name, now) {
            Ok(snapshot) => snapshot,
            Err(detail) => {
                return rpc_error(id, -32072, "monitor observation rejected", Some(detail));
            }
        };
    let Some(item) = snapshot.work_items.iter().find(|item| {
        item.kind == WorkItemKind::PullRequest
            && item.repository_full_name == monitor.repository_full_name
            && item.number == monitor.pull_request_number
    }) else {
        return rpc_error(
            id,
            -32072,
            "monitor observation rejected",
            Some("pull request is missing from the fresh GitHub snapshot".into()),
        );
    };
    let observation = match observation_from_github_item(item, snapshot_at) {
        Ok(observation) => observation,
        Err(detail) => {
            return rpc_error(id, -32072, "monitor observation rejected", Some(detail));
        }
    };
    let outcome = match monitor.observe(observation, now) {
        Ok(outcome) => outcome,
        Err(error) => {
            return rpc_error(
                id,
                -32072,
                "monitor observation rejected",
                Some(error.to_string()),
            );
        }
    };
    if outcome.invalidate_approvals {
        if let Err(error) = store.invalidate_task_approvals(monitor.task_id) {
            return rpc_error(id, -32000, "persistence failure", Some(error.to_string()));
        }
    }
    if outcome.state == MonitorState::Succeeded {
        let mut task = match store.load_task(monitor.task_id) {
            Ok(Some(task)) => task,
            Ok(None) => return rpc_error(id, -32040, "task is missing", None),
            Err(error) => {
                return rpc_error(id, -32000, "persistence failure", Some(error.to_string()));
            }
        };
        if task.state != TaskState::Monitoring {
            return rpc_error(
                id,
                -32072,
                "monitor observation rejected",
                Some("task is not in the monitoring state".into()),
            );
        }
        if let Err(error) = task.transition(TaskState::AwaitingMergeApproval) {
            return rpc_error(
                id,
                -32072,
                "monitor observation rejected",
                Some(error.to_string()),
            );
        }
        return match store.save_monitor_with_task_event(
            &monitor,
            &task,
            "GitHub evidence satisfied monitoring; merge approval is required",
        ) {
            Ok(()) => rpc_result(id, json!({"monitor":monitor,"outcome":outcome})),
            Err(error) => rpc_error(id, -32000, "persistence failure", Some(error.to_string())),
        };
    }
    match store.save_monitor(&monitor) {
        Ok(()) => rpc_result(id, json!({"monitor":monitor,"outcome":outcome})),
        Err(error) => rpc_error(id, -32000, "persistence failure", Some(error.to_string())),
    }
}

fn fresh_github_snapshot(
    store: &EventStore,
    repository_full_name: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> std::result::Result<
    (
        crate::GitHubRepositorySnapshot,
        chrono::DateTime<chrono::Utc>,
    ),
    String,
> {
    let (snapshot, snapshot_at) = store
        .github_repository_with_snapshot_at(repository_full_name)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "repository is missing from the GitHub snapshot".to_owned())?;
    let age = now.signed_duration_since(snapshot_at);
    if age < chrono::Duration::zero() || age > MAX_MONITOR_SNAPSHOT_AGE {
        return Err("GitHub snapshot is stale; sync the repository before monitoring".into());
    }
    Ok((snapshot, snapshot_at))
}

fn validate_monitor_target(
    task: &Task,
    item: &GitHubWorkItem,
    request: &MonitorStartRequest,
) -> std::result::Result<(), String> {
    if item.head_sha.as_deref() != Some(request.expected_head_sha.as_str())
        || item.base_sha.as_deref() != Some(request.expected_base_sha.as_str())
    {
        return Err("monitor SHA identity does not match the fresh pull request".into());
    }
    match &task.source {
        TaskSource::GitHubPullRequest(_) => {
            if task.source.repository_full_name() != Some(request.repository_full_name.as_str())
                || task.source.item_number() != Some(request.pull_request_number)
                || task.source.head_sha() != Some(request.expected_head_sha.as_str())
                || task.source.base_sha() != Some(request.expected_base_sha.as_str())
            {
                return Err("monitor target does not match the task pull-request source".into());
            }
        }
        TaskSource::GitHubIssue(_) => {
            if task.source.repository_full_name() != Some(request.repository_full_name.as_str()) {
                return Err("monitor repository does not match the task issue source".into());
            }
            let expected_branch = format!("patchwright/{}", task.id);
            if item.head_ref.as_deref() != Some(expected_branch.as_str()) {
                return Err("monitor pull request is not the task's prepared branch".into());
            }
        }
        TaskSource::LocalRequest => {
            let expected_branch = format!("patchwright/{}", task.id);
            if item.head_ref.as_deref() != Some(expected_branch.as_str()) {
                return Err("monitor pull request is not the task's prepared branch".into());
            }
        }
    }
    Ok(())
}

fn observation_from_github_item(
    item: &GitHubWorkItem,
    observed_at: chrono::DateTime<chrono::Utc>,
) -> std::result::Result<RemoteObservation, String> {
    let head_sha = item
        .head_sha
        .clone()
        .ok_or_else(|| "pull request head SHA is missing".to_owned())?;
    let base_sha = item
        .base_sha
        .clone()
        .ok_or_else(|| "pull request base SHA is missing".to_owned())?;
    let ci = match item.ci_health.as_deref() {
        Some("passing") => CIState::Success,
        Some("failing") => CIState::Failure,
        Some("pending" | "unknown") | None => CIState::Pending,
        Some(_) => return Err("pull request CI state is unsupported".into()),
    };
    let review = match item.review_decision.as_deref() {
        Some("approved") => ReviewState::Approved,
        Some("changesRequested") => ReviewState::ChangesRequested,
        Some("approvalDismissed") => ReviewState::ApprovalDismissed,
        Some("reviewRequired") | None => ReviewState::Pending,
        Some(_) => return Err("pull request review state is unsupported".into()),
    };
    let mergeability = if item.has_conflicts == Some(true) {
        Mergeability::Conflicting
    } else {
        match item.mergeable {
            Some(true) => Mergeability::Mergeable,
            Some(false) => Mergeability::Conflicting,
            None => Mergeability::Unknown,
        }
    };
    Ok(RemoteObservation {
        observed_at,
        head_sha,
        base_sha,
        ci,
        review,
        mergeability,
        repository_accessible: true,
        network_available: true,
        rate_limited_until: None,
    })
}

fn monitor_wake(id: Value, params: &Value, store: &Mutex<EventStore>) -> Value {
    mutate_monitor(id, params, store, |monitor| {
        monitor.wake(chrono::Utc::now())
    })
}

fn monitor_cancel(id: Value, params: &Value, store: &Mutex<EventStore>) -> Value {
    mutate_monitor(id, params, store, |monitor| {
        monitor.cancel(chrono::Utc::now())
    })
}

fn mutate_monitor(
    id: Value,
    params: &Value,
    store: &Mutex<EventStore>,
    mutate: impl FnOnce(&mut MonitorRecord) -> bool,
) -> Value {
    let Some(monitor_id) = monitor_id(params) else {
        return rpc_error(
            id,
            -32602,
            "invalid parameters",
            Some("monitorId is required".into()),
        );
    };
    let store = store.lock().expect("event store lock poisoned");
    let mut monitor = match store.monitor(monitor_id) {
        Ok(Some(monitor)) => monitor,
        Ok(None) => return rpc_error(id, -32071, "monitor is missing", None),
        Err(error) => return rpc_error(id, -32000, "persistence failure", Some(error.to_string())),
    };
    let changed = mutate(&mut monitor);
    if changed {
        if let Err(error) = store.save_monitor(&monitor) {
            return rpc_error(id, -32000, "persistence failure", Some(error.to_string()));
        }
    }
    rpc_result(id, json!({"changed":changed,"monitor":monitor}))
}

fn monitor_id(params: &Value) -> Option<uuid::Uuid> {
    params
        .get("monitorId")
        .and_then(Value::as_str)
        .and_then(|value| uuid::Uuid::parse_str(value).ok())
}

async fn execute_github_action(
    preview: &crate::DeliveryPreview,
    store: &Mutex<EventStore>,
) -> std::result::Result<patchwright_relay::MutationResult, MutationError> {
    let runtime = github_app_runtime_configuration().ok_or(MutationError::MissingToken)?;
    let app_id = runtime.app_id;
    let client_id = runtime.client_id;
    let key_reference =
        KeyReference::parse(&runtime.key_reference).map_err(|_| MutationError::MissingToken)?;
    let api_url = runtime.api_base_url;
    let configuration =
        GitHubAppConfiguration::new(app_id, client_id, key_reference, api_url.clone())
            .map_err(|_| MutationError::MissingToken)?;
    let authenticator = AppAuthenticator::new(configuration, ConfiguredKeyProvider)
        .map_err(|_| MutationError::MissingToken)?;
    let broker = InstallationBroker::new(authenticator, &api_url)
        .map_err(|_| MutationError::MissingToken)?;
    let full_name = preview.action.remote().repository_full_name();
    let (owner, repository) = full_name
        .split_once('/')
        .ok_or(MutationError::InvalidTarget)?;
    let token = broker
        .token_for_repository(
            owner,
            repository,
            preview.action.remote().repository_id(),
            InstallationPermissions::delivery(),
            chrono::Utc::now().timestamp(),
        )
        .await
        .map_err(|_| MutationError::MissingToken)?;
    if token.installation_id() != preview.action.remote().installation_id() {
        return Err(MutationError::InvalidTarget);
    }
    if let GitHubAction::PushIntent { branch, head_sha } = preview.action.action() {
        let (repository_path, state_root, default_branch, clone_url) = {
            let store = store
                .lock()
                .map_err(|_| MutationError::GitTransportFailed)?;
            let task = store
                .load_task(preview.task_id)
                .map_err(|_| MutationError::GitTransportFailed)?
                .ok_or(MutationError::InvalidTarget)?;
            let binding_id = task
                .repository_binding_id
                .ok_or(MutationError::InvalidTarget)?;
            let binding = store
                .repository_binding(binding_id)
                .map_err(|_| MutationError::GitTransportFailed)?
                .ok_or(MutationError::InvalidTarget)?;
            (
                task.repository_path,
                binding.state_root().to_owned(),
                binding.default_branch().to_owned(),
                binding.clone_url().to_owned(),
            )
        };
        if branch == &default_branch {
            return Err(MutationError::DefaultBranchPushProhibited);
        }
        crate::GitTransport::push_branch(
            Path::new(&repository_path),
            branch,
            head_sha,
            &clone_url,
            Path::new(&state_root),
            token.expose_for_authorization_header(),
        )
        .map_err(|_| MutationError::GitTransportFailed)?;
        return Ok(patchwright_relay::MutationResult {
            sha: Some(head_sha.clone()),
            ..Default::default()
        });
    }
    GitHubMutationClient::new(&api_url, token.expose_for_authorization_header())?
        .execute(owner, repository, preview.action.action())
        .await
}

fn github_cli_path() -> String {
    std::env::var("PATCHWRIGHT_GH_PATH").unwrap_or_else(|_| {
        if std::path::Path::new("/opt/homebrew/bin/gh").is_file() {
            "/opt/homebrew/bin/gh".into()
        } else {
            "gh".into()
        }
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GitHubAppRuntimeConfiguration {
    app_id: u64,
    client_id: String,
    key_reference: String,
    #[serde(default = "default_github_api_url")]
    api_base_url: String,
}

fn github_app_runtime_configuration() -> Option<GitHubAppRuntimeConfiguration> {
    let environment = (
        std::env::var("PATCHWRIGHT_GITHUB_APP_ID").ok(),
        std::env::var("PATCHWRIGHT_GITHUB_APP_CLIENT_ID").ok(),
        std::env::var("PATCHWRIGHT_GITHUB_APP_KEY_REFERENCE").ok(),
    );
    if let (Some(app_id), Some(client_id), Some(key_reference)) = environment {
        return Some(GitHubAppRuntimeConfiguration {
            app_id: app_id.parse().ok()?,
            client_id,
            key_reference,
            api_base_url: std::env::var("PATCHWRIGHT_GITHUB_API_URL")
                .unwrap_or_else(|_| default_github_api_url()),
        });
    }
    let home = std::env::var_os("HOME")?;
    let path = std::path::PathBuf::from(home).join(".patchwright/github-app.json");
    load_github_app_runtime_configuration(&path)
}

fn load_github_app_runtime_configuration(
    path: &std::path::Path,
) -> Option<GitHubAppRuntimeConfiguration> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.file_type().is_symlink() || metadata.permissions().mode() & 0o077 != 0 {
            return None;
        }
    }
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

fn default_github_api_url() -> String {
    "https://api.github.com".into()
}

fn encoded_parameter<T: DeserializeOwned>(
    params: &Value,
    key: &str,
) -> std::result::Result<T, String> {
    let value = params
        .get(key)
        .ok_or_else(|| format!("{key} is required"))?;
    if let Some(encoded) = value.as_str() {
        serde_json::from_str(encoded).map_err(|_| format!("{key} is invalid"))
    } else {
        serde_json::from_value(value.clone()).map_err(|_| format!("{key} is invalid"))
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

fn positive_u64(params: &Value, key: &str) -> Option<u64> {
    params
        .get(key)
        .and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_str().and_then(|value| value.parse().ok()))
        })
        .filter(|value| *value > 0)
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
                    .filter_map(|value| serde_json::from_str::<Task>(&value).ok())
                    .collect::<Vec<_>>()
            ),
        ),
        Err(error) => rpc_error(id, -32000, "persistence failure", Some(error.to_string())),
    }
}

fn required_task_id(params: &Value) -> std::result::Result<TaskId, String> {
    params
        .get("taskId")
        .and_then(Value::as_str)
        .and_then(|value| TaskId::from_str(value).ok())
        .ok_or_else(|| "taskId must be a UUID".to_owned())
}

fn task_worktree(id: Value, params: &Value, store: &Mutex<EventStore>) -> Value {
    let task_id = match required_task_id(params) {
        Ok(task_id) => task_id,
        Err(detail) => return rpc_error(id, -32602, "invalid parameters", Some(detail)),
    };
    let task = match store
        .lock()
        .expect("event store lock poisoned")
        .load_task(task_id)
    {
        Ok(Some(task)) => task,
        Ok(None) => return rpc_error(id, -32040, "task is missing", None),
        Err(error) => return rpc_error(id, -32000, "persistence failure", Some(error.to_string())),
    };
    match crate::RepositoryService::inspect(Path::new(&task.repository_path)) {
        Ok(inspection) => rpc_result(id, json!(inspection)),
        Err(error) => rpc_error(
            id,
            -32044,
            "task worktree is unavailable",
            Some(error.to_string()),
        ),
    }
}

async fn task_plan(id: Value, params: &Value, store: &Mutex<EventStore>) -> Value {
    let task_id = match required_task_id(params) {
        Ok(task_id) => task_id,
        Err(detail) => return rpc_error(id, -32602, "invalid parameters", Some(detail)),
    };
    let (mut task, binding) = {
        let store = store.lock().expect("event store lock poisoned");
        let task = match store.load_task(task_id) {
            Ok(Some(task)) => task,
            Ok(None) => return rpc_error(id, -32040, "task is missing", None),
            Err(error) => {
                return rpc_error(id, -32000, "persistence failure", Some(error.to_string()));
            }
        };
        if task.state == TaskState::AwaitingPreparationApproval {
            return match store.task_contract(task_id) {
                Ok(Some(_)) => rpc_result(id, json!(task)),
                Ok(None) => rpc_error(id, -32045, "task contract is missing", None),
                Err(error) => rpc_error(id, -32000, "persistence failure", Some(error.to_string())),
            };
        }
        if task.state != TaskState::Discovered {
            return rpc_error(
                id,
                -32041,
                "task cannot be planned from its current state",
                Some(task.state.to_string()),
            );
        }
        let Some(binding_id) = task.repository_binding_id else {
            return rpc_error(
                id,
                -32041,
                "task planning failed",
                Some("task repository binding is missing".into()),
            );
        };
        let binding = match store.repository_binding(binding_id) {
            Ok(Some(binding)) => binding,
            Ok(None) => {
                return rpc_error(
                    id,
                    -32041,
                    "task planning failed",
                    Some("task repository binding is missing".into()),
                );
            }
            Err(error) => {
                return rpc_error(id, -32000, "persistence failure", Some(error.to_string()));
            }
        };
        (task, binding)
    };
    if let Err(error) = ensure_planning_repository(&task, &binding).await {
        return rpc_error(id, -32041, "task planning failed", Some(error));
    }
    let contract = match RepositoryPlanner::assess(&task, &binding) {
        Ok(contract) => contract,
        Err(error) => {
            return rpc_error(id, -32041, "task planning failed", Some(error.to_string()));
        }
    };
    let store = store.lock().expect("event store lock poisoned");
    match store.load_task(task_id) {
        Ok(Some(fresh)) if fresh == task => {}
        Ok(Some(fresh)) => {
            return rpc_error(
                id,
                -32041,
                "task changed while planning",
                Some(fresh.state.to_string()),
            );
        }
        Ok(None) => return rpc_error(id, -32040, "task is missing", None),
        Err(error) => return rpc_error(id, -32000, "persistence failure", Some(error.to_string())),
    }
    task.contract_version = contract.version();
    if let Err(error) = store.save_task_contract(&contract) {
        return rpc_error(id, -32000, "persistence failure", Some(error.to_string()));
    }
    for (next, summary) in [
        (TaskState::Assessing, "Task source and repository assessed"),
        (TaskState::Planned, "Typed task contract planned"),
        (
            TaskState::AwaitingPreparationApproval,
            "Task is awaiting preparation approval",
        ),
    ] {
        if let Err(error) = task.transition(next) {
            return rpc_error(id, -32041, "task planning failed", Some(error.to_string()));
        }
        if let Err(error) = store.save_task(&task, summary) {
            return rpc_error(id, -32000, "persistence failure", Some(error.to_string()));
        }
    }
    rpc_result(id, json!(task))
}

async fn ensure_planning_repository(
    task: &Task,
    binding: &RepositoryBinding,
) -> std::result::Result<(), String> {
    let repository = Path::new(&task.repository_path);
    if crate::RepositoryService::inspect(repository).is_ok() {
        return Ok(());
    }
    if repository.exists() || binding.managed_clone() != Some(task.repository_path.as_str()) {
        return Err("bound repository is unavailable or invalid".into());
    }
    let token = github_app_installation_token(
        binding.full_name(),
        binding.github_repository_id(),
        InstallationPermissions::ingestion(),
    )
    .await
    .map_err(|error| format!("managed repository authentication failed: {error}"))?;
    if token.installation_id() != binding.installation_id() {
        return Err("GitHub App installation identity mismatch".into());
    }
    crate::GitTransport::clone_repository(
        binding.clone_url(),
        binding.full_name(),
        repository,
        Path::new(binding.state_root()),
        token.expose_for_authorization_header(),
    )
    .map_err(|error| format!("managed repository clone failed: {error}"))
}

fn task_contract(id: Value, params: &Value, store: &Mutex<EventStore>) -> Value {
    let task_id = match required_task_id(params) {
        Ok(task_id) => task_id,
        Err(detail) => return rpc_error(id, -32602, "invalid parameters", Some(detail)),
    };
    match store
        .lock()
        .expect("event store lock poisoned")
        .task_contract(task_id)
    {
        Ok(Some(contract)) => rpc_result(id, json!(contract)),
        Ok(None) => rpc_error(id, -32045, "task contract is missing", None),
        Err(error) => rpc_error(id, -32000, "persistence failure", Some(error.to_string())),
    }
}

fn task_preparation_preview(id: Value, params: &Value, store: &Mutex<EventStore>) -> Value {
    let task_id = match required_task_id(params) {
        Ok(task_id) => task_id,
        Err(detail) => return rpc_error(id, -32602, "invalid parameters", Some(detail)),
    };
    match preview_preparation(&store.lock().expect("event store lock poisoned"), task_id) {
        Ok(preview) => rpc_result(id, json!(preview)),
        Err(error) => rpc_error(
            id,
            -32042,
            "preparation preview failed",
            Some(error.to_string()),
        ),
    }
}

fn task_preparation_approve(id: Value, params: &Value, store: &Mutex<EventStore>) -> Value {
    let preview = match encoded_parameter(params, "preview") {
        Ok(preview) => preview,
        Err(detail) => return rpc_error(id, -32602, "invalid parameters", Some(detail)),
    };
    let approved_by = params
        .get("approvedBy")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match approve_preparation(
        &store.lock().expect("event store lock poisoned"),
        &preview,
        approved_by,
    ) {
        Ok(approval) => rpc_result(id, json!(approval)),
        Err(error) => rpc_error(
            id,
            -32045,
            "preparation approval failed",
            Some(error.to_string()),
        ),
    }
}

#[allow(clippy::too_many_lines)]
async fn task_prepare(id: Value, params: &Value, store: &Mutex<EventStore>) -> Value {
    let task_id = match required_task_id(params) {
        Ok(task_id) => task_id,
        Err(detail) => return rpc_error(id, -32602, "invalid parameters", Some(detail)),
    };
    let preview: crate::PreparationPreview = match encoded_parameter(params, "preview") {
        Ok(preview) => preview,
        Err(detail) => return rpc_error(id, -32602, "invalid parameters", Some(detail)),
    };
    if preview.task_id != task_id {
        return rpc_error(
            id,
            -32602,
            "invalid parameters",
            Some("preview taskId does not match taskId".into()),
        );
    }
    let Some(approval_id) = params
        .get("approvalId")
        .and_then(Value::as_str)
        .and_then(|value| uuid::Uuid::parse_str(value).ok())
    else {
        return rpc_error(
            id,
            -32602,
            "invalid parameters",
            Some("approvalId is required".into()),
        );
    };
    if let Err(error) = authorize_preparation(
        &store.lock().expect("event store lock poisoned"),
        &preview,
        approval_id,
    ) {
        return rpc_error(
            id,
            -32046,
            "preparation authorization failed",
            Some(error.to_string()),
        );
    }
    let response = task_prepare_claimed(id.clone(), task_id, &preview, store).await;
    let result = if response.get("error").is_some() {
        "failed"
    } else {
        "succeeded"
    };
    if let Err(error) = store
        .lock()
        .expect("event store lock poisoned")
        .complete_preparation_claim(approval_id, result)
    {
        return rpc_error(id, -32000, "persistence failure", Some(error.to_string()));
    }
    response
}

#[allow(clippy::too_many_lines)]
async fn task_prepare_claimed(
    id: Value,
    task_id: TaskId,
    preview: &crate::PreparationPreview,
    store: &Mutex<EventStore>,
) -> Value {
    let (mut task, binding) = {
        let store = store.lock().expect("event store lock poisoned");
        match preview_preparation(&store, task_id) {
            Ok(fresh) if fresh == *preview => {}
            Ok(_) => return rpc_error(id, -32046, "preparation preview is stale", None),
            Err(error) => {
                return rpc_error(
                    id,
                    -32046,
                    "preparation preview is stale",
                    Some(error.to_string()),
                );
            }
        }
        let task = match store.load_task(task_id) {
            Ok(Some(task)) => task,
            Ok(None) => return rpc_error(id, -32040, "task is missing", None),
            Err(error) => {
                return rpc_error(id, -32000, "persistence failure", Some(error.to_string()));
            }
        };
        if task.state != TaskState::AwaitingPreparationApproval {
            return rpc_error(
                id,
                -32042,
                "task is not awaiting preparation approval",
                Some(task.state.to_string()),
            );
        }
        let Some(binding_id) = task.repository_binding_id else {
            return rpc_error(id, -32042, "task repository binding is missing", None);
        };
        let binding = match store.repository_binding(binding_id) {
            Ok(Some(binding)) => binding,
            Ok(None) => return rpc_error(id, -32042, "task repository binding is missing", None),
            Err(error) => {
                return rpc_error(id, -32000, "persistence failure", Some(error.to_string()));
            }
        };
        (task, binding)
    };
    let repository = Path::new(&preview.repository_path);
    if !repository.is_dir() {
        let token = match github_app_installation_token(
            binding.full_name(),
            binding.github_repository_id(),
            InstallationPermissions::ingestion(),
        )
        .await
        {
            Ok(token) if token.installation_id() == binding.installation_id() => token,
            Ok(_) => {
                return rpc_error(
                    id,
                    -32043,
                    "GitHub App installation identity mismatch",
                    None,
                );
            }
            Err(error) => {
                return rpc_error(
                    id,
                    -32043,
                    "managed repository authentication failed",
                    Some(error.to_string()),
                );
            }
        };
        if let Err(error) = crate::GitTransport::clone_repository(
            binding.clone_url(),
            binding.full_name(),
            repository,
            Path::new(binding.state_root()),
            token.expose_for_authorization_header(),
        ) {
            return rpc_error(
                id,
                -32043,
                "managed repository clone failed",
                Some(error.to_string()),
            );
        }
    }
    let worktree = Path::new(&preview.worktree_path);
    let branch = &preview.branch;
    let start_sha = Some(preview.source_sha.as_str());
    if worktree.exists() {
        match crate::RepositoryService::inspect(worktree) {
            Ok(inspection)
                if inspection.branch == *branch
                    && start_sha.is_none_or(|expected| inspection.head_sha == expected) => {}
            Ok(_) => {
                return rpc_error(id, -32043, "existing task worktree does not match", None);
            }
            Err(error) => {
                return rpc_error(
                    id,
                    -32043,
                    "task worktree is invalid",
                    Some(error.to_string()),
                );
            }
        }
    } else if let Err(error) =
        crate::WorktreeService::prepare_at(repository, worktree, branch, start_sha)
    {
        return rpc_error(
            id,
            -32043,
            "task worktree preparation failed",
            Some(error.to_string()),
        );
    }
    task.repository_path.clone_from(&preview.worktree_path);
    if let Err(error) = task.transition(TaskState::Preparing) {
        return rpc_error(
            id,
            -32042,
            "task preparation failed",
            Some(error.to_string()),
        );
    }
    let checkpoint = match crate::TaskCheckpoint::new(
        task.id,
        task.state,
        "Approved isolated worktree prepared at captured source SHA",
    ) {
        Ok(checkpoint) => checkpoint,
        Err(error) => {
            return rpc_error(
                id,
                -32042,
                "task preparation failed",
                Some(error.to_string()),
            );
        }
    };
    task.checkpoint_id = Some(checkpoint.id());
    if let Err(error) = store
        .lock()
        .expect("event store lock poisoned")
        .save_task_with_checkpoint(&task, "Task preparation approved", &checkpoint)
    {
        return rpc_error(id, -32000, "persistence failure", Some(error.to_string()));
    }
    rpc_result(id, json!(task))
}

async fn task_ready_for_delivery(id: Value, params: &Value, store: &Mutex<EventStore>) -> Value {
    let task_id = match required_task_id(params) {
        Ok(task_id) => task_id,
        Err(detail) => return rpc_error(id, -32602, "invalid parameters", Some(detail)),
    };
    match crate::verify_task_for_delivery(task_id, store).await {
        Ok(task) => rpc_result(id, json!(task)),
        Err(error) => rpc_error(id, -32045, "verification failed", Some(error.to_string())),
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
    let gh_path = github_cli_path();
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
    let gh_path = github_cli_path();
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

async fn sync_github_repository(id: Value, params: &Value, store: &Mutex<EventStore>) -> Value {
    let request = match repository_sync_parameters(params) {
        Ok(request) => request,
        Err(detail) => return rpc_error(id, -32602, "invalid parameters", Some(detail)),
    };
    let (source, installation_id) = match github_app_source_for_repository(
        &request.full_name,
        request.repository_id,
        request.expected_installation_id,
    )
    .await
    {
        Ok(source) => source,
        Err(error) => {
            return rpc_error(
                id,
                -32010,
                "GitHub App repository authentication unavailable",
                Some(error.to_string()),
            );
        }
    };
    let mut repository = match source.repository(&request.full_name).await {
        Ok(repository) if repository.id == request.repository_id => repository,
        Ok(_) => {
            return rpc_error(id, -32014, "GitHub repository identity mismatch", None);
        }
        Err(error) => {
            return rpc_error(
                id,
                -32013,
                "GitHub repository lookup failed",
                Some(error.to_string()),
            );
        }
    };
    repository.installation_id = Some(installation_id);
    let resource_limit = bounded_limit(params, "resourceLimit", 1000, 1000);
    let snapshot = match source
        .repository_snapshot(&repository, resource_limit)
        .await
    {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return rpc_error(
                id,
                -32015,
                "GitHub repository synchronization failed",
                Some(error.to_string()),
            );
        }
    };
    if let Err(error) = store
        .lock()
        .expect("event store lock poisoned")
        .replace_github_snapshot(&snapshot)
    {
        return rpc_error(id, -32000, "persistence failure", Some(error.to_string()));
    }
    rpc_result(id, json!(snapshot))
}

async fn task_reconcile_github(id: Value, params: &Value, store: &Mutex<EventStore>) -> Value {
    let task_id = match required_task_id(params) {
        Ok(task_id) => task_id,
        Err(detail) => return rpc_error(id, -32602, "invalid parameters", Some(detail)),
    };
    let (full_name, repository_id, installation_id) = {
        let store = store.lock().expect("event store lock poisoned");
        let task = match store.load_task(task_id) {
            Ok(Some(task)) => task,
            Ok(None) => return rpc_error(id, -32040, "task is missing", None),
            Err(error) => {
                return rpc_error(id, -32000, "persistence failure", Some(error.to_string()));
            }
        };
        let Some(full_name) = task.source.repository_full_name().map(ToOwned::to_owned) else {
            return rpc_error(id, -32045, "task is not backed by GitHub", None);
        };
        let Some(repository_id) = task.source.repository_id() else {
            return rpc_error(id, -32045, "task is not backed by GitHub", None);
        };
        let Some(binding_id) = task.repository_binding_id else {
            return rpc_error(id, -32033, "task repository binding is missing", None);
        };
        let binding = match store.repository_binding(binding_id) {
            Ok(Some(binding)) => binding,
            Ok(None) => return rpc_error(id, -32033, "task repository binding is missing", None),
            Err(error) => {
                return rpc_error(id, -32000, "persistence failure", Some(error.to_string()));
            }
        };
        if binding.github_repository_id() != repository_id || binding.full_name() != full_name {
            return rpc_error(id, -32045, "task repository identity changed", None);
        }
        (full_name, repository_id, binding.installation_id())
    };
    let (source, _) =
        match github_app_source_for_repository(&full_name, repository_id, Some(installation_id))
            .await
        {
            Ok(source) => source,
            Err(error) => {
                return rpc_error(
                    id,
                    -32010,
                    "GitHub App repository authentication unavailable",
                    Some(error.to_string()),
                );
            }
        };
    let mut repository = match source.repository(&full_name).await {
        Ok(repository) if repository.id == repository_id => repository,
        Ok(_) => return rpc_error(id, -32014, "GitHub repository identity mismatch", None),
        Err(error) => {
            return rpc_error(
                id,
                -32013,
                "GitHub repository lookup failed",
                Some(error.to_string()),
            );
        }
    };
    repository.installation_id = Some(installation_id);
    let snapshot = match source.repository_snapshot(&repository, 1_000).await {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return rpc_error(
                id,
                -32015,
                "GitHub task reconciliation refresh failed",
                Some(error.to_string()),
            );
        }
    };
    let result = {
        let store = store.lock().expect("event store lock poisoned");
        if let Err(error) = store.replace_github_snapshot(&snapshot) {
            return rpc_error(id, -32000, "persistence failure", Some(error.to_string()));
        }
        crate::reconcile_completed_task_from_snapshot(&store, task_id, &snapshot)
    };
    match result {
        Ok(task) => rpc_result(id, json!(task)),
        Err(error) => rpc_error(
            id,
            -32046,
            "GitHub does not confirm this exact task is complete",
            Some(error.to_string()),
        ),
    }
}

struct RepositorySyncParameters {
    full_name: String,
    repository_id: u64,
    expected_installation_id: Option<u64>,
}

fn repository_sync_parameters(
    params: &Value,
) -> std::result::Result<RepositorySyncParameters, String> {
    let full_name =
        required_string(params, "fullName").ok_or_else(|| "fullName is required".to_owned())?;
    let repository_id = positive_u64(params, "repositoryId")
        .ok_or_else(|| "repositoryId must be a positive integer".to_owned())?;
    let expected_installation_id = if params.get("installationId").is_some() {
        Some(
            positive_u64(params, "installationId")
                .ok_or_else(|| "installationId must be a positive integer".to_owned())?,
        )
    } else {
        None
    };
    Ok(RepositorySyncParameters {
        full_name,
        repository_id,
        expected_installation_id,
    })
}

async fn github_app_source_for_repository(
    full_name: &str,
    repository_id: u64,
    expected_installation_id: Option<u64>,
) -> Result<(GitHubSource, u64)> {
    let token = github_app_installation_token(
        full_name,
        repository_id,
        InstallationPermissions::ingestion(),
    )
    .await?;
    let installation_id = token.installation_id();
    if expected_installation_id.is_some_and(|expected| expected != installation_id) {
        bail!("GitHub App installation identity mismatch");
    }
    let runtime = github_app_runtime_configuration()
        .context("GitHub App runtime configuration is unavailable")?;
    let source = GitHubSource::new(
        &runtime.api_base_url,
        GitHubToken::new(token.expose_for_authorization_header()),
    )
    .context("GitHub source is unavailable")?;
    Ok((source, installation_id))
}

async fn github_app_installation_token(
    full_name: &str,
    repository_id: u64,
    permissions: InstallationPermissions,
) -> Result<InstallationToken> {
    let (owner, repository_name) = full_name
        .split_once('/')
        .context("repository full name lacks owner")?;
    if owner.is_empty() || repository_name.is_empty() || repository_name.contains('/') {
        bail!("repository full name is invalid");
    }
    let runtime = github_app_runtime_configuration()
        .context("GitHub App runtime configuration is unavailable")?;
    let key_reference = KeyReference::parse(&runtime.key_reference)
        .context("GitHub App key reference is invalid")?;
    let configuration = GitHubAppConfiguration::new(
        runtime.app_id,
        runtime.client_id,
        key_reference,
        runtime.api_base_url.clone(),
    )
    .context("GitHub App configuration is invalid")?;
    let authenticator = AppAuthenticator::new(configuration, ConfiguredKeyProvider)
        .context("GitHub App authenticator is unavailable")?;
    let broker = InstallationBroker::new(authenticator, &runtime.api_base_url)
        .context("GitHub App installation broker is unavailable")?;
    let token = broker
        .token_for_repository(
            owner,
            repository_name,
            repository_id,
            permissions,
            chrono::Utc::now().timestamp(),
        )
        .await
        .context("GitHub App installation token is unavailable")?;
    Ok(token)
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

#[cfg(test)]
mod tests {
    use super::{load_github_app_runtime_configuration, repository_sync_parameters};
    use serde_json::json;
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[test]
    fn github_app_file_requires_owner_only_regular_file() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let configuration = directory.path().join("github-app.json");
        std::fs::write(
            &configuration,
            r#"{"appId":123,"clientId":"client","keyReference":"keychain:service/account"}"#,
        )
        .expect("write configuration");
        std::fs::set_permissions(&configuration, std::fs::Permissions::from_mode(0o600))
            .expect("secure permissions");

        let loaded = load_github_app_runtime_configuration(&configuration)
            .expect("secure configuration should load");
        assert_eq!(loaded.app_id, 123);
        assert_eq!(loaded.api_base_url, "https://api.github.com");

        std::fs::set_permissions(&configuration, std::fs::Permissions::from_mode(0o644))
            .expect("permissive permissions");
        assert!(load_github_app_runtime_configuration(&configuration).is_none());

        std::fs::set_permissions(&configuration, std::fs::Permissions::from_mode(0o600))
            .expect("restore secure permissions");
        let linked = directory.path().join("linked.json");
        symlink(&configuration, &linked).expect("create symlink");
        assert!(load_github_app_runtime_configuration(&linked).is_none());
    }

    #[test]
    fn repository_sync_can_discover_or_verify_an_installation() {
        let discovered = repository_sync_parameters(&json!({
            "fullName": "acme/widget",
            "repositoryId": "42"
        }))
        .expect("installation discovery should be supported");
        assert_eq!(discovered.full_name, "acme/widget");
        assert_eq!(discovered.repository_id, 42);
        assert_eq!(discovered.expected_installation_id, None);

        let verified = repository_sync_parameters(&json!({
            "fullName": "acme/widget",
            "repositoryId": "42",
            "installationId": "99"
        }))
        .expect("known installation should remain verifiable");
        assert_eq!(verified.expected_installation_id, Some(99));

        assert!(
            repository_sync_parameters(&json!({
                "fullName": "acme/widget",
                "repositoryId": "0"
            }))
            .is_err()
        );
    }
}
