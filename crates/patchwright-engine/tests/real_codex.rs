use patchwright_core::{Task, TaskState};
use patchwright_engine::EventStore;
use patchwright_engine::codex::process::{
    CodexExecutable, CodexProcessConfig, CodexProcessFactory,
};
use patchwright_engine::codex::service::{CodexApprovalState, CodexService};
use patchwright_engine::codex::session::CodexAccountState;
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};
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

#[tokio::test]
#[ignore = "requires installed, signed-in Codex and may consume model quota"]
#[allow(clippy::too_many_lines)]
async fn real_codex_disposable_repository_lifecycle() {
    let root = tempdir().unwrap();
    let repository = root.path().join("repository");
    std::fs::create_dir(&repository).unwrap();
    std::fs::write(
        repository.join("README.md"),
        "# Disposable Patchwright Codex smoke\n",
    )
    .unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(&repository)
            .status()
            .unwrap()
            .success()
    );

    let executable = CodexExecutable::discover(None)
        .await
        .expect("installed compatible Codex");
    assert!(
        executable.version().starts_with("codex-cli 0.144."),
        "unexpected pinned Codex version: {}",
        executable.version()
    );
    let version = executable.version().to_owned();
    let factory = CodexProcessFactory::new(executable, CodexProcessConfig::default());
    let database = root.path().join("events.sqlite3");
    let store = Mutex::new(EventStore::open(&database).unwrap());
    let mut task = Task::new(
        "Create the deterministic smoke artifact",
        repository.to_str().unwrap(),
    )
    .unwrap();
    advance_to_preparing(&mut task);
    store
        .lock()
        .unwrap()
        .save_task(&task, "real Codex smoke prepared")
        .unwrap();

    let mut service = CodexService::new(factory.clone(), version.clone());
    let started = service
        .start(task.id, &store)
        .await
        .expect("start real Codex");
    assert_eq!(
        started.account_state,
        Some(CodexAccountState::SignedIn),
        "Codex account must be signed in"
    );
    let original_generation = started.process_generation.unwrap();
    let thread_id = started.thread_id.clone().expect("real thread id");
    let receipt = service.start_turn(
        task.id,
        "patchwright-real-smoke-1",
        "Create a new file named result.txt containing exactly PATCHWRIGHT_CODEX_SMOKE followed by one newline. Do not modify any other file. Then run `test \"$(cat result.txt)\" = PATCHWRIGHT_CODEX_SMOKE` to verify it.",
        &store,
    ).await.expect("start real turn");

    let deadline = Instant::now() + Duration::from_secs(180);
    let mut approval_count = 0;
    loop {
        for approval in service
            .approvals(task.id, &store)
            .await
            .expect("poll real approvals")
        {
            if approval.state == CodexApprovalState::Pending {
                service
                    .resolve_approval(
                        task.id,
                        approval.id,
                        approval.process_generation,
                        true,
                        &store,
                    )
                    .await
                    .expect("approve exact disposable request");
                approval_count += 1;
            }
        }
        let events = service
            .events(task.id, 0, 500, &store)
            .await
            .expect("poll real events");
        if events.iter().any(|event| {
            event.kind == "turnCompleted" && event.turn_id.as_deref() == Some(&receipt.turn_id)
        }) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "real Codex turn timed out; approvals handled: {approval_count}"
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    assert_eq!(
        std::fs::read_to_string(repository.join("result.txt")).unwrap(),
        "PATCHWRIGHT_CODEX_SMOKE\n"
    );
    let events = store.lock().unwrap().codex_events(task.id, 0).unwrap();
    assert!(
        events
            .iter()
            .any(|event| event.kind == "textDelta" || event.kind == "itemCompleted")
    );
    assert!(events.iter().any(|event| event.kind == "turnCompleted"));
    eprintln!(
        "real Codex smoke: version={version}, approvals={approval_count}, persisted_events={}",
        events.len()
    );

    service
        .interrupt(task.id, false, &store)
        .await
        .expect("pause after completed turn");
    drop(service);
    drop(store);
    let reopened = Mutex::new(EventStore::open(&database).unwrap());
    let mut restarted = CodexService::new(factory, version);
    let resumed = restarted
        .start(task.id, &reopened)
        .await
        .expect("resume real Codex thread");
    assert_eq!(resumed.thread_id.as_deref(), Some(thread_id.as_str()));
    assert_ne!(resumed.process_generation.unwrap(), original_generation);
    restarted
        .interrupt(task.id, true, &reopened)
        .await
        .expect("cancel resumed real task");
    assert_eq!(
        reopened
            .lock()
            .unwrap()
            .load_task(task.id)
            .unwrap()
            .unwrap()
            .state,
        TaskState::Cancelled
    );
}
