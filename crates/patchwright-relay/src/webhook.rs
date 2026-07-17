use axum::{
    Router,
    body::Bytes,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use hmac::{Hmac, Mac};
use rusqlite::{Connection, params};
use serde::Deserialize;
use sha2::Sha256;
use std::{
    fs::{self, OpenOptions},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

const MAX_BODY_BYTES: usize = 1024 * 1024;

#[derive(Clone)]
pub struct RelayState {
    secret: Arc<Vec<u8>>,
    inbox: Arc<Mutex<Connection>>,
}

impl RelayState {
    #[must_use]
    pub fn new(secret: Vec<u8>) -> Self {
        let connection = Connection::open_in_memory().expect("open in-memory relay inbox");
        initialize_inbox(&connection).expect("initialize in-memory relay inbox");
        Self {
            secret: Arc::new(secret),
            inbox: Arc::new(Mutex::new(connection)),
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

    fn record(&self, delivery: &WebhookDelivery) -> anyhow::Result<RecordOutcome> {
        let connection = self
            .inbox
            .lock()
            .map_err(|_| anyhow::anyhow!("relay inbox lock poisoned"))?;
        let payload_digest = sha256_hex(&delivery.payload);
        let inserted = connection.execute(
            "INSERT OR IGNORE INTO webhook_deliveries
                (delivery_id, event, action, payload, payload_sha256, received_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                delivery.delivery_id,
                delivery.event,
                delivery.action,
                delivery.payload,
                payload_digest,
                chrono::Utc::now().to_rfc3339(),
            ],
        )?;
        if inserted == 1 {
            return Ok(RecordOutcome::Accepted);
        }
        Ok(RecordOutcome::Duplicate)
    }
}

struct WebhookDelivery {
    delivery_id: String,
    event: String,
    action: String,
    payload: Vec<u8>,
}

enum RecordOutcome {
    Accepted,
    Duplicate,
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

    let delivery = WebhookDelivery {
        delivery_id: delivery,
        event,
        action,
        payload: body.to_vec(),
    };
    match tokio::task::spawn_blocking(move || state.record(&delivery)).await {
        Ok(Ok(RecordOutcome::Accepted)) => (StatusCode::ACCEPTED, "accepted"),
        Ok(Ok(RecordOutcome::Duplicate)) => (StatusCode::OK, "duplicate"),
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
             received_at TEXT NOT NULL
         );",
    )?;
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
