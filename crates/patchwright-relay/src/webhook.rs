use axum::{
    Router,
    body::Bytes,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

#[derive(Clone)]
pub struct RelayState {
    secret: Arc<Vec<u8>>,
    deliveries: Arc<Mutex<HashSet<String>>>,
}

impl RelayState {
    #[must_use]
    pub fn new(secret: Vec<u8>) -> Self {
        Self {
            secret: Arc::new(secret),
            deliveries: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    #[must_use]
    pub fn delivery_count(&self) -> usize {
        self.deliveries
            .lock()
            .expect("delivery lock poisoned")
            .len()
    }
}

pub fn router(state: RelayState) -> Router {
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/webhooks/github", post(github_webhook))
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .with_state(state)
}

async fn github_webhook(
    State(state): State<RelayState>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let signature = headers
        .get("x-hub-signature-256")
        .and_then(|value| value.to_str().ok());
    if !signature.is_some_and(|value| verify_signature(&state.secret, &body, value)) {
        return (StatusCode::UNAUTHORIZED, "invalid signature");
    }
    let delivery = match headers
        .get("x-github-delivery")
        .and_then(|value| value.to_str().ok())
    {
        Some(value) if !value.is_empty() && value.len() <= 128 => value.to_owned(),
        _ => return (StatusCode::BAD_REQUEST, "missing delivery id"),
    };
    if serde_json::from_slice::<serde_json::Value>(&body).is_err() {
        return (StatusCode::BAD_REQUEST, "invalid json");
    }
    let mut deliveries = state.deliveries.lock().expect("delivery lock poisoned");
    if !deliveries.insert(delivery) {
        return (StatusCode::OK, "duplicate");
    }
    (StatusCode::ACCEPTED, "accepted")
}

#[must_use]
pub fn verify_signature(secret: &[u8], body: &[u8], signature: &str) -> bool {
    let Some(hex_signature) = signature.strip_prefix("sha256=") else {
        return false;
    };
    let Ok(expected) = hex::decode(hex_signature) else {
        return false;
    };
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret) else {
        return false;
    };
    mac.update(body);
    mac.verify_slice(&expected).is_ok()
}
