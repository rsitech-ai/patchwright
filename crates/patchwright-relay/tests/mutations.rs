use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::any,
};
use patchwright_core::{GitHubAction, InlineReviewComment, MergeMethod, ReviewEvent};
use patchwright_relay::{GitHubMutationClient, MutationError, MutationResult};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<(Method, String, Value)>>>);

async fn capture_request(
    State(capture): State<Capture>,
    Path(path): Path<String>,
    method: Method,
    headers: HeaderMap,
    body: Option<Json<Value>>,
) -> (StatusCode, Json<Value>) {
    assert_eq!(headers["accept"], "application/vnd.github+json");
    assert_eq!(headers["x-github-api-version"], "2022-11-28");
    assert_eq!(headers["authorization"], "Bearer ghs_fixture");
    let body = body.map_or(Value::Null, |Json(value)| value);
    capture
        .0
        .lock()
        .unwrap()
        .push((method.clone(), format!("/{path}"), body.clone()));
    let response = if method == Method::GET && path.ends_with("/pulls/4") {
        json!({"id":91,"node_id":"PR_node","number":4,"html_url":"https://example.invalid/pull/4","draft":true,"head":{"sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}})
    } else if method == Method::GET && path.ends_with("/git/ref/heads/main") {
        json!({"ref":"refs/heads/main","object":{"sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}})
    } else if path.ends_with("/git/refs") {
        json!({"ref":body["ref"],"object":{"sha":body["sha"]}})
    } else if path == "graphql"
        && body["query"]
            .as_str()
            .is_some_and(|query| query.contains("ReviewThreadIdentity"))
    {
        json!({"data":{"node":{"id":"PRRT_kwDOExample","isResolved":false,"viewerCanResolve":true,"pullRequest":{"number":4,"headRefOid":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","repository":{"nameWithOwner":"octo/fixture"}}}}})
    } else if path == "graphql"
        && body["query"]
            .as_str()
            .is_some_and(|query| query.contains("ResolveReviewThread"))
    {
        json!({"data":{"resolveReviewThread":{"thread":{"id":"PRRT_kwDOExample","isResolved":true}}}})
    } else if path == "graphql" {
        json!({"data":{"markPullRequestReadyForReview":{"pullRequest":{"databaseId":91,"number":4,"url":"https://example.invalid/pull/4","isDraft":false}}}})
    } else if path.ends_with("/merge") {
        json!({"sha":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","merged":true,"message":"merged"})
    } else if path.ends_with("/update-branch") {
        json!({"message":"Updating pull request branch.","url":"https://example.invalid/update/4"})
    } else if method == Method::PATCH && path.ends_with("/pulls/4") {
        json!({"id":91,"number":4,"html_url":"https://example.invalid/pull/4","state":"closed"})
    } else if method == Method::PATCH && path.ends_with("/issues/5") {
        json!({"id":92,"number":5,"html_url":"https://example.invalid/issues/5","state":"closed","state_reason":"completed"})
    } else if path.ends_with("/pulls") {
        json!({"number":17,"html_url":"https://example.invalid/pull/17","base":{"sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}})
    } else {
        json!({"id":91,"html_url":"https://example.invalid/result/91"})
    };
    (StatusCode::CREATED, Json(response))
}

#[tokio::test]
async fn resolves_only_the_exact_owned_review_thread_identity() {
    let (client, capture) = client().await;
    let result = client
        .execute(
            "octo",
            "fixture",
            &GitHubAction::resolve_review_thread(4, "PRRT_kwDOExample").unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(result.resolved, Some(true));
    let requests = capture.0.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].1, "/graphql");
    assert_eq!(requests[0].2["variables"]["threadId"], "PRRT_kwDOExample");
    assert_eq!(requests[1].1, "/graphql");
    assert_eq!(requests[1].2["variables"]["threadId"], "PRRT_kwDOExample");
}

async fn client() -> (GitHubMutationClient, Capture) {
    let capture = Capture::default();
    let app = Router::new()
        .route("/{*path}", any(capture_request))
        .with_state(capture.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (
        GitHubMutationClient::new_for_test(format!("http://{address}"), "ghs_fixture").unwrap(),
        capture,
    )
}

#[tokio::test]
async fn emits_documented_identity_and_sha_bound_requests() {
    let (client, capture) = client().await;
    let sha = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let actions = vec![
        GitHubAction::create_branch("feat/test", sha).unwrap(),
        GitHubAction::comment(4, "bounded comment").unwrap(),
        GitHubAction::review(
            4,
            sha,
            ReviewEvent::Approve,
            "review body",
            vec![InlineReviewComment::new("src/lib.rs", 9, "inline body").unwrap()],
        )
        .unwrap(),
        GitHubAction::check_run("Patchwright", sha, "completed", Some("success")).unwrap(),
        GitHubAction::draft_pull_request("title", "feat/test", "main", "body").unwrap(),
        GitHubAction::update_pull_request_branch(4, sha).unwrap(),
        GitHubAction::ready_pull_request(4).unwrap(),
        GitHubAction::close_pull_request(4).unwrap(),
        GitHubAction::close_issue(5).unwrap(),
        GitHubAction::merge_pull_request(4, sha, MergeMethod::Squash).unwrap(),
    ];
    for action in actions {
        client.execute("octo", "fixture", &action).await.unwrap();
    }

    let requests = capture.0.lock().unwrap();
    assert_eq!(
        requests[0],
        (
            Method::POST,
            "/repos/octo/fixture/git/refs".into(),
            json!({"ref":"refs/heads/feat/test","sha":sha})
        )
    );
    assert_eq!(
        requests[1],
        (
            Method::POST,
            "/repos/octo/fixture/issues/4/comments".into(),
            json!({"body":"bounded comment"})
        )
    );
    assert_eq!(requests[2].1, "/repos/octo/fixture/pulls/4/reviews");
    assert_eq!(requests[2].2["commit_id"], sha);
    assert_eq!(requests[2].2["event"], "APPROVE");
    assert_eq!(
        requests[2].2["comments"][0],
        json!({"path":"src/lib.rs","line":9,"side":"RIGHT","body":"inline body"})
    );
    assert_eq!(requests[3].2["conclusion"], "success");
    assert_eq!(requests[4].2["draft"], true);
    assert_eq!(requests[5].2, json!({"expected_head_sha":sha}));
    assert_eq!(
        requests[6],
        (
            Method::GET,
            "/repos/octo/fixture/pulls/4".into(),
            Value::Null
        )
    );
    assert_eq!(requests[7].0, Method::POST);
    assert_eq!(requests[7].1, "/graphql");
    assert_eq!(requests[7].2["variables"]["pullRequestId"], "PR_node");
    assert_eq!(requests[8].2, json!({"state":"closed"}));
    assert_eq!(
        requests[9],
        (
            Method::PATCH,
            "/repos/octo/fixture/issues/5".into(),
            json!({"state":"closed","state_reason":"completed"})
        )
    );
    assert_eq!(
        requests[10],
        (
            Method::PUT,
            "/repos/octo/fixture/pulls/4/merge".into(),
            json!({"sha":sha,"merge_method":"squash"})
        )
    );
}

#[tokio::test]
async fn transport_only_actions_and_ambiguous_failures_are_explicit_and_redacted() {
    let (client, _) = client().await;
    let sha = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let error = client
        .execute(
            "octo",
            "fixture",
            &GitHubAction::push_intent("feat/test", sha).unwrap(),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, MutationError::GitTransportRequired));
    assert!(!format!("{client:?}{error:?}").contains("ghs_fixture"));

    let result = MutationResult::default();
    assert_eq!(result.merged, None);
}

async fn invalid_json() -> (StatusCode, &'static str) {
    (StatusCode::CREATED, "{not-json")
}

async fn ready_response(Path(path): Path<String>, method: Method) -> Response {
    if method == Method::GET && path.ends_with("/pulls/4") {
        return Json(json!({
            "id":91,
            "node_id":"PR_node",
            "number":4,
            "html_url":"https://example.invalid/pull/4",
            "draft":true,
            "head":{"sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}
        }))
        .into_response();
    }
    Json(json!({
        "data":{
            "markPullRequestReadyForReview":{
                "pullRequest":{
                    "databaseId":91,
                    "number":4,
                    "url":"https://example.invalid/pull/4",
                    "isDraft":true
                }
            }
        }
    }))
    .into_response()
}

async fn test_client(app: Router) -> GitHubMutationClient {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    GitHubMutationClient::new_for_test(format!("http://{address}"), "ghs_fixture").unwrap()
}

#[tokio::test]
async fn successful_non_idempotent_mutation_with_unparseable_body_is_ambiguous() {
    let client = test_client(Router::new().route("/{*path}", any(invalid_json))).await;
    let error = client
        .execute(
            "octo",
            "fixture",
            &GitHubAction::comment(4, "bounded comment").unwrap(),
        )
        .await
        .unwrap_err();

    assert!(matches!(error, MutationError::AmbiguousTransport));
}

#[tokio::test]
async fn preflight_read_with_unparseable_body_is_a_definite_invalid_response() {
    let client = test_client(Router::new().route("/{*path}", any(invalid_json))).await;
    let error = client
        .execute(
            "octo",
            "fixture",
            &GitHubAction::ready_pull_request(4).unwrap(),
        )
        .await
        .unwrap_err();

    assert!(matches!(error, MutationError::InvalidResponse));
}

#[tokio::test]
async fn successful_mutation_with_untrusted_semantics_is_ambiguous() {
    let client = test_client(Router::new().route("/{*path}", any(ready_response))).await;
    let error = client
        .execute(
            "octo",
            "fixture",
            &GitHubAction::ready_pull_request(4).unwrap(),
        )
        .await
        .unwrap_err();

    assert!(matches!(error, MutationError::AmbiguousTransport));
}
