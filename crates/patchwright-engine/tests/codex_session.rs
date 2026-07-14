#[path = "support/fake_codex_app_server.rs"]
mod fake_codex_app_server;

use fake_codex_app_server::FakeCodexAppServer;
use patchwright_core::{Task, TaskState};
use patchwright_engine::codex::process::{
    CodexExecutable, CodexProcessConfig, CodexProcessFactory, CodexProcessState,
};
use patchwright_engine::codex::session::{
    CodexAccountState, CodexSession, CodexSessionStatus, ThreadBootstrap,
};
use patchwright_engine::{EventStore, TaskCheckpoint};
use std::sync::Mutex;
use tempfile::tempdir;

fn protocol_body(
    account_result: &str,
    thread_result: &str,
    expected_thread_method: &str,
) -> String {
    format!(
        r#"IFS= read -r initialize
case "$initialize" in *'"method":"initialize"'*) ;; *) exit 70 ;; esac
printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"userAgent":"codex_cli_rs/0.144.2","codexHome":"/tmp/codex-home","platformFamily":"unix","platformOs":"macos"}}}}'
IFS= read -r initialized
case "$initialized" in *'"method":"initialized"'*) ;; *) exit 71 ;; esac
IFS= read -r account
case "$account" in *'"method":"account/read"'*) ;; *) exit 72 ;; esac
printf '%s\n' '{account_result}'
IFS= read -r thread
case "$thread" in *'"method":"{expected_thread_method}"'*) ;; *) exit 73 ;; esac
printf '%s\n' '{thread_result}'
while IFS= read -r line; do :; done"#
    )
}

fn ready_thread_result(thread_id: &str) -> String {
    format!(r#"{{"jsonrpc":"2.0","id":3,"result":{{"thread":{{"id":"{thread_id}"}}}}}}"#)
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

#[tokio::test]
async fn initializes_in_order_persists_account_and_atomically_enters_implementing() {
    let root = tempdir().unwrap();
    let body = protocol_body(
        r#"{"jsonrpc":"2.0","id":2,"result":{"account":{"type":"chatgpt","email":"operator@example.invalid","planType":"team"},"requiresOpenaiAuth":true}}"#,
        &ready_thread_result("thread-new"),
        "thread/start",
    );
    let fake = FakeCodexAppServer::create(root.path(), "codex-cli 0.144.2", &body);
    let executable = CodexExecutable::discover(Some(fake.path())).await.unwrap();
    let version = executable.version().to_owned();
    let factory = CodexProcessFactory::new(executable, CodexProcessConfig::default());
    let worktree = root.path().join("worktree");
    std::fs::create_dir(&worktree).unwrap();
    let mut process = factory.launch("task", &worktree).unwrap();
    let store = Mutex::new(EventStore::open(&root.path().join("events.sqlite")).unwrap());
    let mut task = Task::new("Codex task", worktree.to_str().unwrap()).unwrap();
    advance_to_preparing(&mut task);
    store
        .lock()
        .unwrap()
        .save_task(&task, "worktree prepared")
        .unwrap();

    let session = CodexSession::connect(
        task.id,
        &mut process,
        &store,
        &version,
        ThreadBootstrap::Start {
            instructions: "Implement the approved task contract.".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(session.status(), CodexSessionStatus::Ready);
    assert_eq!(session.account_state(), CodexAccountState::SignedIn);
    assert_eq!(session.thread_id(), Some("thread-new"));
    assert_eq!(process.state(), CodexProcessState::Ready);
    assert_eq!(
        store
            .lock()
            .unwrap()
            .load_task(task.id)
            .unwrap()
            .unwrap()
            .state,
        TaskState::Preparing
    );

    task.transition(TaskState::Implementing).unwrap();
    let checkpoint = TaskCheckpoint::new(task.id, task.state, "Codex thread ready").unwrap();
    store
        .lock()
        .unwrap()
        .enter_implementing_with_codex(&task, &checkpoint, session.record())
        .unwrap();
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
    assert_eq!(
        store.lock().unwrap().codex_session(task.id).unwrap(),
        Some(session.record().clone())
    );
    assert!(
        store
            .lock()
            .unwrap()
            .codex_events(task.id, 0)
            .unwrap()
            .len()
            >= 4
    );
    process.terminate().await.unwrap();
}

#[tokio::test]
async fn records_signed_out_and_unavailable_account_states() {
    for (name, account_result, expected) in [
        (
            "signed-out",
            r#"{"jsonrpc":"2.0","id":2,"result":{"account":null,"requiresOpenaiAuth":true}}"#,
            CodexAccountState::SignedOut,
        ),
        (
            "unavailable",
            r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32000,"message":"account unavailable"}}"#,
            CodexAccountState::Unavailable,
        ),
    ] {
        let root = tempdir().unwrap();
        let body = protocol_body(
            account_result,
            &ready_thread_result("thread-account"),
            "thread/start",
        );
        let fake = FakeCodexAppServer::create(root.path(), "codex-cli 0.144.2", &body);
        let executable = CodexExecutable::discover(Some(fake.path())).await.unwrap();
        let version = executable.version().to_owned();
        let factory = CodexProcessFactory::new(executable, CodexProcessConfig::default());
        let worktree = root.path().join("worktree");
        std::fs::create_dir(&worktree).unwrap();
        let mut process = factory.launch(name, &worktree).unwrap();
        let store = Mutex::new(EventStore::open(&root.path().join("events.sqlite")).unwrap());
        let task = Task::new(name, worktree.to_str().unwrap()).unwrap();
        let session = CodexSession::connect(
            task.id,
            &mut process,
            &store,
            &version,
            ThreadBootstrap::Start {
                instructions: "Implement.".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(session.account_state(), expected);
        assert_eq!(session.status(), CodexSessionStatus::Ready);
        process.terminate().await.unwrap();
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn resumes_a_saved_thread_after_restart_and_requires_confirmation_when_stale() {
    let root = tempdir().unwrap();
    let database = root.path().join("events.sqlite");
    let worktree = root.path().join("worktree");
    std::fs::create_dir(&worktree).unwrap();
    let task = Task::new("Resume", worktree.to_str().unwrap()).unwrap();

    let first_root = tempdir().unwrap();
    let first_body = protocol_body(
        r#"{"jsonrpc":"2.0","id":2,"result":{"account":null,"requiresOpenaiAuth":true}}"#,
        &ready_thread_result("thread-saved"),
        "thread/start",
    );
    let fake = FakeCodexAppServer::create(first_root.path(), "codex-cli 0.144.2", &first_body);
    let executable = CodexExecutable::discover(Some(fake.path())).await.unwrap();
    let version = executable.version().to_owned();
    let factory = CodexProcessFactory::new(executable, CodexProcessConfig::default());
    let mut process = factory.launch("first", &worktree).unwrap();
    let store = Mutex::new(EventStore::open(&database).unwrap());
    let first = CodexSession::connect(
        task.id,
        &mut process,
        &store,
        &version,
        ThreadBootstrap::Start {
            instructions: "Implement.".into(),
        },
    )
    .await
    .unwrap();
    let first_generation = first.process_generation();
    process.terminate().await.unwrap();
    drop(store);

    let resumed_root = tempdir().unwrap();
    let resumed_body = protocol_body(
        r#"{"jsonrpc":"2.0","id":2,"result":{"account":null,"requiresOpenaiAuth":true}}"#,
        &ready_thread_result("thread-saved"),
        "thread/resume",
    );
    let fake = FakeCodexAppServer::create(resumed_root.path(), "codex-cli 0.144.2", &resumed_body);
    let executable = CodexExecutable::discover(Some(fake.path())).await.unwrap();
    let version = executable.version().to_owned();
    let factory = CodexProcessFactory::new(executable, CodexProcessConfig::default());
    let mut process = factory.launch("resumed", &worktree).unwrap();
    let store = Mutex::new(EventStore::open(&database).unwrap());
    let saved = store
        .lock()
        .unwrap()
        .codex_session(task.id)
        .unwrap()
        .unwrap();
    let resumed = CodexSession::connect(
        task.id,
        &mut process,
        &store,
        &version,
        ThreadBootstrap::Resume {
            thread_id: saved.thread_id.unwrap(),
        },
    )
    .await
    .unwrap();
    assert_eq!(resumed.status(), CodexSessionStatus::Ready);
    assert_ne!(resumed.process_generation(), first_generation);
    process.terminate().await.unwrap();

    let stale_root = tempdir().unwrap();
    let stale_body = protocol_body(
        r#"{"jsonrpc":"2.0","id":2,"result":{"account":null,"requiresOpenaiAuth":true}}"#,
        r#"{"jsonrpc":"2.0","id":3,"error":{"code":-32602,"message":"thread not found"}}"#,
        "thread/resume",
    );
    let fake = FakeCodexAppServer::create(stale_root.path(), "codex-cli 0.144.2", &stale_body);
    let executable = CodexExecutable::discover(Some(fake.path())).await.unwrap();
    let version = executable.version().to_owned();
    let factory = CodexProcessFactory::new(executable, CodexProcessConfig::default());
    let mut process = factory.launch("stale", &worktree).unwrap();
    let stale = CodexSession::connect(
        task.id,
        &mut process,
        &store,
        &version,
        ThreadBootstrap::Resume {
            thread_id: "thread-saved".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        stale.status(),
        CodexSessionStatus::StaleThreadNeedsConfirmation
    );
    assert_eq!(process.state(), CodexProcessState::Starting);
    let mut preparing = task.clone();
    advance_to_preparing(&mut preparing);
    preparing.transition(TaskState::Implementing).unwrap();
    let checkpoint = TaskCheckpoint::new(preparing.id, preparing.state, "not ready").unwrap();
    assert!(
        store
            .lock()
            .unwrap()
            .enter_implementing_with_codex(&preparing, &checkpoint, stale.record())
            .is_err()
    );
    process.terminate().await.unwrap();
}
