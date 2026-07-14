use chrono::{DateTime, Utc};
use patchwright_core::TaskId;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Mutex;
use thiserror::Error;
use uuid::Uuid;

use crate::EventStore;

use super::process::{CodexProcess, CodexProcessError};
use super::protocol::{
    ClientMethod, ClientRequest, IncomingMessage, InitializedNotification, ProtocolDecoder,
    ProtocolError, RequestId, ResponseEnvelope,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CodexAccountState {
    SignedIn,
    SignedOut,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CodexSessionStatus {
    Starting,
    Initialized,
    Ready,
    StaleThreadNeedsConfirmation,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSessionRecord {
    pub task_id: TaskId,
    pub process_generation: Uuid,
    pub protocol_version: String,
    pub executable_version: String,
    pub account_state: CodexAccountState,
    pub thread_id: Option<String>,
    pub last_turn_id: Option<String>,
    pub last_sequence: u64,
    pub status: CodexSessionStatus,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexEventRecord {
    pub task_id: TaskId,
    pub process_generation: Uuid,
    pub sequence: u64,
    pub kind: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexEventDraft {
    pub kind: String,
    pub summary: String,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub item_id: Option<String>,
    pub content: Option<String>,
}

impl CodexEventDraft {
    #[must_use]
    pub fn status(kind: impl Into<String>, summary: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            summary: summary.into(),
            thread_id: None,
            turn_id: None,
            item_id: None,
            content: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ThreadBootstrap {
    Start { instructions: String },
    Resume { thread_id: String },
}

pub struct CodexSession {
    record: CodexSessionRecord,
}

impl CodexSession {
    pub async fn connect(
        task_id: TaskId,
        process: &mut CodexProcess,
        store: &Mutex<EventStore>,
        executable_version: &str,
        bootstrap: ThreadBootstrap,
    ) -> Result<Self, CodexSessionError> {
        validate_metadata(executable_version, "executable version", 128)?;
        validate_bootstrap(&bootstrap)?;
        let mut record = CodexSessionRecord {
            task_id,
            process_generation: process.generation(),
            protocol_version: "2.0".to_owned(),
            executable_version: executable_version.to_owned(),
            account_state: CodexAccountState::Unavailable,
            thread_id: match &bootstrap {
                ThreadBootstrap::Start { .. } => None,
                ThreadBootstrap::Resume { thread_id } => Some(thread_id.clone()),
            },
            last_turn_id: None,
            last_sequence: 0,
            status: CodexSessionStatus::Starting,
            updated_at: Utc::now(),
        };
        checkpoint_session(
            store,
            &mut record,
            "processStarted",
            "Codex process started",
        )?;

        let mut decoder = ProtocolDecoder::default();
        let initialize = send_request(
            process,
            &mut decoder,
            1,
            ClientMethod::Initialize,
            json!({
                "clientInfo": {
                    "name": "Patchwright",
                    "title": "Patchwright",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": null
            }),
        )
        .await?;
        validate_initialize_response(&initialize)?;
        process
            .write_line(&serde_json::to_string(&InitializedNotification::default())?)
            .await?;
        record.status = CodexSessionStatus::Initialized;
        checkpoint_session(store, &mut record, "initialized", "Codex initialized")?;

        let account = send_request(
            process,
            &mut decoder,
            2,
            ClientMethod::AccountRead,
            json!({"refreshToken": false}),
        )
        .await?;
        record.account_state = decode_account_state(&account);
        checkpoint_session(
            store,
            &mut record,
            "accountRead",
            "Codex account state read",
        )?;

        let is_resume = matches!(bootstrap, ThreadBootstrap::Resume { .. });
        let thread = bootstrap_thread(process, &mut decoder, &bootstrap).await?;
        if thread.is_error && is_resume {
            record.status = CodexSessionStatus::StaleThreadNeedsConfirmation;
            checkpoint_session(
                store,
                &mut record,
                "threadStale",
                "Saved Codex thread requires confirmation",
            )?;
            return Ok(Self { record });
        }
        if thread.is_error {
            record.status = CodexSessionStatus::Failed;
            checkpoint_session(
                store,
                &mut record,
                "threadFailed",
                "Codex thread start failed",
            )?;
            return Err(CodexSessionError::ServerRejected("thread/start"));
        }
        let thread_id = thread
            .payload
            .get("thread")
            .and_then(|thread| thread.get("id"))
            .and_then(Value::as_str)
            .ok_or(CodexSessionError::MissingResponseField("thread.id"))?;
        validate_metadata(thread_id, "thread id", 256)?;
        record.thread_id = Some(thread_id.to_owned());
        record.status = CodexSessionStatus::Ready;
        process.mark_ready()?;
        checkpoint_session(store, &mut record, "threadReady", "Codex thread ready")?;
        Ok(Self { record })
    }

    #[must_use]
    pub const fn status(&self) -> CodexSessionStatus {
        self.record.status
    }

    #[must_use]
    pub const fn account_state(&self) -> CodexAccountState {
        self.record.account_state
    }

    #[must_use]
    pub fn thread_id(&self) -> Option<&str> {
        self.record.thread_id.as_deref()
    }

    #[must_use]
    pub const fn process_generation(&self) -> Uuid {
        self.record.process_generation
    }

    #[must_use]
    pub const fn record(&self) -> &CodexSessionRecord {
        &self.record
    }
}

async fn bootstrap_thread(
    process: &mut CodexProcess,
    decoder: &mut ProtocolDecoder,
    bootstrap: &ThreadBootstrap,
) -> Result<ResponseEnvelope, CodexSessionError> {
    let (method, params) = match bootstrap {
        ThreadBootstrap::Start { instructions } => (
            ClientMethod::ThreadStart,
            json!({
                "cwd": process.worktree().to_string_lossy(),
                "baseInstructions": instructions,
                "approvalPolicy": "on-request",
                "sandbox": "workspace-write"
            }),
        ),
        ThreadBootstrap::Resume { thread_id } => (
            ClientMethod::ThreadResume,
            json!({
                "threadId": thread_id,
                "cwd": process.worktree().to_string_lossy()
            }),
        ),
    };
    send_request(process, decoder, 3, method, params).await
}

#[derive(Debug, Error)]
pub enum CodexSessionError {
    #[error(transparent)]
    Process(#[from] CodexProcessError),
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("failed to persist Codex session: {0}")]
    Store(#[from] anyhow::Error),
    #[error("Codex session store lock is poisoned")]
    StoreLock,
    #[error("Codex response is missing required field {0}")]
    MissingResponseField(&'static str),
    #[error("Codex app-server rejected {0}")]
    ServerRejected(&'static str),
    #[error("unexpected message during Codex handshake")]
    UnexpectedHandshakeMessage,
    #[error("invalid {0}")]
    InvalidMetadata(&'static str),
}

async fn send_request(
    process: &mut CodexProcess,
    decoder: &mut ProtocolDecoder,
    id: i64,
    method: ClientMethod,
    params: Value,
) -> Result<ResponseEnvelope, CodexSessionError> {
    let id = RequestId::Number(id);
    decoder.register_request(id.clone())?;
    let request = ClientRequest::new(id, method, params)?;
    process
        .write_line(&serde_json::to_string(&request)?)
        .await?;
    for _ in 0..128 {
        let line = process.read_initialization_line().await?;
        match decoder.decode_line(line.as_bytes())? {
            IncomingMessage::Response(response) => return Ok(response),
            IncomingMessage::Event(_) => {}
            IncomingMessage::ServerRequest(_) => {
                return Err(CodexSessionError::UnexpectedHandshakeMessage);
            }
        }
    }
    Err(CodexSessionError::UnexpectedHandshakeMessage)
}

fn validate_initialize_response(response: &ResponseEnvelope) -> Result<(), CodexSessionError> {
    if response.is_error {
        return Err(CodexSessionError::ServerRejected("initialize"));
    }
    for field in ["userAgent", "codexHome", "platformFamily", "platformOs"] {
        response
            .payload
            .get(field)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or(CodexSessionError::MissingResponseField(field))?;
    }
    Ok(())
}

fn decode_account_state(response: &ResponseEnvelope) -> CodexAccountState {
    if response.is_error {
        CodexAccountState::Unavailable
    } else if response.payload.get("account").is_some_and(Value::is_null) {
        CodexAccountState::SignedOut
    } else if response
        .payload
        .get("account")
        .is_some_and(Value::is_object)
    {
        CodexAccountState::SignedIn
    } else {
        CodexAccountState::Unavailable
    }
}

fn validate_bootstrap(bootstrap: &ThreadBootstrap) -> Result<(), CodexSessionError> {
    match bootstrap {
        ThreadBootstrap::Start { instructions } => {
            validate_metadata(instructions, "task instructions", 64 * 1024)
        }
        ThreadBootstrap::Resume { thread_id } => validate_metadata(thread_id, "thread id", 256),
    }
}

fn validate_metadata(
    value: &str,
    field: &'static str,
    maximum: usize,
) -> Result<(), CodexSessionError> {
    if value.trim().is_empty() || value.len() > maximum || value.contains('\0') {
        return Err(CodexSessionError::InvalidMetadata(field));
    }
    Ok(())
}

fn checkpoint_session(
    store: &Mutex<EventStore>,
    record: &mut CodexSessionRecord,
    kind: &str,
    summary: &str,
) -> Result<(), CodexSessionError> {
    store
        .lock()
        .map_err(|_| CodexSessionError::StoreLock)?
        .checkpoint_codex_session(record, kind, summary)?;
    Ok(())
}
