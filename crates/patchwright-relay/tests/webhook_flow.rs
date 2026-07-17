use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use hmac::{Hmac, Mac};
use patchwright_relay::{RelayState, router};
use rusqlite::Connection;
use sha2::Sha256;
use std::{fs, os::unix::fs::PermissionsExt};
use tempfile::TempDir;
use tower::ServiceExt;

fn signature(secret: &[u8], body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).unwrap();
    mac.update(body);
    format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
}

fn request(secret: &[u8], delivery: &str, body: &'static [u8]) -> Request<Body> {
    request_for_event(secret, delivery, "pull_request", body)
}

fn request_for_event(
    secret: &[u8],
    delivery: &str,
    event: &str,
    body: &'static [u8],
) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/webhooks/github")
        .header("x-github-event", event)
        .header("x-github-delivery", delivery)
        .header("x-hub-signature-256", signature(secret, body))
        .body(Body::from(body))
        .unwrap()
}

#[tokio::test]
async fn supported_events_require_and_accept_their_typed_minimum_payloads() {
    let secret = b"test-secret";
    let state = RelayState::new(secret.to_vec());
    let app = router(state.clone());
    let cases: [(&str, &'static [u8]); 8] = [
        (
            "pull_request",
            br#"{"action":"synchronize","pull_request":{"number":42},"repository":{"id":1,"full_name":"octocat/example"}}"#,
        ),
        (
            "issue_comment",
            br#"{"action":"created","issue":{"number":42},"comment":{"id":10},"repository":{"id":1,"full_name":"octocat/example"}}"#,
        ),
        (
            "pull_request_review",
            br#"{"action":"submitted","pull_request":{"number":42},"review":{"id":11},"repository":{"id":1,"full_name":"octocat/example"}}"#,
        ),
        (
            "pull_request_review_comment",
            br#"{"action":"edited","pull_request":{"number":42},"comment":{"id":12},"repository":{"id":1,"full_name":"octocat/example"}}"#,
        ),
        (
            "check_run",
            br#"{"action":"completed","check_run":{"id":13},"repository":{"id":1,"full_name":"octocat/example"}}"#,
        ),
        (
            "check_suite",
            br#"{"action":"rerequested","check_suite":{"id":14},"repository":{"id":1,"full_name":"octocat/example"}}"#,
        ),
        (
            "workflow_run",
            br#"{"action":"in_progress","workflow_run":{"id":15},"repository":{"id":1,"full_name":"octocat/example"}}"#,
        ),
        ("ping", br#"{"hook_id":16,"zen":"Responsive is better."}"#),
    ];

    for (index, (event, body)) in cases.into_iter().enumerate() {
        let response = app
            .clone()
            .oneshot(request_for_event(
                secret,
                &format!("delivery-supported-{index}"),
                event,
                body,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED, "event {event}");
    }
    assert_eq!(state.delivery_count(), cases.len());
}

#[tokio::test]
async fn webhook_rejects_tampering_and_deduplicates_delivery() {
    let secret = b"test-secret";
    let state = RelayState::new(secret.to_vec());
    let app = router(state.clone());
    let body = br#"{"action":"opened","pull_request":{"number":42,"title":"private issue contents"},"repository":{"id":1,"full_name":"octocat/example"}}"#;

    let accepted = app
        .clone()
        .oneshot(request(secret, "delivery-1", body))
        .await
        .unwrap();
    assert_eq!(accepted.status(), StatusCode::ACCEPTED);
    assert_eq!(state.delivery_count(), 1);

    let duplicate = app
        .clone()
        .oneshot(request(secret, "delivery-1", body))
        .await
        .unwrap();
    assert_eq!(duplicate.status(), StatusCode::OK);
    assert_eq!(state.delivery_count(), 1);

    let mut tampered = request(secret, "delivery-2", body);
    *tampered.body_mut() = Body::from(br#"{"action":"closed"}"#.as_slice());
    let rejected = app.oneshot(tampered).await.unwrap();
    assert_eq!(rejected.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(state.delivery_count(), 1);
}

#[tokio::test]
async fn accepted_delivery_remains_a_duplicate_after_reopening_the_database() {
    let secret = b"test-secret";
    let temporary = TempDir::new().unwrap();
    fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let database = temporary.path().join("relay.sqlite");
    let body = br#"{"action":"opened","pull_request":{"number":42,"title":"private issue contents"},"repository":{"id":1,"full_name":"octocat/example"}}"#;

    {
        let state = RelayState::open(secret.to_vec(), &database).unwrap();
        let response = router(state.clone())
            .oneshot(request(secret, "delivery-restart", body))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        assert_eq!(state.delivery_count(), 1);

        let committed: i64 = Connection::open(&database)
            .unwrap()
            .query_row("SELECT COUNT(*) FROM webhook_deliveries", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(committed, 1, "202 is returned only after commit");
    }

    let reopened = RelayState::open(secret.to_vec(), &database).unwrap();
    let duplicate = router(reopened.clone())
        .oneshot(request(secret, "delivery-restart", body))
        .await
        .unwrap();
    assert_eq!(duplicate.status(), StatusCode::OK);
    assert_eq!(reopened.delivery_count(), 1);
    drop(reopened);

    let stored = Connection::open(&database)
        .unwrap()
        .query_row(
            "SELECT event, action, payload FROM webhook_deliveries WHERE delivery_id = ?1",
            ["delivery-restart"],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(stored.0, "pull_request");
    assert_eq!(stored.1, "opened");
    assert_ne!(stored.2, body);
    let sanitized: serde_json::Value = serde_json::from_slice(&stored.2).unwrap();
    assert_eq!(sanitized["event"], "pull_request");
    assert_eq!(sanitized["action"], "opened");
    assert_eq!(sanitized["repositoryId"], 1);
    assert_eq!(sanitized["repositoryFullName"], "octocat/example");
    assert_eq!(sanitized["entityNumber"], 42);
    assert!(
        !String::from_utf8(stored.2)
            .unwrap()
            .contains("private issue contents")
    );
}

#[tokio::test]
async fn unsupported_or_incomplete_events_are_rejected_without_a_durable_write() {
    let secret = b"test-secret";
    let temporary = TempDir::new().unwrap();
    fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let state = RelayState::open(secret.to_vec(), temporary.path().join("relay.sqlite")).unwrap();
    let app = router(state.clone());

    let unsupported_action = br#"{"action":"unknown","pull_request":{"number":42},"repository":{"id":1,"full_name":"octocat/example"}}"#;
    let response = app
        .clone()
        .oneshot(request(secret, "delivery-action", unsupported_action))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let incomplete = br#"{"action":"opened","pull_request":{}}"#;
    let response = app
        .clone()
        .oneshot(request(secret, "delivery-incomplete", incomplete))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let missing_repository = br#"{"action":"opened","pull_request":{"number":42}}"#;
    let response = app
        .clone()
        .oneshot(request(
            secret,
            "delivery-missing-repository",
            missing_repository,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);

    let mut unsupported_event = request(
        secret,
        "delivery-event",
        br#"{"action":"created","repository":{"id":1}}"#,
    );
    unsupported_event
        .headers_mut()
        .insert("x-github-event", "repository".parse().unwrap());
    let response = app.oneshot(unsupported_event).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(state.delivery_count(), 0);
}

#[tokio::test]
async fn plain_issue_comments_do_not_forge_a_pull_request_monitor_identity() {
    let secret = b"test-secret";
    let temporary = TempDir::new().unwrap();
    fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let database = temporary.path().join("relay.sqlite");
    let state = RelayState::open(secret.to_vec(), &database).unwrap();
    let body = br#"{"action":"created","issue":{"number":42},"comment":{"id":10},"repository":{"id":1,"full_name":"octocat/example"}}"#;
    let response = router(state)
        .oneshot(request_for_event(
            secret,
            "plain-issue-comment",
            "issue_comment",
            body,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let payload: Vec<u8> = Connection::open(database)
        .unwrap()
        .query_row(
            "SELECT payload FROM webhook_deliveries WHERE delivery_id = 'plain-issue-comment'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let envelope: serde_json::Value = serde_json::from_slice(&payload).unwrap();
    assert!(envelope["entityNumber"].is_null());
}

#[test]
fn relay_database_is_created_in_an_owner_only_directory_with_owner_only_permissions() {
    let temporary = TempDir::new().unwrap();
    let parent = temporary.path().join("private");
    let database = parent.join("relay.sqlite");

    let state = RelayState::open(b"secret".to_vec(), &database).unwrap();
    drop(state);

    assert_eq!(
        fs::metadata(&parent).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(&database).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[cfg(unix)]
#[test]
fn relay_database_rejects_a_symlink_path() {
    use std::os::unix::fs::symlink;

    let temporary = TempDir::new().unwrap();
    fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let target = temporary.path().join("target.sqlite");
    fs::write(&target, []).unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
    let link = temporary.path().join("relay.sqlite");
    symlink(&target, &link).unwrap();

    let error = RelayState::open(b"secret".to_vec(), &link)
        .err()
        .expect("symlink database must be rejected");
    assert!(error.to_string().contains("regular owner-only file"));
}

#[test]
fn relay_database_rejects_an_existing_group_accessible_parent() {
    let temporary = TempDir::new().unwrap();
    fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o750)).unwrap();

    let error = RelayState::open(b"secret".to_vec(), temporary.path().join("relay.sqlite"))
        .err()
        .expect("group-accessible parent must be rejected");
    assert!(error.to_string().contains("owner-only directory"));
}
