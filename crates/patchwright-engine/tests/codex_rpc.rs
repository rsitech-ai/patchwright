#[path = "support/fake_codex_app_server.rs"]
mod fake_codex_app_server;

use fake_codex_app_server::FakeCodexAppServer;
use patchwright_core::{Task, TaskState};
use patchwright_engine::codex::process::{
    CodexExecutable, CodexProcessConfig, CodexProcessFactory,
};
use patchwright_engine::codex::service::{CodexService, CodexServiceError, CodexServiceState};
use patchwright_engine::{EventStore, serve_with_codex};
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, sync::Mutex};
use tempfile::tempdir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

async fn rpc_call(stream: &mut BufReader<UnixStream>, request: Value) -> Value {
    stream
        .get_mut()
        .write_all(&serde_json::to_vec(&request).unwrap())
        .await
        .unwrap();
    stream.get_mut().write_all(b"\n").await.unwrap();
    let mut line = String::new();
    stream.read_line(&mut line).await.unwrap();
    serde_json::from_str(&line).unwrap()
}

fn advance_to_preparing(task: &mut Task) {
    for state in [
        TaskState::Assessing,
        TaskState::Planned,
        TaskState::AwaitingPreparationApproval,
        TaskState::Preparing,
    ] {
        task.transition(state).unwrap();
    }
}

fn streaming_server_body() -> &'static str {
    r#"IFS= read -r initialize
printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"userAgent":"codex_cli_rs/0.144.2","codexHome":"/tmp/codex-home","platformFamily":"unix","platformOs":"macos"}}'
IFS= read -r initialized
IFS= read -r account
printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"account":null,"requiresOpenaiAuth":true}}'
IFS= read -r thread
printf '%s\n' '{"jsonrpc":"2.0","id":3,"result":{"thread":{"id":"thread-rpc"}}}'
IFS= read -r turn
printf '%s\n' '{"jsonrpc":"2.0","id":4,"result":{"turn":{"id":"turn-rpc","items":[],"itemsView":"full","status":"inProgress","error":null,"startedAt":1783987200,"completedAt":null,"durationMs":null}}}'
printf '%s\n' '{"jsonrpc":"2.0","method":"item/started","params":{"threadId":"thread-rpc","turnId":"turn-rpc","startedAtMs":1783987200000,"item":{"type":"agentMessage","id":"item-text","text":"","phase":null,"memoryCitation":null}}}'
printf '%s\n' '{"jsonrpc":"2.0","method":"item/agentMessage/delta","params":{"threadId":"thread-rpc","turnId":"turn-rpc","itemId":"item-text","delta":"Hello from Codex"}}'
printf '%s\n' '{"jsonrpc":"2.0","method":"item/reasoning/summaryTextDelta","params":{"threadId":"thread-rpc","turnId":"turn-rpc","itemId":"item-reasoning","delta":"Checked the contract","summaryIndex":0}}'
printf '%s\n' '{"jsonrpc":"2.0","method":"item/commandExecution/outputDelta","params":{"threadId":"thread-rpc","turnId":"turn-rpc","itemId":"item-command","delta":"tests passed"}}'
printf '%s\n' '{"jsonrpc":"2.0","method":"item/fileChange/outputDelta","params":{"threadId":"thread-rpc","turnId":"turn-rpc","itemId":"item-file","delta":"M Sources/App.swift"}}'
printf '%s\n' '{"jsonrpc":"2.0","method":"item/completed","params":{"threadId":"thread-rpc","turnId":"turn-rpc","completedAtMs":1783987201000,"item":{"type":"agentMessage","id":"item-text","text":"Hello from Codex","phase":"final_answer","memoryCitation":null}}}'
printf '%s\n' '{"jsonrpc":"2.0","method":"turn/completed","params":{"threadId":"thread-rpc","turn":{"id":"turn-rpc","items":[],"itemsView":"full","status":"completed","error":null,"startedAt":1783987200,"completedAt":1783987201,"durationMs":1000}}}'
IFS= read -r steer
printf '%s\n' '{"jsonrpc":"2.0","id":5,"result":{"turnId":"turn-rpc"}}'
while IFS= read -r line; do :; done"#
}

#[tokio::test]
async fn starts_turns_persists_streamed_events_and_steers_the_active_turn() {
    let root = tempdir().unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let fake =
        FakeCodexAppServer::create(root.path(), "codex-cli 0.144.2", streaming_server_body());
    let executable = CodexExecutable::discover(Some(fake.path())).await.unwrap();
    let version = executable.version().to_owned();
    let factory = CodexProcessFactory::new(executable, CodexProcessConfig::default());
    let worktree = root.path().join("worktree");
    std::fs::create_dir(&worktree).unwrap();
    let mut task = Task::new("Stream a turn", worktree.to_str().unwrap()).unwrap();
    advance_to_preparing(&mut task);
    let store = Mutex::new(EventStore::open(&root.path().join("events.sqlite")).unwrap());
    store.lock().unwrap().save_task(&task, "prepared").unwrap();
    let mut service = CodexService::new(factory, version);

    assert_eq!(
        service.status(task.id, &store).unwrap().state,
        CodexServiceState::NotStarted
    );
    let started = service.start(task.id, &store).await.unwrap();
    assert_eq!(started.state, CodexServiceState::Ready);
    let turn = service
        .start_turn(
            task.id,
            "message-1",
            "Implement the approved slice.",
            &store,
        )
        .await
        .unwrap();
    assert_eq!(turn.turn_id, "turn-rpc");

    let steered = service
        .steer_turn(
            task.id,
            "message-2",
            "Preserve the regression test.",
            &store,
        )
        .await
        .unwrap();
    assert_eq!(steered.turn_id, "turn-rpc");

    let events = service.events(task.id, 0, 100, &store).await.unwrap();
    let kinds = events
        .iter()
        .map(|event| event.kind.as_str())
        .collect::<Vec<_>>();
    for expected in [
        "userMessage",
        "itemStarted",
        "textDelta",
        "reasoningDelta",
        "commandOutputDelta",
        "fileChangeDelta",
        "itemCompleted",
        "turnCompleted",
    ] {
        assert!(kinds.contains(&expected), "missing {expected}: {kinds:?}");
    }
    assert!(
        events
            .windows(2)
            .all(|pair| pair[0].sequence < pair[1].sequence)
    );
    let cursor = events.last().unwrap().sequence;
    assert!(
        service
            .events(task.id, cursor, 100, &store)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store
            .lock()
            .unwrap()
            .load_task(task.id)
            .unwrap()
            .unwrap()
            .state,
        TaskState::Verifying
    );

    service.stop(task.id).await.unwrap();
}

#[tokio::test]
async fn rejects_invalid_task_input_duplicate_message_and_event_page_boundaries() {
    let root = tempdir().unwrap();
    let fake =
        FakeCodexAppServer::create(root.path(), "codex-cli 0.144.2", streaming_server_body());
    let executable = CodexExecutable::discover(Some(fake.path())).await.unwrap();
    let version = executable.version().to_owned();
    let factory = CodexProcessFactory::new(executable, CodexProcessConfig::default());
    let worktree = root.path().join("worktree");
    std::fs::create_dir(&worktree).unwrap();
    let store = Mutex::new(EventStore::open(&root.path().join("events.sqlite")).unwrap());
    let discovered = Task::new("Not prepared", worktree.to_str().unwrap()).unwrap();
    store
        .lock()
        .unwrap()
        .save_task(&discovered, "discovered")
        .unwrap();
    let mut service = CodexService::new(factory, version);
    assert!(matches!(
        service.start(discovered.id, &store).await,
        Err(CodexServiceError::InvalidTaskState(TaskState::Discovered))
    ));

    let mut task = Task::new("Prepared", worktree.to_str().unwrap()).unwrap();
    advance_to_preparing(&mut task);
    store.lock().unwrap().save_task(&task, "prepared").unwrap();
    service.start(task.id, &store).await.unwrap();
    assert!(matches!(
        service.start_turn(task.id, "", "message", &store).await,
        Err(CodexServiceError::InvalidInput)
    ));
    assert!(matches!(
        service
            .start_turn(task.id, "too-large", &"x".repeat(64 * 1024 + 1), &store)
            .await,
        Err(CodexServiceError::InvalidInput)
    ));
    service
        .start_turn(task.id, "message-1", "Implement.", &store)
        .await
        .unwrap();
    assert!(matches!(
        service
            .start_turn(task.id, "message-1", "Duplicate.", &store)
            .await,
        Err(CodexServiceError::DuplicateClientMessageId)
    ));
    assert!(matches!(
        service.events(task.id, 0, 0, &store).await,
        Err(CodexServiceError::InvalidEventLimit)
    ));
    service.stop(task.id).await.unwrap();
}

#[tokio::test]
async fn unix_rpc_exposes_status_start_turn_steer_and_cursor_events() {
    let root = tempdir().unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let fake =
        FakeCodexAppServer::create(root.path(), "codex-cli 0.144.2", streaming_server_body());
    let executable = CodexExecutable::discover(Some(fake.path())).await.unwrap();
    let version = executable.version().to_owned();
    let factory = CodexProcessFactory::new(executable, CodexProcessConfig::default());
    let database = root.path().join("events.sqlite");
    let socket = root.path().join("engine.sock");
    let worktree = root.path().join("worktree");
    std::fs::create_dir(&worktree).unwrap();
    let mut task = Task::new("RPC task", worktree.to_str().unwrap()).unwrap();
    advance_to_preparing(&mut task);
    EventStore::open(&database)
        .unwrap()
        .save_task(&task, "prepared")
        .unwrap();
    let server_socket = socket.clone();
    let server_database = database.clone();
    let server = tokio::spawn(async move {
        serve_with_codex(&server_socket, &server_database, factory, version).await
    });
    for _ in 0..100 {
        if socket.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let mut stream = BufReader::new(UnixStream::connect(&socket).await.unwrap());
    let task_id = task.id.to_string();

    let status = rpc_call(
        &mut stream,
        json!({"jsonrpc":"2.0","id":1,"method":"codex.status","params":{"taskId":task_id}}),
    )
    .await;
    assert_eq!(status["result"]["state"], "notStarted");
    let started = rpc_call(
        &mut stream,
        json!({"jsonrpc":"2.0","id":2,"method":"codex.start","params":{"taskId":task_id}}),
    )
    .await;
    assert_eq!(started["result"]["state"], "ready");
    let turn = rpc_call(
        &mut stream,
        json!({
            "jsonrpc":"2.0","id":3,"method":"codex.turn.start",
            "params":{"taskId":task_id,"clientMessageId":"rpc-message-1","input":"Implement."}
        }),
    )
    .await;
    assert_eq!(turn["result"]["turnId"], "turn-rpc");
    let steer = rpc_call(
        &mut stream,
        json!({
            "jsonrpc":"2.0","id":4,"method":"codex.turn.steer",
            "params":{"taskId":task_id,"clientMessageId":"rpc-message-2","input":"Keep tests."}
        }),
    )
    .await;
    assert_eq!(steer["result"]["turnId"], "turn-rpc");
    let events = rpc_call(
        &mut stream,
        json!({
            "jsonrpc":"2.0","id":5,"method":"codex.events",
            "params":{"taskId":task_id,"after":"0","limit":"100"}
        }),
    )
    .await;
    let events = events["result"].as_array().unwrap();
    assert!(events.iter().any(|event| event["kind"] == "textDelta"));
    let cursor = events.last().unwrap()["sequence"].as_u64().unwrap();
    let after = rpc_call(
        &mut stream,
        json!({
            "jsonrpc":"2.0","id":6,"method":"codex.events",
            "params":{"taskId":task_id,"after":cursor.to_string(),"limit":"100"}
        }),
    )
    .await;
    assert!(after["result"].as_array().unwrap().is_empty());
    server.abort();
}
