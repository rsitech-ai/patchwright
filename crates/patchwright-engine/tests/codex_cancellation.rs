#[path = "support/fake_codex_app_server.rs"]
mod fake_codex_app_server;

use fake_codex_app_server::FakeCodexAppServer;
use patchwright_core::{Task, TaskState};
use patchwright_engine::EventStore;
use patchwright_engine::codex::process::{
    CodexExecutable, CodexProcessConfig, CodexProcessFactory,
};
use patchwright_engine::codex::service::CodexService;
use std::process::Command;
use std::sync::Mutex;
use tempfile::tempdir;

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

async fn service(root: &std::path::Path, body: &str, grace: std::time::Duration) -> CodexService {
    let fake = FakeCodexAppServer::create(root, "codex-cli 0.144.2", body);
    let executable = CodexExecutable::discover(Some(fake.path())).await.unwrap();
    let version = executable.version().to_owned();
    let config = CodexProcessConfig {
        shutdown_grace: grace,
        ..CodexProcessConfig::default()
    };
    CodexService::new(CodexProcessFactory::new(executable, config), version)
}

fn handshake() -> &'static str {
    r#"IFS= read -r initialize
printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"userAgent":"codex_cli_rs/0.144.2","codexHome":"/tmp/codex-home","platformFamily":"unix","platformOs":"macos"}}'
IFS= read -r initialized
IFS= read -r account
printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"account":{"type":"apiKey"},"requiresOpenaiAuth":true}}'
IFS= read -r thread
printf '%s\n' '{"jsonrpc":"2.0","id":3,"result":{"thread":{"id":"thread-cancel"}}}'"#
}

#[tokio::test]
async fn cancel_before_turn_retains_worktree_and_unrelated_process() {
    let root = tempdir().unwrap();
    let body = format!("{}\nwhile IFS= read -r line; do :; done", handshake());
    let mut service = service(root.path(), &body, std::time::Duration::from_millis(100)).await;
    let worktree = root.path().join("worktree");
    std::fs::create_dir(&worktree).unwrap();
    std::fs::write(worktree.join("evidence.txt"), "retain").unwrap();
    let mut task = Task::new("Cancel before turn", worktree.to_str().unwrap()).unwrap();
    advance_to_preparing(&mut task);
    let store = Mutex::new(EventStore::open(&root.path().join("events.sqlite")).unwrap());
    store.lock().unwrap().save_task(&task, "prepared").unwrap();
    service.start(task.id, &store).await.unwrap();
    let mut unrelated = Command::new("sleep").arg("30").spawn().unwrap();
    service.interrupt(task.id, true, &store).await.unwrap();
    assert_eq!(
        store
            .lock()
            .unwrap()
            .load_task(task.id)
            .unwrap()
            .unwrap()
            .state,
        TaskState::Cancelled
    );
    assert_eq!(
        std::fs::read_to_string(worktree.join("evidence.txt")).unwrap(),
        "retain"
    );
    assert!(
        unrelated.try_wait().unwrap().is_none(),
        "unrelated process must survive"
    );
    unrelated.kill().unwrap();
}

#[tokio::test]
async fn ignored_interrupt_times_out_terminates_owned_group_and_pause_can_resume() {
    let root = tempdir().unwrap();
    let interrupt_log = root.path().join("interrupt.log");
    let body = format!(
        r#"{}
IFS= read -r turn
printf '%s\n' '{{"jsonrpc":"2.0","id":4,"result":{{"turn":{{"id":"turn-cancel","items":[],"status":"inProgress"}}}}}}'
sleep 30 &
IFS= read -r interrupt
printf '%s\n' "$interrupt" >> '{}'
while :; do sleep 1; done"#,
        handshake(),
        interrupt_log.display()
    );
    let mut service = service(root.path(), &body, std::time::Duration::from_millis(100)).await;
    let worktree = root.path().join("worktree");
    std::fs::create_dir(&worktree).unwrap();
    let mut task = Task::new("Pause streaming turn", worktree.to_str().unwrap()).unwrap();
    advance_to_preparing(&mut task);
    let store = Mutex::new(EventStore::open(&root.path().join("events.sqlite")).unwrap());
    store.lock().unwrap().save_task(&task, "prepared").unwrap();
    service.start(task.id, &store).await.unwrap();
    service
        .start_turn(task.id, "message-1", "Work until interrupted.", &store)
        .await
        .unwrap();
    service.interrupt(task.id, false, &store).await.unwrap();
    let lines = std::fs::read_to_string(&interrupt_log).unwrap();
    assert_eq!(
        lines.lines().count(),
        1,
        "turn/interrupt must be sent exactly once"
    );
    assert!(lines.contains("turn/interrupt"));
    assert_eq!(
        store
            .lock()
            .unwrap()
            .load_task(task.id)
            .unwrap()
            .unwrap()
            .state,
        TaskState::Paused
    );
    assert!(
        service
            .start_turn(task.id, "message-after-pause", "Must not run.", &store)
            .await
            .is_err()
    );
    let resumed = service.start(task.id, &store).await.unwrap();
    assert!(resumed.can_send);
    assert_eq!(
        store
            .lock()
            .unwrap()
            .load_task(task.id)
            .unwrap()
            .unwrap()
            .state,
        TaskState::Implementing
    );
}

#[tokio::test]
async fn completion_racing_with_cancel_is_reconciled_before_terminal_cancel() {
    let root = tempdir().unwrap();
    let body = format!(
        r#"{}
IFS= read -r turn
printf '%s\n' '{{"jsonrpc":"2.0","id":4,"result":{{"turn":{{"id":"turn-race","items":[],"status":"inProgress"}}}}}}'
IFS= read -r interrupt
printf '%s\n' '{{"jsonrpc":"2.0","method":"turn/completed","params":{{"threadId":"thread-cancel","turn":{{"id":"turn-race","status":"completed"}}}}}}'
printf '%s\n' '{{"jsonrpc":"2.0","id":5,"result":{{}}}}'
while IFS= read -r line; do :; done"#,
        handshake()
    );
    let mut service = service(root.path(), &body, std::time::Duration::from_millis(100)).await;
    let worktree = root.path().join("worktree");
    std::fs::create_dir(&worktree).unwrap();
    let mut task = Task::new("Completion race", worktree.to_str().unwrap()).unwrap();
    advance_to_preparing(&mut task);
    let store = Mutex::new(EventStore::open(&root.path().join("events.sqlite")).unwrap());
    store.lock().unwrap().save_task(&task, "prepared").unwrap();
    service.start(task.id, &store).await.unwrap();
    service
        .start_turn(task.id, "message-1", "Finish now.", &store)
        .await
        .unwrap();
    service.interrupt(task.id, true, &store).await.unwrap();
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
    assert!(
        store
            .lock()
            .unwrap()
            .codex_events(task.id, 0)
            .unwrap()
            .iter()
            .any(|event| event.kind == "turnCompleted")
    );
}

#[tokio::test]
async fn child_crash_is_checkpointed_failed_without_deleting_worktree() {
    let root = tempdir().unwrap();
    let body = format!(
        r#"{}
IFS= read -r turn
printf '%s\n' '{{"jsonrpc":"2.0","id":4,"result":{{"turn":{{"id":"turn-crash","items":[],"status":"inProgress"}}}}}}'
exit 70"#,
        handshake()
    );
    let mut service = service(root.path(), &body, std::time::Duration::from_millis(100)).await;
    let worktree = root.path().join("worktree");
    std::fs::create_dir(&worktree).unwrap();
    std::fs::write(worktree.join("partial.txt"), "retained").unwrap();
    let mut task = Task::new("Crash task", worktree.to_str().unwrap()).unwrap();
    advance_to_preparing(&mut task);
    let store = Mutex::new(EventStore::open(&root.path().join("events.sqlite")).unwrap());
    store.lock().unwrap().save_task(&task, "prepared").unwrap();
    service.start(task.id, &store).await.unwrap();
    service
        .start_turn(task.id, "message-1", "Crash after accepting.", &store)
        .await
        .unwrap();
    assert!(service.events(task.id, 0, 100, &store).await.is_err());
    assert_eq!(
        store
            .lock()
            .unwrap()
            .load_task(task.id)
            .unwrap()
            .unwrap()
            .state,
        TaskState::Failed
    );
    assert_eq!(
        std::fs::read_to_string(worktree.join("partial.txt")).unwrap(),
        "retained"
    );
    assert!(
        store
            .lock()
            .unwrap()
            .codex_events(task.id, 0)
            .unwrap()
            .iter()
            .any(|event| event.summary.contains("exited unexpectedly"))
    );
}
