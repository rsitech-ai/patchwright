use std::collections::HashSet;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(untagged)]
pub enum RequestId {
    Number(i64),
    String(String),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ClientMethod {
    #[serde(rename = "initialize")]
    Initialize,
    #[serde(rename = "account/read")]
    AccountRead,
    #[serde(rename = "thread/start")]
    ThreadStart,
    #[serde(rename = "thread/resume")]
    ThreadResume,
    #[serde(rename = "turn/start")]
    TurnStart,
    #[serde(rename = "turn/steer")]
    TurnSteer,
    #[serde(rename = "turn/interrupt")]
    TurnInterrupt,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ClientRequest {
    jsonrpc: String,
    pub id: RequestId,
    pub method: ClientMethod,
    pub params: Value,
}

impl ClientRequest {
    pub fn new(id: RequestId, method: ClientMethod, params: Value) -> Result<Self, ProtocolError> {
        validate_request_id(&id)?;
        Ok(Self {
            jsonrpc: "2.0".to_owned(),
            id,
            method,
            params,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct InitializedNotification {
    jsonrpc: String,
    method: String,
}

impl Default for InitializedNotification {
    fn default() -> Self {
        Self {
            jsonrpc: "2.0".to_owned(),
            method: "initialized".to_owned(),
        }
    }
}

#[derive(Clone, PartialEq)]
pub struct ResponseEnvelope {
    pub id: RequestId,
    pub payload: Value,
    pub is_error: bool,
}

impl fmt::Debug for ResponseEnvelope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResponseEnvelope")
            .field("id", &self.id)
            .field("is_error", &self.is_error)
            .field("payload", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ServerRequestKind {
    CommandApproval,
    FileChangeApproval,
    PermissionApproval,
    Unsupported(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServerRequestEnvelope {
    pub id: RequestId,
    pub kind: ServerRequestKind,
    pub params: Value,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum TurnStatus {
    #[serde(rename = "completed")]
    Completed,
    #[serde(rename = "interrupted")]
    Interrupted,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "inProgress")]
    InProgress,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CodexEvent {
    Initialized,
    ItemStarted {
        thread_id: String,
        turn_id: String,
        item_id: String,
    },
    ItemCompleted {
        thread_id: String,
        turn_id: String,
        item_id: String,
    },
    TurnCompleted {
        thread_id: String,
        turn_id: String,
        status: TurnStatus,
    },
    Error {
        thread_id: String,
        turn_id: String,
        will_retry: bool,
    },
    Unsupported {
        method: String,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum IncomingMessage {
    Response(ResponseEnvelope),
    ServerRequest(ServerRequestEnvelope),
    Event(CodexEvent),
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum ProtocolError {
    #[error("Codex app-server line is {actual} bytes; maximum is {maximum}")]
    LineTooLarge { actual: usize, maximum: usize },
    #[error("malformed Codex app-server JSON: {0}")]
    MalformedJson(String),
    #[error("Codex app-server message is not a JSON object")]
    NotAnObject,
    #[error("Codex app-server message is missing required field {0}")]
    MissingField(&'static str),
    #[error("unsupported JSON-RPC version")]
    UnsupportedJsonRpcVersion,
    #[error("invalid request id")]
    InvalidRequestId,
    #[error("request id is already pending: {0:?}")]
    DuplicateRequestId(RequestId),
    #[error("response id does not match a pending request: {0:?}")]
    UnexpectedResponseId(RequestId),
    #[error("turn completion was already observed for {thread_id}/{turn_id}")]
    DuplicateCompletion { thread_id: String, turn_id: String },
    #[error("unknown required turn status: {0}")]
    UnknownTurnStatus(String),
}

#[derive(Default)]
pub struct ProtocolDecoder {
    pending_requests: HashSet<RequestId>,
    completed_turns: HashSet<(String, String)>,
}

impl ProtocolDecoder {
    pub fn register_request(&mut self, id: RequestId) -> Result<(), ProtocolError> {
        validate_request_id(&id)?;
        if !self.pending_requests.insert(id.clone()) {
            return Err(ProtocolError::DuplicateRequestId(id));
        }
        Ok(())
    }

    pub fn cancel_request(&mut self, id: &RequestId) -> bool {
        self.pending_requests.remove(id)
    }

    pub fn decode_line(&mut self, line: &[u8]) -> Result<IncomingMessage, ProtocolError> {
        if line.len() > MAX_LINE_BYTES {
            return Err(ProtocolError::LineTooLarge {
                actual: line.len(),
                maximum: MAX_LINE_BYTES,
            });
        }
        let value: Value = serde_json::from_slice(line)
            .map_err(|error| ProtocolError::MalformedJson(error.to_string()))?;
        let object = value.as_object().ok_or(ProtocolError::NotAnObject)?;
        if object.get("jsonrpc").is_some()
            && object.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
        {
            return Err(ProtocolError::UnsupportedJsonRpcVersion);
        }

        match (
            object.get("id"),
            object.get("method").and_then(Value::as_str),
        ) {
            (Some(id), Some(method)) => {
                Self::decode_server_request(id, method, object.get("params"))
            }
            (Some(id), None) => self.decode_response(id, object),
            (None, Some(method)) => self.decode_notification(method, object.get("params")),
            (None, None) => Err(ProtocolError::MissingField("method or id")),
        }
    }

    fn decode_response(
        &mut self,
        id: &Value,
        object: &serde_json::Map<String, Value>,
    ) -> Result<IncomingMessage, ProtocolError> {
        let id = decode_request_id(id)?;
        if !self.pending_requests.remove(&id) {
            return Err(ProtocolError::UnexpectedResponseId(id));
        }
        let (payload, is_error) = if let Some(error) = object.get("error") {
            (error.clone(), true)
        } else {
            (
                object
                    .get("result")
                    .cloned()
                    .ok_or(ProtocolError::MissingField("result"))?,
                false,
            )
        };
        Ok(IncomingMessage::Response(ResponseEnvelope {
            id,
            payload,
            is_error,
        }))
    }

    fn decode_server_request(
        id: &Value,
        method: &str,
        params: Option<&Value>,
    ) -> Result<IncomingMessage, ProtocolError> {
        required_object(params)?;
        let id = decode_request_id(id)?;
        let kind = match method {
            "item/commandExecution/requestApproval" => ServerRequestKind::CommandApproval,
            "item/fileChange/requestApproval" => ServerRequestKind::FileChangeApproval,
            "item/permissions/requestApproval" => ServerRequestKind::PermissionApproval,
            other => ServerRequestKind::Unsupported(other.to_owned()),
        };
        Ok(IncomingMessage::ServerRequest(ServerRequestEnvelope {
            id,
            kind,
            params: params.cloned().expect("validated server request params"),
        }))
    }

    fn decode_notification(
        &mut self,
        method: &str,
        params: Option<&Value>,
    ) -> Result<IncomingMessage, ProtocolError> {
        let event = match method {
            "initialized" => CodexEvent::Initialized,
            "item/started" => decode_item(params, true)?,
            "item/completed" => decode_item(params, false)?,
            "turn/completed" => self.decode_completion(params)?,
            "error" => decode_error(params)?,
            other => CodexEvent::Unsupported {
                method: other.to_owned(),
            },
        };
        Ok(IncomingMessage::Event(event))
    }

    fn decode_completion(&mut self, params: Option<&Value>) -> Result<CodexEvent, ProtocolError> {
        let params = required_object(params)?;
        let thread_id = required_string(params.get("threadId"), "threadId")?;
        let turn = params
            .get("turn")
            .and_then(Value::as_object)
            .ok_or(ProtocolError::MissingField("turn"))?;
        let turn_id = required_string(turn.get("id"), "turn.id")?;
        let status_value = required_string(turn.get("status"), "turn.status")?;
        let status = serde_json::from_value(Value::String(status_value.clone()))
            .map_err(|_| ProtocolError::UnknownTurnStatus(status_value))?;
        if !self
            .completed_turns
            .insert((thread_id.clone(), turn_id.clone()))
        {
            return Err(ProtocolError::DuplicateCompletion { thread_id, turn_id });
        }
        Ok(CodexEvent::TurnCompleted {
            thread_id,
            turn_id,
            status,
        })
    }
}

fn decode_item(params: Option<&Value>, started: bool) -> Result<CodexEvent, ProtocolError> {
    let params = required_object(params)?;
    let thread_id = required_string(params.get("threadId"), "threadId")?;
    let turn_id = required_string(params.get("turnId"), "turnId")?;
    let item = params
        .get("item")
        .and_then(Value::as_object)
        .ok_or(ProtocolError::MissingField("item"))?;
    let item_id = required_string(item.get("id"), "item.id")?;
    Ok(if started {
        CodexEvent::ItemStarted {
            thread_id,
            turn_id,
            item_id,
        }
    } else {
        CodexEvent::ItemCompleted {
            thread_id,
            turn_id,
            item_id,
        }
    })
}

fn decode_error(params: Option<&Value>) -> Result<CodexEvent, ProtocolError> {
    let params = required_object(params)?;
    Ok(CodexEvent::Error {
        thread_id: required_string(params.get("threadId"), "threadId")?,
        turn_id: required_string(params.get("turnId"), "turnId")?,
        will_retry: params
            .get("willRetry")
            .and_then(Value::as_bool)
            .ok_or(ProtocolError::MissingField("willRetry"))?,
    })
}

fn required_object(
    value: Option<&Value>,
) -> Result<&serde_json::Map<String, Value>, ProtocolError> {
    value
        .and_then(Value::as_object)
        .ok_or(ProtocolError::MissingField("params"))
}

fn required_string(value: Option<&Value>, field: &'static str) -> Result<String, ProtocolError> {
    value
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or(ProtocolError::MissingField(field))
}

fn decode_request_id(value: &Value) -> Result<RequestId, ProtocolError> {
    let id = serde_json::from_value(value.clone()).map_err(|_| ProtocolError::InvalidRequestId)?;
    validate_request_id(&id)?;
    Ok(id)
}

fn validate_request_id(id: &RequestId) -> Result<(), ProtocolError> {
    match id {
        RequestId::String(value) if value.is_empty() => Err(ProtocolError::InvalidRequestId),
        RequestId::Number(_) | RequestId::String(_) => Ok(()),
    }
}
