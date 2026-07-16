use patchwright_core::{Task, TaskState};
use patchwright_engine::EventStore;

#[test]
fn restart_replays_task_and_deduplicates_delivery() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("patchwright.sqlite3");
    let mut task = Task::new("Fix reconciliation", "/tmp/repository").unwrap();
    task.transition(TaskState::Assessing).unwrap();
    task.transition(TaskState::Planned).unwrap();

    {
        let store = EventStore::open(&database).unwrap();
        store.save_task(&task, "plan created").unwrap();
        assert!(store.claim_delivery("task-1:create-pr:abc123").unwrap());
        store
            .complete_delivery("task-1:create-pr:abc123", "pr:42")
            .unwrap();
    }

    let reopened = EventStore::open(&database).unwrap();
    let loaded = reopened.load_task(task.id).unwrap().unwrap();
    assert_eq!(loaded.state, TaskState::Planned);
    assert_eq!(reopened.timeline(task.id).unwrap().len(), 1);
    assert!(!reopened.claim_delivery("task-1:create-pr:abc123").unwrap());
    assert_eq!(
        reopened
            .delivery_result("task-1:create-pr:abc123")
            .unwrap()
            .as_deref(),
        Some("pr:42")
    );
}
