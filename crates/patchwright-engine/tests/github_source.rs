use axum::{
    Json, Router,
    extract::{Path, Query},
    http::{HeaderMap, HeaderValue},
    response::IntoResponse,
    routing::get,
};
use patchwright_engine::{GitHubSource, GitHubToken, WorkItemKind};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

#[tokio::test]
async fn pagination_never_forwards_a_token_to_a_cross_origin_link() {
    let token_reached_other_origin = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&token_reached_other_origin);
    let other_app = Router::new().route(
        "/steal",
        get(move |headers: HeaderMap| {
            let observed = Arc::clone(&observed);
            async move {
                observed.store(headers.contains_key("authorization"), Ordering::SeqCst);
                Json(json!([]))
            }
        }),
    );
    let other_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let other_address = other_listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(other_listener, other_app).await.unwrap() });

    let app = Router::new().route(
        "/user/repos",
        get(move || async move {
            let mut headers = HeaderMap::new();
            headers.insert(
                "link",
                HeaderValue::from_str(&format!("<http://{other_address}/steal>; rel=\"next\""))
                    .unwrap(),
            );
            (headers, Json(json!([])))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let source = GitHubSource::new(
        format!("http://{address}"),
        GitHubToken::new("secret-token"),
    )
    .unwrap();
    assert!(source.repositories(10).await.unwrap().is_empty());
    assert!(!token_reached_other_origin.load(Ordering::SeqCst));
}

#[tokio::test]
async fn authenticated_source_paginates_and_separates_issues_from_pull_requests() {
    let observed_authorization = Arc::new(Mutex::new(Vec::new()));
    let auth = Arc::clone(&observed_authorization);
    let app = Router::new()
        .route("/user", get(move |headers: HeaderMap| {
            let auth = Arc::clone(&auth);
            async move {
                auth.lock().unwrap().push(headers.get("authorization").unwrap().to_str().unwrap().to_owned());
                Json(json!({"login":"octocat","avatar_url":"https://example/avatar","html_url":"https://github.com/octocat"}))
            }
        }))
        .route("/user/repos", get(repositories))
        .route("/repos/{owner}/{repo}/issues", get(issues))
        .route("/repos/{owner}/{repo}/pulls", get(pulls))
        .route("/repos/{owner}/{repo}/pulls/{number}/reviews", get(empty_array))
        .route("/repos/{owner}/{repo}/issues/comments", get(empty_array))
        .route("/repos/{owner}/{repo}/pulls/comments", get(empty_array))
        .route("/repos/{owner}/{repo}/actions/runs", get(workflow_runs))
        .route("/repos/{owner}/{repo}/commits/{sha}/check-runs", get(check_runs));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let source = GitHubSource::new(
        format!("http://{address}"),
        GitHubToken::new("secret-token"),
    )
    .unwrap();
    let account = source.account().await.unwrap();
    let repositories = source.repositories(10).await.unwrap();
    let snapshot = source
        .repository_snapshot(&repositories[0], 10)
        .await
        .unwrap();

    assert_eq!(account.login, "octocat");
    assert_eq!(repositories.len(), 2);
    assert_eq!(repositories[1].full_name, "octocat/second");
    assert_eq!(snapshot.work_items.len(), 2);
    assert_eq!(snapshot.checks.len(), 2);
    assert_eq!(snapshot.workflow_runs.len(), 2);
    let issue = snapshot
        .work_items
        .iter()
        .find(|item| item.kind == WorkItemKind::Issue)
        .unwrap();
    assert_eq!(issue.labels, ["bug"]);
    assert_eq!(issue.assignees, ["hubot"]);
    assert_eq!(issue.milestone.as_deref(), Some("v1"));
    assert_eq!(
        snapshot
            .work_items
            .iter()
            .filter(|item| item.kind == WorkItemKind::Issue)
            .count(),
        1
    );
    assert_eq!(
        snapshot
            .work_items
            .iter()
            .filter(|item| item.kind == WorkItemKind::PullRequest)
            .count(),
        1
    );
    assert_eq!(
        observed_authorization.lock().unwrap().as_slice(),
        ["Bearer secret-token"]
    );
    assert_eq!(
        format!("{:?}", GitHubToken::new("secret-token")),
        "GitHubToken([REDACTED])"
    );
}

async fn repositories(Query(query): Query<HashMap<String, String>>) -> impl IntoResponse {
    let page = query.get("page").map_or("1", String::as_str);
    let repository = |id, name| {
        json!({
            "id":id,"full_name":format!("octocat/{name}"),"description":null,"private":false,"archived":false,
            "default_branch":"main","html_url":format!("https://github.com/octocat/{name}"),"updated_at":"2026-07-13T10:00:00Z","open_issues_count":2
        })
    };
    if page == "1" {
        let mut headers = HeaderMap::new();
        headers.insert(
            "link",
            HeaderValue::from_static("</user/repos?per_page=100&page=2>; rel=\"next\""),
        );
        (headers, Json(json!([repository(1, "first")]))).into_response()
    } else {
        Json(json!([repository(2, "second")])).into_response()
    }
}

async fn issues(Path((_owner, _repo)): Path<(String, String)>) -> Json<Value> {
    Json(json!([
        {"id":10,"number":1,"title":"Real issue","state":"open","body":"Issue body","user":{"login":"octocat"},"html_url":"https://github.com/octocat/first/issues/1","draft":false,"comments":0,"updated_at":"2026-07-13T10:00:00Z","labels":[{"name":"bug"}],"assignees":[{"login":"hubot"}],"milestone":{"title":"v1"}},
        {"id":11,"number":2,"title":"Also a pull","state":"open","body":null,"user":{"login":"octocat"},"html_url":"https://github.com/octocat/first/pull/2","pull_request":{},"comments":0,"updated_at":"2026-07-13T10:00:00Z"}
    ]))
}

async fn pulls() -> Json<Value> {
    Json(
        json!([{"id":20,"number":2,"title":"Pull","state":"open","body":"PR body","user":{"login":"octocat"},"html_url":"https://github.com/octocat/first/pull/2","draft":true,"head":{"sha":"abc123"},"updated_at":"2026-07-13T10:00:00Z"}]),
    )
}

async fn empty_array() -> Json<Value> {
    Json(json!([]))
}

async fn check_runs(Query(query): Query<HashMap<String, String>>) -> impl IntoResponse {
    let page = query.get("page").map_or("1", String::as_str);
    let run = |id, name| json!({"id":id,"name":name,"status":"completed","conclusion":"success","html_url":"https://example/check"});
    if page == "1" {
        let mut headers = HeaderMap::new();
        headers.insert("link", HeaderValue::from_static("</repos/octocat/first/commits/abc123/check-runs?per_page=100&page=2>; rel=\"next\""));
        (headers, Json(json!({"check_runs":[run(30, "build")]}))).into_response()
    } else {
        Json(json!({"check_runs":[run(31, "test")]})).into_response()
    }
}

async fn workflow_runs(Query(query): Query<HashMap<String, String>>) -> impl IntoResponse {
    let page = query.get("page").map_or("1", String::as_str);
    let run = |id, name| json!({"id":id,"name":name,"status":"completed","conclusion":"success","event":"pull_request","head_sha":"abc123","html_url":"https://example/run","updated_at":"2026-07-13T10:00:00Z"});
    if page == "1" {
        let mut headers = HeaderMap::new();
        headers.insert(
            "link",
            HeaderValue::from_static(
                "</repos/octocat/first/actions/runs?per_page=100&page=2>; rel=\"next\"",
            ),
        );
        (headers, Json(json!({"workflow_runs":[run(40, "CI")]}))).into_response()
    } else {
        Json(json!({"workflow_runs":[run(41, "Audit")]})).into_response()
    }
}
