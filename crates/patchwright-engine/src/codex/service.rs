use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use patchwright_core::{TaskId, TaskState};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use uuid::Uuid;

use crate::{EventStore, TaskCheckpoint};

use super::process::{CodexProcess, CodexProcessError, CodexProcessFactory, CodexProcessState};
use super::protocol::{
    ClientMethod, ClientRequest, IncomingMessage, ProtocolDecoder, ProtocolError, RequestId,
    ResponseEnvelope,
};
use super::session::{
    CodexAccountState, CodexEventDraft, CodexEventRecord, CodexSession, CodexSessionError,
    CodexSessionRecord, CodexSessionStatus, ThreadBootstrap,
};

const MAX_INPUT_BYTES: usize = 64 * 1024;
const MAX_CLIENT_MESSAGE_ID_BYTES: usize = 128;
const MAX_EVENT_PAGE: usize = 500;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CodexServiceState {
    Unavailable,
    NotStarted,
    Ready,
    StaleThreadNeedsConfirmation,
    Failed,
    Exited,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexRuntimeStatus {
    pub task_id: TaskId,
    pub state: CodexServiceState,
    pub process_generation: Option<Uuid>,
    pub account_state: Option<CodexAccountState>,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub last_sequence: u64,
    pub can_start: bool,
    pub can_send: bool,
    pub can_steer: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexTurnReceipt {
    pub thread_id: String,
    pub turn_id: String,
    pub client_message_id: String,
}

pub struct CodexService {
    factory: CodexProcessFactory,
    executable_version: String,
    active: HashMap<TaskId, ActiveCodex>,
}

struct ActiveCodex {
    process: CodexProcess,
    decoder: ProtocolDecoder,
    session: CodexSessionRecord,
    next_request_id: i64,
    client_message_ids: HashSet<String>,
    active_turn_id: Option<String>,
}

impl CodexService {
    #[must_use]
    pub fn new(factory: CodexProcessFactory, executable_version: String) -> Self {
        Self {
            factory,
            executable_version,
            active: HashMap::new(),
        }
    }

    pub fn status(
        &self,
        task_id: TaskId,
        store: &Mutex<EventStore>,
    ) -> Result<CodexRuntimeStatus, CodexServiceError> {
        if let Some(active) = self.active.get(&task_id) {
            return Ok(status_from_active(task_id, active));
        }
        let persisted = lock_store(store)?.codex_session(task_id)?;
        Ok(match persisted {
            Some(session) => status_from_session(&session, false),
            None => CodexRuntimeStatus {
                task_id,
                state: CodexServiceState::NotStarted,
                process_generation: None,
                account_state: None,
                thread_id: None,
                turn_id: None,
                last_sequence: 0,
                can_start: true,
                can_send: false,
                can_steer: false,
            },
        })
    }

    pub async fn start(
        &mut self,
        task_id: TaskId,
        store: &Mutex<EventStore>,
    ) -> Result<CodexRuntimeStatus, CodexServiceError> {
        if self.active.contains_key(&task_id) {
            return self.status(task_id, store);
        }
        let (mut task, persisted_session, instructions) = {
            let store = lock_store(store)?;
            let task = store
                .load_task(task_id)?
                .ok_or(CodexServiceError::TaskNotFound)?;
            if !matches!(task.state, TaskState::Preparing | TaskState::Implementing) {
                return Err(CodexServiceError::InvalidTaskState(task.state));
            }
            let persisted_session = store.codex_session(task_id)?;
            let instructions = store.task_contract(task_id)?.map_or_else(
                || format!("Implement the Patchwright task: {}", task.title),
                |contract| {
                    format!(
                        "Goal: {}\nAcceptance criteria:\n- {}",
                        contract.goal(),
                        contract.acceptance_criteria().join("\n- ")
                    )
                },
            );
            (task, persisted_session, instructions)
        };
        let mut process = self
            .factory
            .launch(task_id.to_string(), Path::new(&task.repository_path))?;
        let bootstrap = persisted_session
            .as_ref()
            .and_then(|session| session.thread_id.clone())
            .map_or(ThreadBootstrap::Start { instructions }, |thread_id| {
                ThreadBootstrap::Resume { thread_id }
            });
        let session = CodexSession::connect(
            task_id,
            &mut process,
            store,
            &self.executable_version,
            bootstrap,
        )
        .await?;
        let record = session.record().clone();
        if record.status == CodexSessionStatus::Ready && task.state == TaskState::Preparing {
            task.transition(TaskState::Implementing)
                .map_err(|_| CodexServiceError::InvalidTaskState(task.state))?;
            let checkpoint = TaskCheckpoint::new(task.id, task.state, "Codex thread ready")?;
            lock_store(store)?.enter_implementing_with_codex(&task, &checkpoint, &record)?;
        }
        let status = status_from_session(&record, record.status == CodexSessionStatus::Ready);
        self.active.insert(
            task_id,
            ActiveCodex {
                process,
                decoder: ProtocolDecoder::default(),
                session: record,
                next_request_id: 4,
                client_message_ids: HashSet::new(),
                active_turn_id: None,
            },
        );
        Ok(status)
    }

    pub async fn start_turn(
        &mut self,
        task_id: TaskId,
        client_message_id: &str,
        input: &str,
        store: &Mutex<EventStore>,
    ) -> Result<CodexTurnReceipt, CodexServiceError> {
        validate_operator_input(client_message_id, input)?;
        let active = self
            .active
            .get_mut(&task_id)
            .ok_or(CodexServiceError::ProcessNotActive)?;
        if active.session.status != CodexSessionStatus::Ready {
            return Err(CodexServiceError::SessionNotReady);
        }
        if !active
            .client_message_ids
            .insert(client_message_id.to_owned())
        {
            return Err(CodexServiceError::DuplicateClientMessageId);
        }
        let thread_id = active
            .session
            .thread_id
            .clone()
            .ok_or(CodexServiceError::SessionNotReady)?;
        let response = request(
            active,
            ClientMethod::TurnStart,
            json!({
                "threadId": thread_id,
                "clientUserMessageId": client_message_id,
                "input": [{"type":"text", "text":input, "text_elements":[]}]
            }),
            store,
        )
        .await?;
        let turn_id = response
            .payload
            .get("turn")
            .and_then(|turn| turn.get("id"))
            .and_then(Value::as_str)
            .ok_or(CodexServiceError::MissingResponseField("turn.id"))?
            .to_owned();
        active.active_turn_id = Some(turn_id.clone());
        active.session.last_turn_id = Some(turn_id.clone());
        append_event(
            store,
            &mut active.session,
            CodexEventDraft {
                kind: "userMessage".into(),
                summary: "Operator message sent".into(),
                thread_id: Some(thread_id.clone()),
                turn_id: Some(turn_id.clone()),
                item_id: Some(client_message_id.to_owned()),
                content: Some(input.to_owned()),
            },
        )?;
        Ok(CodexTurnReceipt {
            thread_id,
            turn_id,
            client_message_id: client_message_id.to_owned(),
        })
    }

    pub async fn steer_turn(
        &mut self,
        task_id: TaskId,
        client_message_id: &str,
        input: &str,
        store: &Mutex<EventStore>,
    ) -> Result<CodexTurnReceipt, CodexServiceError> {
        validate_operator_input(client_message_id, input)?;
        let active = self
            .active
            .get_mut(&task_id)
            .ok_or(CodexServiceError::ProcessNotActive)?;
        if !active
            .client_message_ids
            .insert(client_message_id.to_owned())
        {
            return Err(CodexServiceError::DuplicateClientMessageId);
        }
        let thread_id = active
            .session
            .thread_id
            .clone()
            .ok_or(CodexServiceError::SessionNotReady)?;
        let turn_id = active
            .active_turn_id
            .clone()
            .ok_or(CodexServiceError::NoActiveTurn)?;
        let response = request(
            active,
            ClientMethod::TurnSteer,
            json!({
                "threadId": thread_id,
                "expectedTurnId": turn_id,
                "clientUserMessageId": client_message_id,
                "input": [{"type":"text", "text":input, "text_elements":[]}]
            }),
            store,
        )
        .await?;
        let response_turn_id = response
            .payload
            .get("turnId")
            .and_then(Value::as_str)
            .ok_or(CodexServiceError::MissingResponseField("turnId"))?;
        if response_turn_id != turn_id {
            return Err(CodexServiceError::TurnMismatch);
        }
        append_event(
            store,
            &mut active.session,
            CodexEventDraft {
                kind: "userSteer".into(),
                summary: "Operator steering message sent".into(),
                thread_id: Some(thread_id.clone()),
                turn_id: Some(turn_id.clone()),
                item_id: Some(client_message_id.to_owned()),
                content: Some(input.to_owned()),
            },
        )?;
        Ok(CodexTurnReceipt {
            thread_id,
            turn_id,
            client_message_id: client_message_id.to_owned(),
        })
    }

    pub async fn events(
        &mut self,
        task_id: TaskId,
        after: u64,
        limit: usize,
        store: &Mutex<EventStore>,
    ) -> Result<Vec<CodexEventRecord>, CodexServiceError> {
        if limit == 0 || limit > MAX_EVENT_PAGE {
            return Err(CodexServiceError::InvalidEventLimit);
        }
        if let Some(active) = self.active.get_mut(&task_id) {
            pump_available(active, store).await?;
        }
        let mut events = lock_store(store)?.codex_events(task_id, after)?;
        events.truncate(limit);
        Ok(events)
    }

    pub async fn stop(&mut self, task_id: TaskId) -> Result<(), CodexServiceError> {
        if let Some(mut active) = self.active.remove(&task_id) {
            active.process.terminate().await?;
        }
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum CodexServiceError {
    #[error(transparent)]
    Process(#[from] CodexProcessError),
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    #[error(transparent)]
    Session(#[from] CodexSessionError),
    #[error(transparent)]
    Persistence(#[from] anyhow::Error),
    #[error(transparent)]
    Job(#[from] crate::JobError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("Codex store lock is poisoned")]
    StoreLock,
    #[error("task was not found")]
    TaskNotFound,
    #[error("task state {0} cannot start Codex")]
    InvalidTaskState(TaskState),
    #[error("Codex process is not active for this task")]
    ProcessNotActive,
    #[error("Codex session is not ready")]
    SessionNotReady,
    #[error("a turn is not active")]
    NoActiveTurn,
    #[error("client message id was already used")]
    DuplicateClientMessageId,
    #[error("operator input is invalid")]
    InvalidInput,
    #[error("event page limit is invalid")]
    InvalidEventLimit,
    #[error("Codex response is missing {0}")]
    MissingResponseField(&'static str),
    #[error("Codex returned a different active turn")]
    TurnMismatch,
    #[error("Codex rejected the request")]
    RequestRejected,
}

async fn request(
    active: &mut ActiveCodex,
    method: ClientMethod,
    params: Value,
    store: &Mutex<EventStore>,
) -> Result<ResponseEnvelope, CodexServiceError> {
    let id = RequestId::Number(active.next_request_id);
    active.next_request_id += 1;
    active.decoder.register_request(id.clone())?;
    let request = ClientRequest::new(id, method, params)?;
    active
        .process
        .write_line(&serde_json::to_string(&request).expect("client request serialization"))
        .await?;
    loop {
        let line = active.process.read_line().await?;
        let raw: Value = serde_json::from_str(&line)?;
        match active.decoder.decode_line(line.as_bytes())? {
            IncomingMessage::Response(response) => {
                return if response.is_error {
                    Err(CodexServiceError::RequestRejected)
                } else {
                    Ok(response)
                };
            }
            message => persist_incoming(active, store, &raw, &message)?,
        }
    }
}

async fn pump_available(
    active: &mut ActiveCodex,
    store: &Mutex<EventStore>,
) -> Result<(), CodexServiceError> {
    for _ in 0..128 {
        let line = match active.process.read_line_for(Duration::from_millis(5)).await {
            Ok(line) => line,
            Err(CodexProcessError::Timeout { .. }) => break,
            Err(error) => return Err(error.into()),
        };
        let raw: Value = serde_json::from_str(&line)?;
        let message = active.decoder.decode_line(line.as_bytes())?;
        persist_incoming(active, store, &raw, &message)?;
    }
    Ok(())
}

fn persist_incoming(
    active: &mut ActiveCodex,
    store: &Mutex<EventStore>,
    raw: &Value,
    message: &IncomingMessage,
) -> Result<(), CodexServiceError> {
    if matches!(message, IncomingMessage::Response(_)) {
        return Ok(());
    }
    if let Some(draft) = normalize_event(raw) {
        let completed = draft.kind == "turnCompleted";
        append_event(store, &mut active.session, draft)?;
        if completed {
            active.active_turn_id = None;
        }
    }
    Ok(())
}

fn normalize_event(raw: &Value) -> Option<CodexEventDraft> {
    let method = raw.get("method")?.as_str()?;
    let params = raw.get("params")?;
    let thread_id = string_field(params, "threadId");
    let turn_id = string_field(params, "turnId")
        .or_else(|| params.get("turn").and_then(|turn| string_field(turn, "id")));
    let item_id = string_field(params, "itemId")
        .or_else(|| params.get("item").and_then(|item| string_field(item, "id")));
    let (kind, summary, content) = match method {
        "item/agentMessage/delta" => (
            "textDelta",
            "Codex response streamed",
            string_field(params, "delta"),
        ),
        "item/reasoning/summaryTextDelta" | "item/reasoning/textDelta" => (
            "reasoningDelta",
            "Codex reasoning streamed",
            string_field(params, "delta"),
        ),
        "item/commandExecution/outputDelta" => (
            "commandOutputDelta",
            "Command output streamed",
            string_field(params, "delta"),
        ),
        "item/fileChange/outputDelta" => (
            "fileChangeDelta",
            "File change streamed",
            string_field(params, "delta"),
        ),
        "item/started" => {
            let item = params.get("item")?;
            let item_type = item
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            (
                "itemStarted",
                "Codex item started",
                Some(item_type.to_owned()),
            )
        }
        "item/completed" => {
            let item = params.get("item")?;
            let content = completed_item_content(item);
            ("itemCompleted", "Codex item completed", content)
        }
        "turn/completed" => (
            "turnCompleted",
            "Codex turn completed",
            params
                .get("turn")
                .and_then(|turn| string_field(turn, "status")),
        ),
        "error" => (
            "error",
            "Codex turn failed",
            params
                .get("error")
                .and_then(|error| string_field(error, "message")),
        ),
        _ => return None,
    };
    Some(CodexEventDraft {
        kind: kind.into(),
        summary: summary.into(),
        thread_id,
        turn_id,
        item_id,
        content: content.map(bounded_content),
    })
}

fn completed_item_content(item: &Value) -> Option<String> {
    match item.get("type").and_then(Value::as_str)? {
        "agentMessage" | "plan" => string_field(item, "text"),
        "reasoning" => item.get("summary").and_then(Value::as_array).map(|parts| {
            parts
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("\n")
        }),
        "commandExecution" => {
            string_field(item, "aggregatedOutput").or_else(|| string_field(item, "command"))
        }
        "fileChange" => item
            .get("changes")
            .and_then(|changes| serde_json::to_string(changes).ok()),
        item_type => Some(item_type.to_owned()),
    }
}

fn string_field(value: &Value, field: &str) -> Option<String> {
    value.get(field).and_then(Value::as_str).map(str::to_owned)
}

fn bounded_content(value: String) -> String {
    if value.len() <= MAX_INPUT_BYTES {
        return value;
    }
    let mut boundary = MAX_INPUT_BYTES;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value[..boundary].to_owned()
}

fn append_event(
    store: &Mutex<EventStore>,
    session: &mut CodexSessionRecord,
    draft: CodexEventDraft,
) -> Result<(), CodexServiceError> {
    lock_store(store)?.append_codex_event(session, draft)?;
    Ok(())
}

fn lock_store(
    store: &Mutex<EventStore>,
) -> Result<std::sync::MutexGuard<'_, EventStore>, CodexServiceError> {
    store.lock().map_err(|_| CodexServiceError::StoreLock)
}

fn validate_operator_input(client_message_id: &str, input: &str) -> Result<(), CodexServiceError> {
    if client_message_id.is_empty()
        || client_message_id.len() > MAX_CLIENT_MESSAGE_ID_BYTES
        || client_message_id.chars().any(char::is_control)
        || input.trim().is_empty()
        || input.len() > MAX_INPUT_BYTES
        || input.contains('\0')
    {
        return Err(CodexServiceError::InvalidInput);
    }
    Ok(())
}

fn status_from_active(task_id: TaskId, active: &ActiveCodex) -> CodexRuntimeStatus {
    let process_ready = active.process.state() == CodexProcessState::Ready;
    let mut status = status_from_session(&active.session, process_ready);
    status.task_id = task_id;
    status.can_steer = process_ready && active.active_turn_id.is_some();
    status.turn_id.clone_from(&active.active_turn_id);
    status
}

fn status_from_session(session: &CodexSessionRecord, process_ready: bool) -> CodexRuntimeStatus {
    let state = match session.status {
        CodexSessionStatus::Ready if process_ready => CodexServiceState::Ready,
        CodexSessionStatus::StaleThreadNeedsConfirmation => {
            CodexServiceState::StaleThreadNeedsConfirmation
        }
        CodexSessionStatus::Failed => CodexServiceState::Failed,
        CodexSessionStatus::Starting
        | CodexSessionStatus::Initialized
        | CodexSessionStatus::Ready => CodexServiceState::Exited,
    };
    CodexRuntimeStatus {
        task_id: session.task_id,
        state,
        process_generation: Some(session.process_generation),
        account_state: Some(session.account_state),
        thread_id: session.thread_id.clone(),
        turn_id: session.last_turn_id.clone(),
        last_sequence: session.last_sequence,
        can_start: !process_ready,
        can_send: process_ready,
        can_steer: false,
    }
}
