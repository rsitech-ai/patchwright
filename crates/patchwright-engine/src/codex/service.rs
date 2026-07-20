use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use patchwright_core::{TaskId, TaskState};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use tokio::time::{Instant, timeout_at};
use uuid::Uuid;

use crate::{CancellationState, EventStore, Job, JobId, JobKind, JobState, TaskCheckpoint};

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
const MAX_REQUEST_EVENTS: usize = 4_096;
const MAX_REQUEST_EVENT_BYTES: usize = 16 * 1024 * 1024;
const MAX_REQUEST_DURATION: Duration = Duration::from_secs(5 * 60);

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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CodexApprovalKind {
    Command,
    FileChange,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CodexApprovalState {
    Pending,
    Approved,
    Declined,
    Expired,
    Invalidated,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexRuntimeApproval {
    pub id: Uuid,
    pub task_id: TaskId,
    pub class: patchwright_core::ApprovalClass,
    pub request_id: RequestId,
    pub process_generation: Uuid,
    pub thread_id: String,
    pub turn_id: String,
    pub item_id: String,
    pub kind: CodexApprovalKind,
    pub reason: Option<String>,
    pub command: Option<String>,
    pub cwd: Option<String>,
    pub grant_root: Option<String>,
    pub state: CodexApprovalState,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub decided_at: Option<DateTime<Utc>>,
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
    execution_job_id: JobId,
    interrupt_sent: bool,
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

    /// Terminates every owned app-server process and records non-terminal jobs
    /// as interrupted before the engine releases its database lease.
    pub async fn shutdown(&mut self, store: &Mutex<EventStore>) -> Result<(), CodexServiceError> {
        let active = self
            .active
            .drain()
            .map(|(_, active)| active)
            .collect::<Vec<_>>();
        let execution_job_ids = active
            .iter()
            .map(|active| active.execution_job_id)
            .collect::<Vec<_>>();
        let mut terminations = tokio::task::JoinSet::new();
        for mut active in active {
            terminations.spawn(async move { active.process.terminate().await });
        }
        let mut first_process_error = None;
        while let Some(termination) = terminations.join_next().await {
            match termination {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    tracing::warn!(error = %error, "terminate Codex app-server during shutdown");
                    if first_process_error.is_none() {
                        first_process_error = Some(error);
                    }
                }
                Err(error) => tracing::error!(error = %error, "join Codex shutdown task"),
            }
        }
        for execution_job_id in execution_job_ids {
            let store = lock_store(store)?;
            if let Some(job) = store.job(execution_job_id)? {
                if matches!(job.state(), JobState::Running | JobState::Cancelling) {
                    let _ = store.transition_job(
                        job.id(),
                        job.state(),
                        JobState::Interrupted,
                        job.cancellation(),
                        "Engine shutdown interrupted Codex execution",
                        job.checkpoint(),
                    )?;
                }
            }
        }
        if let Some(error) = first_process_error {
            return Err(error.into());
        }
        Ok(())
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
            let mut task = store
                .load_task(task_id)?
                .ok_or(CodexServiceError::TaskNotFound)?;
            if task.state == TaskState::Paused {
                task.resume()
                    .map_err(|_| CodexServiceError::InvalidTaskState(task.state))?;
                store.save_task(&task, "Codex task resumed")?;
            }
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
        let job = Job::new(
            JobKind::TaskExecution,
            Some(task_id),
            "Codex task execution queued",
        )?;
        {
            let store = lock_store(store)?;
            store.create_job(&job)?;
            if !store.transition_job(
                job.id(),
                JobState::Queued,
                JobState::Running,
                CancellationState::NotRequested,
                "Codex task execution running",
                None,
            )? {
                return Err(CodexServiceError::JobTransition);
            }
        }
        self.active.insert(
            task_id,
            ActiveCodex {
                process,
                decoder: ProtocolDecoder::default(),
                session: record,
                next_request_id: 4,
                client_message_ids: HashSet::new(),
                active_turn_id: None,
                execution_job_id: job.id(),
                interrupt_sent: false,
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
                content: Some(bounded_content(input.to_owned())),
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
                content: Some(bounded_content(input.to_owned())),
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
        let pump_result = if let Some(active) = self.active.get_mut(&task_id) {
            pump_available(active, store).await
        } else {
            Ok(())
        };
        if let Err(error) = pump_result {
            self.record_crash(task_id, store).await?;
            return Err(error);
        }
        let mut events = lock_store(store)?.codex_events(task_id, after)?;
        events.truncate(limit);
        Ok(events)
    }

    async fn record_crash(
        &mut self,
        task_id: TaskId,
        store: &Mutex<EventStore>,
    ) -> Result<(), CodexServiceError> {
        let Some(mut active) = self.active.remove(&task_id) else {
            return Ok(());
        };
        active.session.status = CodexSessionStatus::Failed;
        append_event(
            store,
            &mut active.session,
            CodexEventDraft::status("error", "Codex app-server exited unexpectedly"),
        )?;
        let _ = active.process.terminate().await;
        let mut task = lock_store(store)?
            .load_task(task_id)?
            .ok_or(CodexServiceError::TaskNotFound)?;
        if !matches!(
            task.state,
            TaskState::Failed | TaskState::Cancelled | TaskState::Completed
        ) {
            task.interrupt(
                TaskState::Failed,
                "Codex app-server exited unexpectedly; worktree and evidence retained",
            )
            .map_err(|_| CodexServiceError::InvalidTaskState(task.state))?;
            let checkpoint = TaskCheckpoint::new(
                task_id,
                TaskState::Failed,
                "Codex app-server crash recorded",
            )?;
            lock_store(store)?.save_task_with_checkpoint(
                &task,
                "Codex app-server crash recorded",
                &checkpoint,
            )?;
        }
        let _ = lock_store(store)?.transition_job(
            active.execution_job_id,
            JobState::Running,
            JobState::Failed,
            CancellationState::NotRequested,
            "Codex app-server exited unexpectedly",
            None,
        )?;
        Ok(())
    }

    pub async fn approvals(
        &mut self,
        task_id: TaskId,
        store: &Mutex<EventStore>,
    ) -> Result<Vec<CodexRuntimeApproval>, CodexServiceError> {
        if let Some(active) = self.active.get_mut(&task_id) {
            pump_available(active, store).await?;
        }
        Ok(lock_store(store)?.codex_runtime_approvals(task_id)?)
    }

    pub async fn resolve_approval(
        &mut self,
        task_id: TaskId,
        approval_id: Uuid,
        process_generation: Uuid,
        approve: bool,
        store: &Mutex<EventStore>,
    ) -> Result<CodexRuntimeApproval, CodexServiceError> {
        let active = self
            .active
            .get_mut(&task_id)
            .ok_or(CodexServiceError::ProcessNotActive)?;
        let mut approval = lock_store(store)?
            .codex_runtime_approval(approval_id)?
            .ok_or(CodexServiceError::ApprovalNotFound)?;
        if approval.state != CodexApprovalState::Pending {
            return Ok(approval);
        }
        if approval.process_generation != process_generation
            || active.session.process_generation != process_generation
            || active.active_turn_id.as_deref() != Some(&approval.turn_id)
            || Utc::now() >= approval.expires_at
        {
            approval.state = if Utc::now() >= approval.expires_at {
                CodexApprovalState::Expired
            } else {
                CodexApprovalState::Invalidated
            };
            lock_store(store)?.save_codex_runtime_approval(&approval)?;
            return Err(CodexServiceError::ApprovalInvalid);
        }
        let response = json!({"jsonrpc":"2.0", "id":approval.request_id, "result":{"decision": if approve {"accept"} else {"decline"}}});
        active
            .process
            .write_line(&serde_json::to_string(&response)?)
            .await?;
        approval.state = if approve {
            CodexApprovalState::Approved
        } else {
            CodexApprovalState::Declined
        };
        approval.decided_at = Some(Utc::now());
        lock_store(store)?.save_codex_runtime_approval(&approval)?;
        append_event(
            store,
            &mut active.session,
            CodexEventDraft {
                kind: "approvalResolved".into(),
                summary: if approve {
                    "Codex runtime request approved once".into()
                } else {
                    "Codex runtime request declined".into()
                },
                thread_id: Some(approval.thread_id.clone()),
                turn_id: Some(approval.turn_id.clone()),
                item_id: Some(approval.item_id.clone()),
                content: None,
            },
        )?;
        Ok(approval)
    }

    pub async fn stop(&mut self, task_id: TaskId) -> Result<(), CodexServiceError> {
        if let Some(mut active) = self.active.remove(&task_id) {
            active.process.terminate().await?;
        }
        Ok(())
    }

    pub async fn interrupt(
        &mut self,
        task_id: TaskId,
        cancel: bool,
        store: &Mutex<EventStore>,
    ) -> Result<CodexRuntimeStatus, CodexServiceError> {
        let mut active = self
            .active
            .remove(&task_id)
            .ok_or(CodexServiceError::ProcessNotActive)?;
        {
            let store = lock_store(store)?;
            if !store.transition_job(
                active.execution_job_id,
                JobState::Running,
                JobState::Cancelling,
                CancellationState::Requested,
                if cancel {
                    "Codex task cancellation requested"
                } else {
                    "Codex task pause requested"
                },
                None,
            )? {
                return Err(CodexServiceError::JobTransition);
            }
        }
        let completed_before_cancel = request_interrupt_if_active(&mut active, store).await;
        active.process.terminate().await?;
        let mut task = lock_store(store)?
            .load_task(task_id)?
            .ok_or(CodexServiceError::TaskNotFound)?;
        if completed_before_cancel {
            lock_store(store)?.transition_job(
                active.execution_job_id,
                JobState::Cancelling,
                JobState::Succeeded,
                CancellationState::Acknowledged,
                "Codex turn completed during cancellation",
                None,
            )?;
        } else {
            let next = if cancel {
                TaskState::Cancelled
            } else {
                TaskState::Paused
            };
            task.interrupt(
                next,
                if cancel {
                    "Cancelled by operator; worktree and evidence retained"
                } else {
                    "Paused by operator; worktree and evidence retained"
                },
            )
            .map_err(|_| CodexServiceError::InvalidTaskState(task.state))?;
            let checkpoint = TaskCheckpoint::new(
                task_id,
                next,
                if cancel {
                    "Codex task cancelled"
                } else {
                    "Codex task paused"
                },
            )?;
            lock_store(store)?.save_task_with_checkpoint(
                &task,
                if cancel {
                    "Codex task cancelled"
                } else {
                    "Codex task paused"
                },
                &checkpoint,
            )?;
            if !lock_store(store)?.transition_job(
                active.execution_job_id,
                JobState::Cancelling,
                JobState::Cancelled,
                CancellationState::Acknowledged,
                if cancel {
                    "Codex task cancelled"
                } else {
                    "Codex task paused"
                },
                None,
            )? {
                return Err(CodexServiceError::JobTransition);
            }
        }
        self.status(task_id, store)
    }
}

async fn request_interrupt_if_active(active: &mut ActiveCodex, store: &Mutex<EventStore>) -> bool {
    let (Some(thread_id), Some(turn_id)) = (
        active.session.thread_id.clone(),
        active.active_turn_id.clone(),
    ) else {
        return false;
    };
    if active.interrupt_sent {
        return false;
    }
    active.interrupt_sent = true;
    let result = tokio::time::timeout(
        Duration::from_millis(750),
        request(
            active,
            ClientMethod::TurnInterrupt,
            json!({"threadId":thread_id,"turnId":turn_id}),
            store,
        ),
    )
    .await;
    matches!(result, Ok(Ok(_))) && active.active_turn_id.is_none()
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
    #[error("Codex returned a response for a different request")]
    ResponseMismatch,
    #[error("Codex request exceeded its aggregate event or duration budget")]
    RequestBudgetExceeded,
    #[error("Codex runtime approval was not found")]
    ApprovalNotFound,
    #[error("Codex runtime approval is expired or no longer bound to the active turn")]
    ApprovalInvalid,
    #[error("durable Codex job compare-and-set transition failed")]
    JobTransition,
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
    let expected_id = id.clone();
    let result = async {
        let request = ClientRequest::new(id, method, params)?;
        active
            .process
            .write_line(&serde_json::to_string(&request).expect("client request serialization"))
            .await?;
        let deadline = Instant::now() + MAX_REQUEST_DURATION;
        let mut event_count = 0usize;
        let mut event_bytes = 0usize;
        loop {
            let line = timeout_at(deadline, active.process.read_line())
                .await
                .map_err(|_| CodexServiceError::RequestBudgetExceeded)??;
            event_count = event_count.saturating_add(1);
            event_bytes = event_bytes.saturating_add(line.len());
            if event_count > MAX_REQUEST_EVENTS || event_bytes > MAX_REQUEST_EVENT_BYTES {
                return Err(CodexServiceError::RequestBudgetExceeded);
            }
            let raw: Value = serde_json::from_str(&line)?;
            match active.decoder.decode_line(line.as_bytes())? {
                IncomingMessage::Response(response) => {
                    if response.id != expected_id {
                        return Err(CodexServiceError::ResponseMismatch);
                    }
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
    .await;
    if result.is_err() {
        active.decoder.cancel_request(&expected_id);
    }
    result
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
        if completed
            && (draft.thread_id.as_deref() != active.session.thread_id.as_deref()
                || draft.turn_id.as_deref() != active.active_turn_id.as_deref())
        {
            return Ok(());
        }
        append_event(store, &mut active.session, draft)?;
        if completed {
            active.active_turn_id = None;
            let store = lock_store(store)?;
            let mut task = store
                .load_task(active.session.task_id)?
                .ok_or(CodexServiceError::TaskNotFound)?;
            if task.state == TaskState::Implementing {
                task.transition(TaskState::Verifying)
                    .map_err(|_| CodexServiceError::InvalidTaskState(task.state))?;
                let checkpoint = TaskCheckpoint::new(
                    task.id,
                    task.state,
                    "Codex turn completed; verification is ready",
                )?;
                task.checkpoint_id = Some(checkpoint.id());
                store.save_task_with_checkpoint(
                    &task,
                    "Codex implementation turn completed",
                    &checkpoint,
                )?;
            }
        }
    }
    if let IncomingMessage::ServerRequest(request) = message {
        if let Some(approval) = normalize_approval(active, request)? {
            lock_store(store)?.save_codex_runtime_approval(&approval)?;
        }
    }
    Ok(())
}

fn normalize_approval(
    active: &ActiveCodex,
    request: &super::protocol::ServerRequestEnvelope,
) -> Result<Option<CodexRuntimeApproval>, CodexServiceError> {
    use super::protocol::ServerRequestKind;
    let kind = match request.kind {
        ServerRequestKind::CommandApproval => CodexApprovalKind::Command,
        ServerRequestKind::FileChangeApproval => CodexApprovalKind::FileChange,
        _ => return Ok(None),
    };
    let p = &request.params;
    let required = |name| {
        p.get(name)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or(CodexServiceError::MissingResponseField("approval identity"))
    };
    let now = Utc::now();
    Ok(Some(CodexRuntimeApproval {
        id: Uuid::new_v4(),
        task_id: active.session.task_id,
        class: patchwright_core::ApprovalClass::CodexRuntime,
        request_id: request.id.clone(),
        process_generation: active.session.process_generation,
        thread_id: required("threadId")?,
        turn_id: required("turnId")?,
        item_id: required("itemId")?,
        kind,
        reason: string_field(p, "reason").map(bounded_content),
        command: string_field(p, "command").map(bounded_content),
        cwd: string_field(p, "cwd").map(bounded_content),
        grant_root: string_field(p, "grantRoot").map(bounded_content),
        state: CodexApprovalState::Pending,
        created_at: now,
        expires_at: now + ChronoDuration::minutes(10),
        decided_at: None,
    }))
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
    if contains_sensitive_content(&value) {
        return "[REDACTED]".to_owned();
    }
    if value.len() <= MAX_INPUT_BYTES {
        return value;
    }
    let mut boundary = MAX_INPUT_BYTES;
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value[..boundary].to_owned()
}

fn contains_sensitive_content(value: &str) -> bool {
    let lowercase = value.to_ascii_lowercase();
    [
        "authorization: bearer ",
        "bearer ",
        "gho_",
        "ghp_",
        "ghs_",
        "github_pat_",
        "sk-",
        "xoxb-",
        "xoxp-",
        "token=",
        "secret=",
        "password=",
        "-----begin private key-----",
        "-----begin rsa private key-----",
        "-----begin openssh private key-----",
    ]
    .iter()
    .any(|marker| lowercase.contains(marker))
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
