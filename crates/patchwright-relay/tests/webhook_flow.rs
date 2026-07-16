use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use hmac::{Hmac, Mac};
use patchwright_relay::{RelayState, router};
use sha2::Sha256;
use tower::ServiceExt;

fn signature(secret: &[u8], body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).unwrap();
    mac.update(body);
    format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
}

fn request(secret: &[u8], delivery: &str, body: &'static [u8]) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/webhooks/github")
        .header("x-github-event", "pull_request")
        .header("x-github-delivery", delivery)
        .header("x-hub-signature-256", signature(secret, body))
        .body(Body::from(body))
        .unwrap()
}

#[tokio::test]
async fn webhook_rejects_tampering_and_deduplicates_delivery() {
    let secret = b"test-secret";
    let state = RelayState::new(secret.to_vec());
    let app = router(state.clone());
    let body = br#"{"action":"opened","pull_request":{"number":42}}"#;

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
