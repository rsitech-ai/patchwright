use crate::SecretString;
use patchwright_core::{GitHubAction, MergeMethod, ReviewEvent};
use reqwest::{Client, Method, StatusCode, Url};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fmt;
use thiserror::Error;

const API_VERSION: &str = "2022-11-28";

pub struct GitHubMutationClient {
    client: Client,
    base_url: Url,
    token: SecretString,
}

impl GitHubMutationClient {
    pub fn new(base_url: impl AsRef<str>, token: impl AsRef<str>) -> Result<Self, MutationError> {
        Self::with_transport_policy(base_url, token, false)
    }

    #[doc(hidden)]
    pub fn new_for_test(
        base_url: impl AsRef<str>,
        token: impl AsRef<str>,
    ) -> Result<Self, MutationError> {
        Self::with_transport_policy(base_url, token, true)
    }

    fn with_transport_policy(
        base_url: impl AsRef<str>,
        token: impl AsRef<str>,
        allow_loopback_http: bool,
    ) -> Result<Self, MutationError> {
        let base_url = Url::parse(base_url.as_ref()).map_err(|_| MutationError::InvalidTarget)?;
        let secure = base_url.scheme() == "https";
        let loopback = base_url.scheme() == "http"
            && matches!(base_url.host_str(), Some("localhost" | "127.0.0.1"));
        if !(secure || allow_loopback_http && loopback) {
            return Err(MutationError::InvalidTarget);
        }
        if token.as_ref().trim().is_empty() {
            return Err(MutationError::MissingToken);
        }
        Ok(Self {
            client: Client::builder()
                .user_agent("Patchwright/0.1")
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            base_url,
            token: SecretString::new(token),
        })
    }

    #[allow(clippy::too_many_lines)]
    pub async fn execute(
        &self,
        owner: &str,
        repository: &str,
        action: &GitHubAction,
    ) -> Result<MutationResult, MutationError> {
        validate_component(owner)?;
        validate_component(repository)?;
        let prefix = format!("repos/{owner}/{repository}");
        match action {
            GitHubAction::CreateBranch { branch, from_sha } => {
                let reference = format!("refs/heads/{branch}");
                self.request(
                    Method::POST,
                    &format!("{prefix}/git/refs"),
                    json!({"ref":reference,"sha":from_sha}),
                    MutationExpectation::CreatedRef {
                        reference: &reference,
                        sha: from_sha,
                    },
                )
                .await
            }
            GitHubAction::PushIntent { .. } => Err(MutationError::GitTransportRequired),
            GitHubAction::Comment { issue_number, body } => {
                self.request(
                    Method::POST,
                    &format!("{prefix}/issues/{issue_number}/comments"),
                    json!({"body":body}),
                    MutationExpectation::Resource,
                )
                .await
            }
            GitHubAction::Review {
                pull_request_number,
                expected_head_sha,
                event,
                body,
                inline_comments,
            } => {
                let comments: Vec<Value> = inline_comments
                    .iter()
                    .map(|comment| {
                        json!({"path":comment.path(),"line":comment.line(),"side":"RIGHT","body":comment.body()})
                    })
                    .collect();
                let mut payload = json!({"body":body,"comments":comments,"commit_id":expected_head_sha});
                if *event != ReviewEvent::Pending {
                    payload["event"] = Value::String(review_event(*event).to_owned());
                }
                self.request(
                    Method::POST,
                    &format!("{prefix}/pulls/{pull_request_number}/reviews"),
                    payload,
                    MutationExpectation::Resource,
                )
                .await
            }
            GitHubAction::ResolveReviewThread {
                pull_request_number,
                thread_id,
                expected_head_sha,
            } => {
                let pull = self
                    .request_value(
                        Method::GET,
                        &format!("{prefix}/pulls/{pull_request_number}"),
                        None,
                        RequestEffect::Read,
                    )
                    .await?;
                if pull.pointer("/head/sha").and_then(Value::as_str)
                    != Some(expected_head_sha.as_str())
                {
                    return Err(MutationError::StaleRemoteHead);
                }
                let identity = self
                    .request_value(
                        Method::POST,
                        "graphql",
                        Some(&json!({
                            "query":"query ReviewThreadIdentity($threadId: ID!) { node(id: $threadId) { ... on PullRequestReviewThread { id isResolved viewerCanResolve pullRequest { number headRefOid repository { nameWithOwner } } } } }",
                            "variables":{"threadId":thread_id}
                        })),
                        RequestEffect::Read,
                    )
                    .await?;
                if identity.get("errors").is_some() {
                    return Err(MutationError::GraphQlRejected);
                }
                let thread = identity
                    .pointer("/data/node")
                    .ok_or(MutationError::InvalidResponse)?;
                let repository_full_name = format!("{owner}/{repository}");
                if thread.get("id").and_then(Value::as_str) != Some(thread_id)
                    || thread.pointer("/pullRequest/number").and_then(Value::as_u64)
                        != Some(*pull_request_number)
                    || thread
                        .pointer("/pullRequest/headRefOid")
                        .and_then(Value::as_str)
                        != Some(expected_head_sha)
                    || thread
                        .pointer("/pullRequest/repository/nameWithOwner")
                        .and_then(Value::as_str)
                        != Some(repository_full_name.as_str())
                {
                    return Err(MutationError::ReviewThreadMismatch);
                }
                if thread.get("isResolved").and_then(Value::as_bool) == Some(true) {
                    return Ok(MutationResult {
                        node_id: Some(thread_id.clone()),
                        resolved: Some(true),
                        ..MutationResult::default()
                    });
                }
                if thread.get("viewerCanResolve").and_then(Value::as_bool) != Some(true) {
                    return Err(MutationError::ReviewThreadNotResolvable);
                }
                let response = self
                    .request_value(
                        Method::POST,
                        "graphql",
                        Some(&json!({
                            "query":"mutation ResolveReviewThread($threadId: ID!) { resolveReviewThread(input: {threadId: $threadId}) { thread { id isResolved } } }",
                            "variables":{"threadId":thread_id}
                        })),
                        RequestEffect::Mutation,
                    )
                    .await?;
                if response.get("errors").is_some() {
                    return Err(MutationError::GraphQlRejected);
                }
                let resolved = response
                    .pointer("/data/resolveReviewThread/thread")
                    .ok_or(MutationError::AmbiguousTransport)?;
                if resolved.get("id").and_then(Value::as_str) != Some(thread_id)
                    || resolved.get("isResolved").and_then(Value::as_bool) != Some(true)
                {
                    return Err(MutationError::AmbiguousTransport);
                }
                Ok(MutationResult {
                    node_id: Some(thread_id.clone()),
                    resolved: Some(true),
                    ..MutationResult::default()
                })
            }
            GitHubAction::CheckRun {
                name,
                head_sha,
                status,
                conclusion,
            } => {
                self.request(
                    Method::POST,
                    &format!("{prefix}/check-runs"),
                    json!({"name":name,"head_sha":head_sha,"status":status,"conclusion":conclusion}),
                    MutationExpectation::Resource,
                )
                .await
            }
            GitHubAction::DraftPullRequest {
                title,
                head,
                base,
                body,
            } => {
                self.request(
                    Method::POST,
                    &format!("{prefix}/pulls"),
                    json!({"title":title,"head":head,"base":base,"body":body,"draft":true}),
                    MutationExpectation::PullRequest,
                )
                .await
            }
            GitHubAction::UpdatePullRequestBranch {
                pull_request_number,
                expected_head_sha,
            } => {
                self.request(
                    Method::PUT,
                    &format!("{prefix}/pulls/{pull_request_number}/update-branch"),
                    json!({"expected_head_sha":expected_head_sha}),
                    MutationExpectation::UpdateBranch,
                )
                .await
            }
            GitHubAction::ReadyPullRequest {
                pull_request_number,
                expected_head_sha,
            } => {
                let pull = self
                    .request_value(
                        Method::GET,
                        &format!("{prefix}/pulls/{pull_request_number}"),
                        None,
                        RequestEffect::Read,
                    )
                    .await?;
                if pull.pointer("/head/sha").and_then(Value::as_str)
                    != Some(expected_head_sha.as_str())
                {
                    return Err(MutationError::StaleRemoteHead);
                }
                if pull.get("draft").and_then(Value::as_bool) == Some(false) {
                    return Ok(decode_value(&pull));
                }
                let pull_request_id = pull
                    .get("node_id")
                    .and_then(Value::as_str)
                    .ok_or(MutationError::InvalidResponse)?;
                let response = self
                    .request_value(
                        Method::POST,
                        "graphql",
                        Some(&json!({
                            "query":"mutation MarkReady($pullRequestId: ID!) { markPullRequestReadyForReview(input: {pullRequestId: $pullRequestId}) { pullRequest { databaseId number url isDraft } } }",
                            "variables":{"pullRequestId":pull_request_id}
                        })),
                        RequestEffect::Mutation,
                    )
                    .await?;
                if response.get("errors").is_some() {
                    return Err(MutationError::GraphQlRejected);
                }
                let ready = response
                    .pointer("/data/markPullRequestReadyForReview/pullRequest")
                    .ok_or(MutationError::AmbiguousTransport)?;
                if ready.get("databaseId").and_then(Value::as_u64) == Some(0)
                    || ready.get("databaseId").and_then(Value::as_u64).is_none()
                    || ready.get("number").and_then(Value::as_u64)
                        != Some(*pull_request_number)
                    || ready.get("url").and_then(nonempty_string).is_none()
                    || ready.get("isDraft").and_then(Value::as_bool) != Some(false)
                {
                    return Err(MutationError::AmbiguousTransport);
                }
                Ok(MutationResult {
                    id: ready.get("databaseId").and_then(Value::as_u64),
                    number: ready.get("number").and_then(Value::as_u64),
                    html_url: ready
                        .get("url")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                    sha: None,
                    merged: None,
                    node_id: None,
                    resolved: None,
                })
            }
            GitHubAction::ClosePullRequest {
                pull_request_number,
                expected_head_sha,
            } => {
                let pull = self
                    .request_value(
                        Method::GET,
                        &format!("{prefix}/pulls/{pull_request_number}"),
                        None,
                        RequestEffect::Read,
                    )
                    .await?;
                if pull.pointer("/head/sha").and_then(Value::as_str)
                    != Some(expected_head_sha.as_str())
                {
                    return Err(MutationError::StaleRemoteHead);
                }
                self.request(
                    Method::PATCH,
                    &format!("{prefix}/pulls/{pull_request_number}"),
                    json!({"state":"closed"}),
                    MutationExpectation::Closed {
                        number: *pull_request_number,
                    },
                )
                .await
            }
            GitHubAction::CloseIssue { issue_number } => {
                self.request(
                    Method::PATCH,
                    &format!("{prefix}/issues/{issue_number}"),
                    json!({"state":"closed","state_reason":"completed"}),
                    MutationExpectation::Closed {
                        number: *issue_number,
                    },
                )
                .await
            }
            GitHubAction::EnqueuePullRequest { .. } => Err(MutationError::MergeQueueRequired),
            GitHubAction::MergePullRequest {
                pull_request_number,
                expected_head_sha,
                method,
            } => {
                self.request(
                    Method::PUT,
                    &format!("{prefix}/pulls/{pull_request_number}/merge"),
                    json!({"sha":expected_head_sha,"merge_method":merge_method(*method)}),
                    MutationExpectation::Merged,
                )
                .await
            }
        }
    }

    async fn request(
        &self,
        method: Method,
        path: &str,
        body: Value,
        expectation: MutationExpectation<'_>,
    ) -> Result<MutationResult, MutationError> {
        let value = self
            .request_value(method, path, Some(&body), RequestEffect::Mutation)
            .await?;
        expectation
            .decode(&value)
            .ok_or(MutationError::AmbiguousTransport)
    }

    async fn request_value(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
        effect: RequestEffect,
    ) -> Result<Value, MutationError> {
        let url = self
            .base_url
            .join(path)
            .map_err(|_| MutationError::InvalidTarget)?;
        let mut request = self
            .client
            .request(method, url)
            .bearer_auth(self.token.expose_for_authorization_header())
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", API_VERSION);
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request.send().await.map_err(|error| match effect {
            RequestEffect::Read => MutationError::Client(error),
            RequestEffect::Mutation => MutationError::AmbiguousTransport,
        })?;
        let status = response.status();
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse().ok());
        let request_id = response
            .headers()
            .get("x-github-request-id")
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned);
        if !status.is_success() {
            return Err(MutationError::Rejected {
                status,
                retry_after,
                request_id,
            });
        }
        response.json().await.map_err(|_| match effect {
            RequestEffect::Read => MutationError::InvalidResponse,
            RequestEffect::Mutation => MutationError::AmbiguousTransport,
        })
    }
}

impl fmt::Debug for GitHubMutationClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GitHubMutationClient")
            .field("client", &"reqwest::Client")
            .field("base_url", &self.base_url)
            .field("token", &"[REDACTED]")
            .finish()
    }
}

fn decode_value(value: &Value) -> MutationResult {
    MutationResult {
        id: value.get("id").and_then(Value::as_u64),
        number: value.get("number").and_then(Value::as_u64),
        html_url: value
            .get("html_url")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        sha: value
            .get("sha")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        merged: value.get("merged").and_then(Value::as_bool),
        node_id: value
            .get("node_id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        resolved: value.get("isResolved").and_then(Value::as_bool),
    }
}

#[derive(Clone, Copy)]
enum RequestEffect {
    Read,
    Mutation,
}

enum MutationExpectation<'a> {
    CreatedRef { reference: &'a str, sha: &'a str },
    Resource,
    PullRequest,
    UpdateBranch,
    Closed { number: u64 },
    Merged,
}

impl MutationExpectation<'_> {
    fn decode(&self, value: &Value) -> Option<MutationResult> {
        let decoded = decode_value(value);
        match self {
            Self::CreatedRef { reference, sha } => {
                if value.get("ref").and_then(Value::as_str) != Some(*reference)
                    || value.pointer("/object/sha").and_then(Value::as_str) != Some(*sha)
                {
                    return None;
                }
                Some(MutationResult {
                    sha: Some((*sha).to_owned()),
                    ..decoded
                })
            }
            Self::Resource => (decoded.id.is_some_and(|id| id > 0)
                && decoded
                    .html_url
                    .as_deref()
                    .is_some_and(|url| !url.is_empty()))
            .then_some(decoded),
            Self::PullRequest => (decoded.number.is_some_and(|number| number > 0)
                && decoded
                    .html_url
                    .as_deref()
                    .is_some_and(|url| !url.is_empty()))
            .then_some(decoded),
            Self::UpdateBranch => (value.get("message").and_then(nonempty_string).is_some()
                && value.get("url").and_then(nonempty_string).is_some())
            .then_some(decoded),
            Self::Closed { number } => (decoded.number == Some(*number)
                && value.get("state").and_then(Value::as_str) == Some("closed")
                && decoded
                    .html_url
                    .as_deref()
                    .is_some_and(|url| !url.is_empty()))
            .then_some(decoded),
            Self::Merged => (decoded.merged == Some(true)
                && decoded.sha.as_deref().is_some_and(valid_sha))
            .then_some(decoded),
        }
    }
}

fn nonempty_string(value: &Value) -> Option<&str> {
    value.as_str().filter(|value| !value.trim().is_empty())
}

fn valid_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

const fn review_event(event: ReviewEvent) -> &'static str {
    match event {
        ReviewEvent::Approve => "APPROVE",
        ReviewEvent::RequestChanges => "REQUEST_CHANGES",
        ReviewEvent::Comment => "COMMENT",
        ReviewEvent::Pending => "",
    }
}

const fn merge_method(method: MergeMethod) -> &'static str {
    match method {
        MergeMethod::Merge => "merge",
        MergeMethod::Squash => "squash",
        MergeMethod::Rebase => "rebase",
    }
}

fn validate_component(value: &str) -> Result<(), MutationError> {
    if value.is_empty()
        || value.len() > 100
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(MutationError::InvalidTarget);
    }
    Ok(())
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationResult {
    pub id: Option<u64>,
    pub number: Option<u64>,
    pub html_url: Option<String>,
    pub sha: Option<String>,
    pub merged: Option<bool>,
    pub node_id: Option<String>,
    pub resolved: Option<bool>,
}

#[derive(Debug, Error)]
pub enum MutationError {
    #[error("invalid GitHub mutation target")]
    InvalidTarget,
    #[error("GitHub installation token is missing")]
    MissingToken,
    #[error("Git push requires the ephemeral credential-helper transport")]
    GitTransportRequired,
    #[error("ephemeral Git transport failed")]
    GitTransportFailed,
    #[error("direct pushes to the default branch are prohibited")]
    DefaultBranchPushProhibited,
    #[error("repository requires native merge-queue handoff")]
    MergeQueueRequired,
    #[error("pull request head changed before the approved mutation")]
    StaleRemoteHead,
    #[error("review thread identity does not match the approved pull request")]
    ReviewThreadMismatch,
    #[error("GitHub App is not allowed to resolve this review thread")]
    ReviewThreadNotResolvable,
    #[error("GitHub GraphQL mutation was rejected")]
    GraphQlRejected,
    #[error("GitHub mutation transport ended ambiguously; reconcile before retrying")]
    AmbiguousTransport,
    #[error("GitHub rejected mutation with status {status}")]
    Rejected {
        status: StatusCode,
        retry_after: Option<u64>,
        request_id: Option<String>,
    },
    #[error("GitHub mutation response was invalid")]
    InvalidResponse,
    #[error("GitHub mutation client setup failed")]
    Client(#[from] reqwest::Error),
}
