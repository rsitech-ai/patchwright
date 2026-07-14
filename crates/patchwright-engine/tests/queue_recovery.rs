use patchwright_core::{QueueDecision, QueueTier};
use patchwright_engine::EventStore;

#[test]
fn queue_order_reasons_and_input_identity_survive_restart() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("engine.sqlite3");
    let decisions = vec![
        QueueDecision {
            repository_full_name: "octo/fixture".into(),
            number: 2,
            tier: QueueTier::Critical,
            score: 1_250,
            reasons: vec!["Security-sensitive change".into()],
            decision_input_sha256: "a".repeat(64),
        },
        QueueDecision {
            repository_full_name: "octo/fixture".into(),
            number: 1,
            tier: QueueTier::Ready,
            score: 900,
            reasons: vec!["Approved with passing CI".into()],
            decision_input_sha256: "b".repeat(64),
        },
    ];
    let store = EventStore::open(&database).unwrap();
    store.replace_queue_decisions(&decisions).unwrap();
    drop(store);
    let reopened = EventStore::open(&database).unwrap();
    assert_eq!(reopened.queue_decisions().unwrap(), decisions);
}
