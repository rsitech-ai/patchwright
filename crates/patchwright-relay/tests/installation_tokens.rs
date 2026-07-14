use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use patchwright_relay::{
    AppAuthenticator, GitHubAppConfiguration, InstallationBroker, InstallationPermissions,
    KeyReference, ProtectedFileKeyProvider,
};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tempfile::tempdir;

#[derive(Clone)]
struct MockState {
    token_requests: Arc<AtomicUsize>,
}

async fn installation(
    Path((owner, repository)): Path<(String, String)>,
    headers: HeaderMap,
) -> (StatusCode, Json<Value>) {
    assert_eq!((owner.as_str(), repository.as_str()), ("octo", "fixture"));
    assert!(
        headers
            .get("authorization")
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("Bearer eyJ")
    );
    (
        StatusCode::OK,
        Json(
            json!({"id":7,"account":{"login":"octo"},"permissions":{"metadata":"read","contents":"write","pull_requests":"write","checks":"write","issues":"write"}}),
        ),
    )
}

async fn token(
    State(state): State<MockState>,
    Path(installation_id): Path<u64>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    assert_eq!(installation_id, 7);
    assert!(headers.get("authorization").is_some());
    assert_eq!(body["repository_ids"], json!([123]));
    assert_eq!(body["permissions"]["metadata"], "read");
    assert_eq!(body["permissions"]["contents"], "write");
    let request = state.token_requests.fetch_add(1, Ordering::SeqCst) + 1;
    (
        StatusCode::CREATED,
        Json(
            json!({"token":format!("ghs_synthetic_{request}"),"expires_at":"2027-01-15T08:10:00Z","permissions":body["permissions"],"repository_selection":"selected"}),
        ),
    )
}

fn authenticator(root: &std::path::Path) -> AppAuthenticator<ProtectedFileKeyProvider> {
    let private = root.join("private.pem");
    assert!(
        Command::new("openssl")
            .args(["genrsa", "-out", private.to_str().unwrap(), "2048"])
            .status()
            .unwrap()
            .success()
    );
    std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o600)).unwrap();
    AppAuthenticator::new(
        GitHubAppConfiguration::new(
            42,
            "Iv1.patchwright",
            KeyReference::protected_file(private),
            "https://api.github.com",
        )
        .unwrap(),
        ProtectedFileKeyProvider,
    )
    .unwrap()
}

#[tokio::test]
async fn discovers_installation_mints_scoped_token_and_collapses_cache() {
    let root = tempdir().unwrap();
    let state = MockState {
        token_requests: Arc::new(AtomicUsize::new(0)),
    };
    let app = Router::new()
        .route(
            "/repos/{owner}/{repository}/installation",
            get(installation),
        )
        .route(
            "/app/installations/{installation_id}/access_tokens",
            post(token),
        )
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let broker = Arc::new(
        InstallationBroker::new(authenticator(root.path()), format!("http://{address}")).unwrap(),
    );
    let permissions = InstallationPermissions::delivery();
    let now = 1_800_000_000;
    let (first, second) = tokio::join!(
        broker.token_for_repository("octo", "fixture", 123, permissions.clone(), now),
        broker.token_for_repository("octo", "fixture", 123, permissions.clone(), now)
    );
    let first = first.unwrap();
    let second = second.unwrap();
    assert_eq!(
        first.expose_for_authorization_header(),
        second.expose_for_authorization_header()
    );
    assert_eq!(state.token_requests.load(Ordering::SeqCst), 1);
    assert!(!format!("{first:?}{second:?}{broker:?}").contains("ghs_synthetic"));
    assert_eq!(first.installation_id(), 7);
    assert_eq!(first.repository_ids(), &[123]);

    broker.revoke_cached_tokens().await;
    let refreshed = broker
        .token_for_repository("octo", "fixture", 123, permissions, now)
        .await
        .unwrap();
    assert_ne!(
        first.expose_for_authorization_header(),
        refreshed.expose_for_authorization_header()
    );
    assert_eq!(state.token_requests.load(Ordering::SeqCst), 2);
}
