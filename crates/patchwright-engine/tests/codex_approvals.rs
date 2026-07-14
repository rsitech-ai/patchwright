#[path = "support/fake_codex_app_server.rs"]
mod fake_codex_app_server;

use chrono::{Duration, Utc};
use fake_codex_app_server::FakeCodexAppServer;
use patchwright_core::{ApprovalClass, Task, TaskState};
use patchwright_engine::EventStore;
use patchwright_engine::codex::process::{
    CodexExecutable, CodexProcessConfig, CodexProcessFactory,
};
use patchwright_engine::codex::service::{
    CodexApprovalKind, CodexApprovalState, CodexService, CodexServiceError,
};
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

fn approval_server_body() -> &'static str {
    r#"IFS= read -r initialize
printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"userAgent":"codex_cli_rs/0.144.2","codexHome":"/tmp/codex-home","platformFamily":"unix","platformOs":"macos"}}'
IFS= read -r initialized
IFS= read -r account
printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"account":{"type":"apiKey"},"requiresOpenaiAuth":true}}'
IFS= read -r thread
printf '%s\n' '{"jsonrpc":"2.0","id":3,"result":{"thread":{"id":"thread-approval"}}}'
IFS= read -r turn
printf '%s\n' '{"jsonrpc":"2.0","id":4,"result":{"turn":{"id":"turn-approval","items":[],"status":"inProgress"}}}'
printf '%s\n' '{"jsonrpc":"2.0","id":"command-request","method":"item/commandExecution/requestApproval","params":{"threadId":"thread-approval","turnId":"turn-approval","itemId":"command-item","startedAtMs":1783987200100,"reason":"Run focused tests","command":"cargo test -p patchwright-engine","cwd":"/tmp/worktree"}}'
printf '%s\n' '{"jsonrpc":"2.0","id":"file-request","method":"item/fileChange/requestApproval","params":{"threadId":"thread-approval","turnId":"turn-approval","itemId":"file-item","startedAtMs":1783987200200,"reason":"Apply reviewed patch","grantRoot":null}}'
while IFS= read -r response; do :; done"#
}

async fn harness() -> (
    tempfile::TempDir,
    Mutex<EventStore>,
    CodexService,
    CodexProcessFactory,
    String,
    Task,
) {
    let root = tempdir().unwrap();
    let fake = FakeCodexAppServer::create(root.path(), "codex-cli 0.144.2", approval_server_body());
    let executable = CodexExecutable::discover(Some(fake.path())).await.unwrap();
    let version = executable.version().to_owned();
    let factory = CodexProcessFactory::new(executable, CodexProcessConfig::default());
    let worktree = root.path().join("worktree");
    std::fs::create_dir(&worktree).unwrap();
    let mut task = Task::new("Approval task", worktree.to_str().unwrap()).unwrap();
    advance_to_preparing(&mut task);
    let store = Mutex::new(EventStore::open(&root.path().join("events.sqlite")).unwrap());
    store.lock().unwrap().save_task(&task, "prepared").unwrap();
    (
        root,
        store,
        CodexService::new(factory.clone(), version.clone()),
        factory,
        version,
        task,
    )
}

#[tokio::test]
async fn command_and_file_requests_are_exact_codex_runtime_approvals_only() {
    let (_root, store, mut service, _factory, _version, task) = harness().await;
    service.start(task.id, &store).await.unwrap();
    service
        .start_turn(task.id, "message-1", "Implement.", &store)
        .await
        .unwrap();
    let approvals = service.approvals(task.id, &store).await.unwrap();
    assert_eq!(approvals.len(), 2);
    assert!(
        approvals
            .iter()
            .all(|value| value.class == ApprovalClass::CodexRuntime)
    );
    assert_eq!(
        approvals[0].process_generation,
        service
            .status(task.id, &store)
            .unwrap()
            .process_generation
            .unwrap()
    );
    assert_eq!(
        approvals.iter().map(|value| value.kind).collect::<Vec<_>>(),
        [CodexApprovalKind::Command, CodexApprovalKind::FileChange]
    );
    assert!(
        approvals
            .iter()
            .all(|value| value.thread_id == "thread-approval" && value.turn_id == "turn-approval")
    );
    assert!(
        approvals
            .iter()
            .all(|value| value.state == CodexApprovalState::Pending)
    );

    let generation = approvals[0].process_generation;
    let approved = service
        .resolve_approval(task.id, approvals[0].id, generation, true, &store)
        .await
        .unwrap();
    assert_eq!(approved.state, CodexApprovalState::Approved);
    let duplicate = service
        .resolve_approval(task.id, approvals[0].id, generation, false, &store)
        .await
        .unwrap();
    assert_eq!(
        duplicate.state,
        CodexApprovalState::Approved,
        "a duplicate decision must be idempotent"
    );
    let declined = service
        .resolve_approval(task.id, approvals[1].id, generation, false, &store)
        .await
        .unwrap();
    assert_eq!(declined.state, CodexApprovalState::Declined);
}

#[tokio::test]
async fn expired_and_wrong_generation_approvals_fail_closed_and_survive_restart() {
    let (root, store, mut service, factory, version, task) = harness().await;
    service.start(task.id, &store).await.unwrap();
    service
        .start_turn(task.id, "message-1", "Implement.", &store)
        .await
        .unwrap();
    let approvals = service.approvals(task.id, &store).await.unwrap();
    let mut expired = approvals[0].clone();
    expired.expires_at = Utc::now() - Duration::seconds(1);
    store
        .lock()
        .unwrap()
        .save_codex_runtime_approval(&expired)
        .unwrap();
    assert!(matches!(
        service
            .resolve_approval(
                task.id,
                expired.id,
                expired.process_generation,
                true,
                &store
            )
            .await,
        Err(CodexServiceError::ApprovalInvalid)
    ));
    assert_eq!(
        store
            .lock()
            .unwrap()
            .codex_runtime_approval(expired.id)
            .unwrap()
            .unwrap()
            .state,
        CodexApprovalState::Expired
    );

    let pending = approvals[1].clone();
    service.stop(task.id).await.unwrap();
    drop(store);
    let reopened = Mutex::new(EventStore::open(&root.path().join("events.sqlite")).unwrap());
    assert_eq!(
        reopened
            .lock()
            .unwrap()
            .codex_runtime_approval(pending.id)
            .unwrap()
            .unwrap(),
        pending
    );
    let mut restarted = CodexService::new(factory, version);
    let status = restarted.start(task.id, &reopened).await.unwrap();
    assert_ne!(
        status.process_generation.unwrap(),
        pending.process_generation
    );
    assert!(matches!(
        restarted
            .resolve_approval(
                task.id,
                pending.id,
                pending.process_generation,
                true,
                &reopened
            )
            .await,
        Err(CodexServiceError::ApprovalInvalid)
    ));
    assert_eq!(
        reopened
            .lock()
            .unwrap()
            .codex_runtime_approval(pending.id)
            .unwrap()
            .unwrap()
            .state,
        CodexApprovalState::Invalidated
    );
}
