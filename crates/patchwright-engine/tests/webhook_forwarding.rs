use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use chrono::{TimeZone, Utc};
use hmac::{Hmac, Mac};
use patchwright_engine::{EventStore, MonitorRecord, MonitorState, serve_until};
use patchwright_relay::{RelayState, router};
use sha2::Sha256;
use std::{fs, os::unix::fs::PermissionsExt, path::Path, time::Duration};
use tempfile::TempDir;
use tokio::sync::oneshot;
use tower::ServiceExt;

fn signature(secret: &[u8], body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).unwrap();
    mac.update(body);
    format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
}

fn webhook_request(secret: &[u8], delivery: &str, body: &'static [u8]) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/webhooks/github")
        .header("x-github-event", "pull_request")
        .header("x-github-delivery", delivery)
        .header("x-hub-signature-256", signature(secret, body))
        .body(Body::from(body))
        .unwrap()
}

fn monitor(repository: &str, number: u64) -> MonitorRecord {
    MonitorRecord::new(
        patchwright_core::TaskId::new(),
        repository,
        number,
        "b".repeat(40),
        "a".repeat(40),
        Utc.with_ymd_and_hms(2026, 7, 17, 8, 0, 0).unwrap(),
        2,
    )
    .unwrap()
}

async fn wait_for_socket(path: &Path) {
    for _ in 0..100 {
        if path.exists() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("engine socket was not created");
}

#[tokio::test]
async fn pending_delivery_survives_relay_outage_and_wakes_only_the_exact_monitor() {
    let temporary = TempDir::new().unwrap();
    fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let relay_database = temporary.path().join("relay.sqlite");
    let engine_database = temporary.path().join("engine.sqlite");
    let engine_socket = temporary.path().join("engine.sock");
    let secret = b"forwarding-test-secret";
    let body = br#"{"action":"synchronize","pull_request":{"number":42,"title":"private title"},"repository":{"id":1,"full_name":"octocat/example"}}"#;

    let relay = RelayState::open(secret.to_vec(), &relay_database).unwrap();
    let accepted = router(relay.clone())
        .oneshot(webhook_request(secret, "delivery-outage", body))
        .await
        .unwrap();
    assert_eq!(accepted.status(), StatusCode::ACCEPTED);

    let unavailable = relay.forward_pending_once(&engine_socket).await.unwrap();
    assert_eq!(unavailable.attempted, 1);
    assert_eq!(unavailable.forwarded, 0);
    assert_eq!(relay.pending_delivery_count().unwrap(), 1);
    drop(relay);

    let store = EventStore::open(&engine_database).unwrap();
    let exact = monitor("octocat/example", 42);
    let other_pr = monitor("octocat/example", 43);
    let other_repository = monitor("octocat/other", 42);
    store.save_monitor(&exact).unwrap();
    store.save_monitor(&other_pr).unwrap();
    store.save_monitor(&other_repository).unwrap();
    drop(store);

    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let socket = engine_socket.clone();
    let database = engine_database.clone();
    let engine = tokio::spawn(async move {
        serve_until(&socket, &database, async {
            let _ = shutdown_rx.await;
        })
        .await
    });
    wait_for_socket(&engine_socket).await;

    let reopened = RelayState::open(secret.to_vec(), &relay_database).unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let forwarded = reopened.forward_pending_once(&engine_socket).await.unwrap();
    assert_eq!(forwarded.attempted, 1);
    assert_eq!(forwarded.forwarded, 1);
    assert_eq!(reopened.pending_delivery_count().unwrap(), 0);

    shutdown_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), engine)
        .await
        .expect("engine shutdown timed out")
        .unwrap()
        .unwrap();

    let store = EventStore::open(&engine_database).unwrap();
    assert_eq!(
        store.monitor(exact.id).unwrap().unwrap().summary,
        "webhook requested an immediate refresh"
    );
    assert_eq!(
        store.monitor(other_pr.id).unwrap().unwrap().summary,
        "waiting for first remote observation"
    );
    assert_eq!(
        store.monitor(other_repository.id).unwrap().unwrap().summary,
        "waiting for first remote observation"
    );
    assert_eq!(store.github_webhook_delivery_count().unwrap(), 1);
}

#[tokio::test]
async fn engine_deduplicates_across_relay_databases_and_restart() {
    let temporary = TempDir::new().unwrap();
    fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let engine_database = temporary.path().join("engine.sqlite");
    let engine_socket = temporary.path().join("engine.sock");
    let secret = b"dedupe-test-secret";
    let body = br#"{"action":"opened","pull_request":{"number":42,"title":"private title"},"repository":{"id":1,"full_name":"octocat/example"}}"#;

    let exact = monitor("octocat/example", 42);
    let store = EventStore::open(&engine_database).unwrap();
    store.save_monitor(&exact).unwrap();
    drop(store);

    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let socket = engine_socket.clone();
    let database = engine_database.clone();
    let engine = tokio::spawn(async move {
        serve_until(&socket, &database, async {
            let _ = shutdown_rx.await;
        })
        .await
    });
    wait_for_socket(&engine_socket).await;

    for name in ["relay-one.sqlite", "relay-two.sqlite"] {
        let relay = RelayState::open(secret.to_vec(), temporary.path().join(name)).unwrap();
        let accepted = router(relay.clone())
            .oneshot(webhook_request(secret, "same-delivery-id", body))
            .await
            .unwrap();
        assert_eq!(accepted.status(), StatusCode::ACCEPTED);
        let result = relay.forward_pending_once(&engine_socket).await.unwrap();
        assert_eq!(result.forwarded, 1);
    }

    shutdown_tx.send(()).unwrap();
    engine.await.unwrap().unwrap();
    let reopened = EventStore::open(&engine_database).unwrap();
    assert_eq!(reopened.github_webhook_delivery_count().unwrap(), 1);
    assert_eq!(
        reopened.monitor(exact.id).unwrap().unwrap().state,
        MonitorState::Pending
    );
}
