use axum::{
    Router,
    body::Bytes,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use hmac::{Hmac, Mac};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::{
    fs::{self, OpenOptions},
    future::Future,
    os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};

const MAX_BODY_BYTES: usize = 1024 * 1024;
const MAX_RPC_FRAME_BYTES: usize = 1024 * 1024;
const DEFAULT_QUEUE_CAPACITY: usize = 10_000;
const FORWARD_BATCH_SIZE: usize = 32;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const RPC_TIMEOUT: Duration = Duration::from_secs(5);
const INITIAL_RETRY_MILLIS: i64 = 200;
const MAX_RETRY_MILLIS: i64 = 60_000;

#[derive(Clone)]
pub struct RelayState {
    secret: Arc<Vec<u8>>,
    inbox: Arc<Mutex<Connection>>,
    queue_capacity: usize,
}

impl RelayState {
    #[must_use]
    pub fn new(secret: Vec<u8>) -> Self {
        let connection = Connection::open_in_memory().expect("open in-memory relay inbox");
        initialize_inbox(&connection).expect("initialize in-memory relay inbox");
        Self {
            secret: Arc::new(secret),
            inbox: Arc::new(Mutex::new(connection)),
            queue_capacity: DEFAULT_QUEUE_CAPACITY,
        }
    }

    /// Opens a durable webhook inbox at an owner-only local path.
    ///
    /// # Errors
    ///
    /// Returns an error when the secret is empty, the path is not a secure
    /// regular file in an owner-only directory, or `SQLite` cannot initialize.
    pub fn open(secret: Vec<u8>, database: impl AsRef<Path>) -> anyhow::Result<Self> {
        anyhow::ensure!(!secret.is_empty(), "webhook secret must not be empty");
        let database = secure_database_path(database.as_ref())?;
        let connection = Connection::open(&database)?;
        initialize_inbox(&connection)?;
        verify_owner_only_file(&database)?;
        Ok(Self {
            secret: Arc::new(secret),
            inbox: Arc::new(Mutex::new(connection)),
            queue_capacity: DEFAULT_QUEUE_CAPACITY,
        })
    }

    #[must_use]
    pub fn delivery_count(&self) -> usize {
        self.inbox
            .lock()
            .expect("relay inbox lock poisoned")
            .query_row("SELECT COUNT(*) FROM webhook_deliveries", [], |row| {
                row.get(0)
            })
            .expect("count relay inbox deliveries")
    }

    /// Returns the number of accepted deliveries that the engine has not acknowledged.
    pub fn pending_delivery_count(&self) -> anyhow::Result<usize> {
        let connection = self
            .inbox
            .lock()
            .map_err(|_| anyhow::anyhow!("relay inbox lock poisoned"))?;
        connection
            .query_row(
                "SELECT COUNT(*) FROM webhook_deliveries WHERE forwarded_at IS NULL",
                [],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    fn record(&self, delivery: &WebhookDelivery) -> anyhow::Result<RecordOutcome> {
        let mut connection = self
            .inbox
            .lock()
            .map_err(|_| anyhow::anyhow!("relay inbox lock poisoned"))?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM webhook_deliveries WHERE delivery_id = ?1)",
            [&delivery.delivery_id],
            |row| row.get(0),
        )?;
        if existing {
            transaction.rollback()?;
            return Ok(RecordOutcome::Duplicate);
        }
        let pending: usize = transaction.query_row(
            "SELECT COUNT(*) FROM webhook_deliveries WHERE forwarded_at IS NULL",
            [],
            |row| row.get(0),
        )?;
        if pending >= self.queue_capacity {
            transaction.rollback()?;
            return Ok(RecordOutcome::Full);
        }
        transaction.execute(
            "INSERT OR IGNORE INTO webhook_deliveries
                (delivery_id, event, action, payload, payload_sha256, received_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                delivery.delivery_id,
                delivery.event,
                delivery.action,
                delivery.payload,
                delivery.payload_sha256,
                chrono::Utc::now().to_rfc3339(),
            ],
        )?;
        transaction.commit()?;
        Ok(RecordOutcome::Accepted)
    }

    /// Attempts one bounded batch of due outbox entries.
    ///
    /// Transport failures are converted into durable bounded backoff so a relay
    /// restart continues retrying without requiring GitHub to redeliver.
    pub async fn forward_pending_once(
        &self,
        engine_socket: &Path,
    ) -> anyhow::Result<ForwardSummary> {
        let state = self.clone();
        let deliveries = tokio::task::spawn_blocking(move || state.due_deliveries())
            .await
            .map_err(|error| anyhow::anyhow!("relay outbox worker failed: {error}"))??;
        let mut summary = ForwardSummary {
            attempted: deliveries.len(),
            forwarded: 0,
        };
        for delivery in deliveries {
            match forward_delivery(engine_socket, &delivery).await {
                Ok(()) => {
                    let state = self.clone();
                    let delivery_id = delivery.delivery_id.clone();
                    tokio::task::spawn_blocking(move || state.mark_forwarded(&delivery_id))
                        .await
                        .map_err(|error| {
                            anyhow::anyhow!("relay outbox worker failed: {error}")
                        })??;
                    summary.forwarded += 1;
                }
                Err(error) => {
                    tracing::warn!(error = %error, delivery_id = %delivery.delivery_id, "engine webhook forwarding deferred");
                    let state = self.clone();
                    let delivery_id = delivery.delivery_id.clone();
                    tokio::task::spawn_blocking(move || state.defer_delivery(&delivery_id))
                        .await
                        .map_err(|error| {
                            anyhow::anyhow!("relay outbox worker failed: {error}")
                        })??;
                }
            }
        }
        Ok(summary)
    }

    /// Runs the durable forwarder until shutdown, with a bounded polling interval.
    pub async fn run_forwarder_until<F>(
        &self,
        engine_socket: &Path,
        shutdown: F,
    ) -> anyhow::Result<()>
    where
        F: Future<Output = ()> + Send,
    {
        tokio::pin!(shutdown);
        loop {
            tokio::select! {
                () = &mut shutdown => return Ok(()),
                result = self.forward_pending_once(engine_socket) => {
                    result?;
                }
            }
            tokio::select! {
                () = &mut shutdown => return Ok(()),
                () = tokio::time::sleep(Duration::from_millis(200)) => {}
            }
        }
    }

    fn due_deliveries(&self) -> anyhow::Result<Vec<ForwardedWebhook>> {
        let connection = self
            .inbox
            .lock()
            .map_err(|_| anyhow::anyhow!("relay inbox lock poisoned"))?;
        let mut statement = connection.prepare(
            "SELECT delivery_id, payload, payload_sha256, received_at
             FROM webhook_deliveries
             WHERE forwarded_at IS NULL
               AND (next_attempt_at IS NULL OR julianday(next_attempt_at) <= julianday(?1))
             ORDER BY received_at, delivery_id
             LIMIT ?2",
        )?;
        let rows = statement.query_map(
            params![
                chrono::Utc::now().to_rfc3339(),
                i64::try_from(FORWARD_BATCH_SIZE).expect("batch size fits in i64")
            ],
            |row| {
                let payload: Vec<u8> = row.get(1)?;
                let envelope = serde_json::from_slice(&payload).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        payload.len(),
                        rusqlite::types::Type::Blob,
                        Box::new(error),
                    )
                })?;
                Ok(ForwardedWebhook {
                    delivery_id: row.get(0)?,
                    envelope,
                    payload_sha256: row.get(2)?,
                    received_at: row.get(3)?,
                })
            },
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn mark_forwarded(&self, delivery_id: &str) -> anyhow::Result<()> {
        let connection = self
            .inbox
            .lock()
            .map_err(|_| anyhow::anyhow!("relay inbox lock poisoned"))?;
        let updated = connection.execute(
            "UPDATE webhook_deliveries
             SET forwarded_at = ?2, last_error = NULL
             WHERE delivery_id = ?1 AND forwarded_at IS NULL",
            params![delivery_id, chrono::Utc::now().to_rfc3339()],
        )?;
        anyhow::ensure!(updated == 1, "pending relay delivery disappeared");
        Ok(())
    }

    fn defer_delivery(&self, delivery_id: &str) -> anyhow::Result<()> {
        let connection = self
            .inbox
            .lock()
            .map_err(|_| anyhow::anyhow!("relay inbox lock poisoned"))?;
        let attempts: u32 = connection
            .query_row(
                "SELECT attempt_count FROM webhook_deliveries
                 WHERE delivery_id = ?1 AND forwarded_at IS NULL",
                [delivery_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| anyhow::anyhow!("pending relay delivery disappeared"))?;
        let exponent = attempts.min(8);
        let backoff = (INITIAL_RETRY_MILLIS * (1_i64 << exponent)).min(MAX_RETRY_MILLIS);
        let next_attempt = chrono::Utc::now() + chrono::Duration::milliseconds(backoff);
        connection.execute(
            "UPDATE webhook_deliveries
             SET attempt_count = attempt_count + 1,
                 next_attempt_at = ?2,
                 last_error = 'engine unavailable'
             WHERE delivery_id = ?1 AND forwarded_at IS NULL",
            params![delivery_id, next_attempt.to_rfc3339()],
        )?;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SanitizedWebhookEnvelope {
    pub schema_version: u32,
    pub event: String,
    pub action: String,
    pub repository_id: Option<u64>,
    pub repository_full_name: Option<String>,
    pub entity_number: Option<u64>,
    pub entity_id: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ForwardedWebhook {
    pub delivery_id: String,
    pub envelope: SanitizedWebhookEnvelope,
    pub payload_sha256: String,
    pub received_at: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ForwardSummary {
    pub attempted: usize,
    pub forwarded: usize,
}

struct WebhookDelivery {
    delivery_id: String,
    event: String,
    action: String,
    payload: Vec<u8>,
    payload_sha256: String,
}

#[derive(Debug, Eq, PartialEq)]
enum RecordOutcome {
    Accepted,
    Duplicate,
    Full,
}

#[cfg(test)]
mod tests {
    use super::{RecordOutcome, RelayState, WebhookDelivery};

    fn delivery(id: &str) -> WebhookDelivery {
        WebhookDelivery {
            delivery_id: id.to_owned(),
            event: "pull_request".to_owned(),
            action: "opened".to_owned(),
            payload: br#"{"schemaVersion":1,"event":"pull_request","action":"opened","repositoryId":1,"repositoryFullName":"octocat/example","entityNumber":42,"entityId":null}"#.to_vec(),
            payload_sha256: "a".repeat(64),
        }
    }

    #[test]
    fn durable_outbox_rejects_new_deliveries_at_its_bound_but_keeps_deduplication() {
        let mut state = RelayState::new(b"secret".to_vec());
        state.queue_capacity = 1;
        assert_eq!(
            state.record(&delivery("one")).unwrap(),
            RecordOutcome::Accepted
        );
        assert_eq!(state.record(&delivery("two")).unwrap(), RecordOutcome::Full);
        assert_eq!(
            state.record(&delivery("one")).unwrap(),
            RecordOutcome::Duplicate
        );
    }
}

pub fn router(state: RelayState) -> Router {
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/webhooks/github", post(github_webhook))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .with_state(state)
}

async fn github_webhook(
    State(state): State<RelayState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let signature = headers
        .get("x-hub-signature-256")
        .and_then(|value| value.to_str().ok());
    if !signature.is_some_and(|value| verify_signature(&state.secret, &body, value)) {
        return (StatusCode::UNAUTHORIZED, "invalid signature");
    }
    let delivery = match headers
        .get("x-github-delivery")
        .and_then(|value| value.to_str().ok())
    {
        Some(value)
            if !value.is_empty()
                && value.len() <= 128
                && value.chars().all(|character| !character.is_control()) =>
        {
            value.to_owned()
        }
        _ => return (StatusCode::BAD_REQUEST, "missing delivery id"),
    };
    let event = match headers
        .get("x-github-event")
        .and_then(|value| value.to_str().ok())
    {
        Some(value) if !value.is_empty() && value.len() <= 64 => value.to_owned(),
        _ => return (StatusCode::BAD_REQUEST, "missing event name"),
    };
    let action = match validate_event(&event, &body) {
        Ok(action) => action,
        Err(EventValidationError::InvalidJson) => {
            return (StatusCode::BAD_REQUEST, "invalid json");
        }
        Err(EventValidationError::UnsupportedOrIncomplete) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                "unsupported or incomplete event",
            );
        }
    };

    let sanitized_payload = match sanitized_payload(&event, &action, &body) {
        Ok(payload) => payload,
        Err(error) => {
            tracing::error!(error = %error, "failed to sanitize verified webhook delivery");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "webhook inbox unavailable",
            );
        }
    };
    let delivery = WebhookDelivery {
        delivery_id: delivery,
        event,
        action,
        payload: sanitized_payload,
        payload_sha256: sha256_hex(&body),
    };
    match tokio::task::spawn_blocking(move || state.record(&delivery)).await {
        Ok(Ok(RecordOutcome::Accepted)) => (StatusCode::ACCEPTED, "accepted"),
        Ok(Ok(RecordOutcome::Duplicate)) => (StatusCode::OK, "duplicate"),
        Ok(Ok(RecordOutcome::Full)) => (StatusCode::SERVICE_UNAVAILABLE, "webhook inbox full"),
        Ok(Err(error)) => {
            tracing::error!(error = %error, "failed to commit verified webhook delivery");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "webhook inbox unavailable",
            )
        }
        Err(error) => {
            tracing::error!(error = %error, "webhook inbox worker failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "webhook inbox unavailable",
            )
        }
    }
}

fn sanitized_payload(event: &str, action: &str, body: &[u8]) -> anyhow::Result<Vec<u8>> {
    let value: serde_json::Value = serde_json::from_slice(body)?;
    let repository = value.get("repository");
    let (entity_number, entity_id) = match event {
        "pull_request" => (value.pointer("/pull_request/number"), None),
        "issue_comment" => (
            value
                .pointer("/issue/pull_request")
                .and(value.pointer("/issue/number")),
            value.pointer("/comment/id"),
        ),
        "pull_request_review" => (
            value.pointer("/pull_request/number"),
            value.pointer("/review/id"),
        ),
        "pull_request_review_comment" => (
            value.pointer("/pull_request/number"),
            value.pointer("/comment/id"),
        ),
        "check_run" => (None, value.pointer("/check_run/id")),
        "check_suite" => (None, value.pointer("/check_suite/id")),
        "workflow_run" => (None, value.pointer("/workflow_run/id")),
        "ping" => (None, value.get("hook_id")),
        _ => (None, None),
    };
    serde_json::to_vec(&SanitizedWebhookEnvelope {
        schema_version: 1,
        event: event.to_owned(),
        action: action.to_owned(),
        repository_id: repository
            .and_then(|item| item.get("id"))
            .and_then(serde_json::Value::as_u64),
        repository_full_name: repository
            .and_then(|item| item.get("full_name"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        entity_number: entity_number.and_then(serde_json::Value::as_u64),
        entity_id: entity_id.and_then(serde_json::Value::as_u64),
    })
    .map_err(Into::into)
}

async fn forward_delivery(engine_socket: &Path, delivery: &ForwardedWebhook) -> anyhow::Result<()> {
    verify_engine_socket(engine_socket)?;
    let stream = tokio::time::timeout(CONNECT_TIMEOUT, UnixStream::connect(engine_socket))
        .await
        .map_err(|_| anyhow::anyhow!("engine connection timed out"))??;
    let (peer_uid, _) = nix::unistd::getpeereid(&stream)?;
    anyhow::ensure!(
        peer_uid == nix::unistd::geteuid(),
        "engine RPC peer is not owned by the current user"
    );
    let request = serde_json::to_vec(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": delivery.delivery_id,
        "method": "github.webhook.ingest",
        "params": delivery,
    }))?;
    anyhow::ensure!(
        request.len() < MAX_RPC_FRAME_BYTES,
        "engine RPC frame is too large"
    );
    tokio::time::timeout(RPC_TIMEOUT, async move {
        let (reader, mut writer) = stream.into_split();
        writer.write_all(&request).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;
        let mut reader = BufReader::new(reader);
        let mut response = Vec::with_capacity(4096);
        let read = (&mut reader)
            .take((MAX_RPC_FRAME_BYTES + 1) as u64)
            .read_until(b'\n', &mut response)
            .await?;
        anyhow::ensure!(read > 0, "engine closed before acknowledging webhook");
        anyhow::ensure!(
            response.len() <= MAX_RPC_FRAME_BYTES && response.last() == Some(&b'\n'),
            "engine RPC response frame is invalid"
        );
        let value: serde_json::Value = serde_json::from_slice(&response)?;
        if let Some(error) = value.get("error") {
            anyhow::bail!("engine rejected webhook: {error}");
        }
        anyhow::ensure!(
            value.get("result").is_some(),
            "engine acknowledgement is missing"
        );
        Ok::<(), anyhow::Error>(())
    })
    .await
    .map_err(|_| anyhow::anyhow!("engine RPC timed out"))??;
    Ok(())
}

fn verify_engine_socket(path: &Path) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.file_type().is_socket()
            && !metadata.file_type().is_symlink()
            && metadata.uid() == u32::from(nix::unistd::geteuid())
            && is_owner_only(metadata.permissions().mode()),
        "engine socket must be owner-only and owned by the current user"
    );
    if let Some(parent) = path.parent() {
        verify_owner_only_directory(parent)?;
    }
    Ok(())
}

#[derive(Deserialize)]
struct PullRequestEvent {
    action: String,
    pull_request: NumberedObject,
    repository: RepositoryObject,
}

#[derive(Deserialize)]
struct IssueCommentEvent {
    action: String,
    issue: NumberedObject,
    comment: IdentifiedObject,
    repository: RepositoryObject,
}

#[derive(Deserialize)]
struct PullRequestReviewEvent {
    action: String,
    pull_request: NumberedObject,
    review: IdentifiedObject,
    repository: RepositoryObject,
}

#[derive(Deserialize)]
struct PullRequestReviewCommentEvent {
    action: String,
    pull_request: NumberedObject,
    comment: IdentifiedObject,
    repository: RepositoryObject,
}

#[derive(Deserialize)]
struct CheckRunEvent {
    action: String,
    check_run: IdentifiedObject,
    repository: RepositoryObject,
}

#[derive(Deserialize)]
struct CheckSuiteEvent {
    action: String,
    check_suite: IdentifiedObject,
    repository: RepositoryObject,
}

#[derive(Deserialize)]
struct WorkflowRunEvent {
    action: String,
    workflow_run: IdentifiedObject,
    repository: RepositoryObject,
}

#[derive(Deserialize)]
struct PingEvent {
    hook_id: u64,
    zen: String,
}

#[derive(Deserialize)]
struct NumberedObject {
    number: u64,
}

#[derive(Deserialize)]
struct IdentifiedObject {
    id: u64,
}

#[derive(Deserialize)]
struct RepositoryObject {
    id: u64,
    full_name: String,
}

enum EventValidationError {
    InvalidJson,
    UnsupportedOrIncomplete,
}

fn validate_event(event: &str, body: &[u8]) -> Result<String, EventValidationError> {
    let value: serde_json::Value =
        serde_json::from_slice(body).map_err(|_| EventValidationError::InvalidJson)?;
    match event {
        "pull_request" => {
            let payload: PullRequestEvent = typed_event(value)?;
            require_number(payload.pull_request.number)?;
            require_repository(&payload.repository)?;
            require_action(
                payload.action,
                &[
                    "assigned",
                    "unassigned",
                    "labeled",
                    "unlabeled",
                    "opened",
                    "edited",
                    "closed",
                    "reopened",
                    "synchronize",
                    "converted_to_draft",
                    "locked",
                    "unlocked",
                    "enqueued",
                    "dequeued",
                    "milestoned",
                    "demilestoned",
                    "ready_for_review",
                    "review_requested",
                    "review_request_removed",
                    "auto_merge_enabled",
                    "auto_merge_disabled",
                ],
            )
        }
        "issue_comment" => {
            let payload: IssueCommentEvent = typed_event(value)?;
            require_number(payload.issue.number)?;
            require_identifier(payload.comment.id)?;
            require_repository(&payload.repository)?;
            require_action(payload.action, &["created", "edited", "deleted"])
        }
        "pull_request_review" => {
            let payload: PullRequestReviewEvent = typed_event(value)?;
            require_number(payload.pull_request.number)?;
            require_identifier(payload.review.id)?;
            require_repository(&payload.repository)?;
            require_action(payload.action, &["submitted", "edited", "dismissed"])
        }
        "pull_request_review_comment" => {
            let payload: PullRequestReviewCommentEvent = typed_event(value)?;
            require_number(payload.pull_request.number)?;
            require_identifier(payload.comment.id)?;
            require_repository(&payload.repository)?;
            require_action(payload.action, &["created", "edited", "deleted"])
        }
        "check_run" => {
            let payload: CheckRunEvent = typed_event(value)?;
            require_identifier(payload.check_run.id)?;
            require_repository(&payload.repository)?;
            require_action(
                payload.action,
                &["created", "rerequested", "completed", "requested_action"],
            )
        }
        "check_suite" => {
            let payload: CheckSuiteEvent = typed_event(value)?;
            require_identifier(payload.check_suite.id)?;
            require_repository(&payload.repository)?;
            require_action(payload.action, &["completed", "requested", "rerequested"])
        }
        "workflow_run" => {
            let payload: WorkflowRunEvent = typed_event(value)?;
            require_identifier(payload.workflow_run.id)?;
            require_repository(&payload.repository)?;
            require_action(payload.action, &["completed", "requested", "in_progress"])
        }
        "ping" => {
            let payload: PingEvent = typed_event(value)?;
            require_identifier(payload.hook_id)?;
            if payload.zen.trim().is_empty() {
                return Err(EventValidationError::UnsupportedOrIncomplete);
            }
            Ok("ping".to_owned())
        }
        _ => Err(EventValidationError::UnsupportedOrIncomplete),
    }
}

fn typed_event<T: for<'de> Deserialize<'de>>(
    value: serde_json::Value,
) -> Result<T, EventValidationError> {
    serde_json::from_value(value).map_err(|_| EventValidationError::UnsupportedOrIncomplete)
}

fn require_action(action: String, supported: &[&str]) -> Result<String, EventValidationError> {
    if supported.contains(&action.as_str()) {
        Ok(action)
    } else {
        Err(EventValidationError::UnsupportedOrIncomplete)
    }
}

fn require_identifier(identifier: u64) -> Result<(), EventValidationError> {
    if identifier > 0 {
        Ok(())
    } else {
        Err(EventValidationError::UnsupportedOrIncomplete)
    }
}

fn require_number(number: u64) -> Result<(), EventValidationError> {
    require_identifier(number)
}

fn require_repository(repository: &RepositoryObject) -> Result<(), EventValidationError> {
    require_identifier(repository.id)?;
    let Some((owner, name)) = repository.full_name.split_once('/') else {
        return Err(EventValidationError::UnsupportedOrIncomplete);
    };
    if owner.is_empty()
        || name.is_empty()
        || name.contains('/')
        || repository.full_name.len() > 255
        || repository.full_name.chars().any(char::is_whitespace)
    {
        return Err(EventValidationError::UnsupportedOrIncomplete);
    }
    Ok(())
}

fn initialize_inbox(connection: &Connection) -> anyhow::Result<()> {
    connection.busy_timeout(std::time::Duration::from_secs(5))?;
    connection.execute_batch(
        "PRAGMA journal_mode = DELETE;
         PRAGMA synchronous = FULL;
         CREATE TABLE IF NOT EXISTS webhook_deliveries (
             delivery_id TEXT PRIMARY KEY NOT NULL,
             event TEXT NOT NULL,
             action TEXT NOT NULL,
             payload BLOB NOT NULL,
             payload_sha256 TEXT NOT NULL,
             received_at TEXT NOT NULL,
             attempt_count INTEGER NOT NULL DEFAULT 0,
             next_attempt_at TEXT,
             last_error TEXT,
             forwarded_at TEXT
         );",
    )?;
    ensure_column(
        connection,
        "attempt_count",
        "ALTER TABLE webhook_deliveries ADD COLUMN attempt_count INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        connection,
        "next_attempt_at",
        "ALTER TABLE webhook_deliveries ADD COLUMN next_attempt_at TEXT",
    )?;
    ensure_column(
        connection,
        "last_error",
        "ALTER TABLE webhook_deliveries ADD COLUMN last_error TEXT",
    )?;
    ensure_column(
        connection,
        "forwarded_at",
        "ALTER TABLE webhook_deliveries ADD COLUMN forwarded_at TEXT",
    )?;
    connection.execute_batch(
        "CREATE INDEX IF NOT EXISTS webhook_deliveries_pending
             ON webhook_deliveries(forwarded_at, next_attempt_at, received_at);",
    )?;
    Ok(())
}

fn ensure_column(connection: &Connection, name: &str, sql: &str) -> anyhow::Result<()> {
    let mut statement = connection.prepare("PRAGMA table_info(webhook_deliveries)")?;
    let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
    for column in columns {
        if column? == name {
            return Ok(());
        }
    }
    connection.execute_batch(sql)?;
    Ok(())
}

fn secure_database_path(database: &Path) -> anyhow::Result<PathBuf> {
    let database = if database.is_absolute() {
        database.to_owned()
    } else {
        std::env::current_dir()?.join(database)
    };
    let parent = database
        .parent()
        .ok_or_else(|| anyhow::anyhow!("relay database path has no parent"))?;
    if parent.exists() {
        verify_owner_only_directory(parent)?;
    } else {
        fs::create_dir_all(parent)?;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
        verify_owner_only_directory(parent)?;
    }

    match fs::symlink_metadata(&database) {
        Ok(_) => verify_owner_only_file(&database)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&database)?;
            verify_owner_only_file(&database)?;
        }
        Err(error) => return Err(error.into()),
    }
    Ok(database)
}

fn verify_owner_only_directory(path: &Path) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.file_type().is_dir()
            && !metadata.file_type().is_symlink()
            && metadata.uid() == u32::from(nix::unistd::geteuid())
            && is_owner_only(metadata.permissions().mode()),
        "relay database parent must be an owner-only directory"
    );
    Ok(())
}

fn verify_owner_only_file(path: &Path) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.file_type().is_file()
            && !metadata.file_type().is_symlink()
            && metadata.uid() == u32::from(nix::unistd::geteuid())
            && is_owner_only(metadata.permissions().mode()),
        "relay database must be a regular owner-only file"
    );
    Ok(())
}

#[allow(clippy::verbose_bit_mask)]
const fn is_owner_only(mode: u32) -> bool {
    mode & 0o077 == 0
}

fn sha256_hex(payload: &[u8]) -> String {
    use sha2::Digest;
    hex::encode(Sha256::digest(payload))
}

#[must_use]
pub fn verify_signature(secret: &[u8], body: &[u8], signature: &str) -> bool {
    let Some(hex_signature) = signature.strip_prefix("sha256=") else {
        return false;
    };
    let Ok(expected) = hex::decode(hex_signature) else {
        return false;
    };
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret) else {
        return false;
    };
    mac.update(body);
    mac.verify_slice(&expected).is_ok()
}
