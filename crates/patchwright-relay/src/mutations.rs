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
                self.request(
                    Method::POST,
                    &format!("{prefix}/git/refs"),
                    json!({"ref":format!("refs/heads/{branch}"),"sha":from_sha}),
                )
                .await
            }
            GitHubAction::PushIntent { .. } => Err(MutationError::GitTransportRequired),
            GitHubAction::Comment { issue_number, body } => {
                self.request(
                    Method::POST,
                    &format!("{prefix}/issues/{issue_number}/comments"),
                    json!({"body":body}),
                )
                .await
            }
            GitHubAction::Review {
                pull_request_number,
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
                let mut payload = json!({"body":body,"comments":comments});
                if *event != ReviewEvent::Pending {
                    payload["event"] = Value::String(review_event(*event).to_owned());
                }
                self.request(
                    Method::POST,
                    &format!("{prefix}/pulls/{pull_request_number}/reviews"),
                    payload,
                )
                .await
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
                )
                .await
            }
            GitHubAction::ClosePullRequest {
                pull_request_number,
            } => {
                self.request(
                    Method::PATCH,
                    &format!("{prefix}/pulls/{pull_request_number}"),
                    json!({"state":"closed"}),
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
    ) -> Result<MutationResult, MutationError> {
        let url = self
            .base_url
            .join(path)
            .map_err(|_| MutationError::InvalidTarget)?;
        let response = self
            .client
            .request(method, url)
            .bearer_auth(self.token.expose_for_authorization_header())
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", API_VERSION)
            .json(&body)
            .send()
            .await
            .map_err(|_| MutationError::AmbiguousTransport)?;
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
        decode_result(response).await
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

async fn decode_result(response: reqwest::Response) -> Result<MutationResult, MutationError> {
    let value: Value = response
        .json()
        .await
        .map_err(|_| MutationError::InvalidResponse)?;
    Ok(MutationResult {
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
    })
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
}

#[derive(Debug, Error)]
pub enum MutationError {
    #[error("invalid GitHub mutation target")]
    InvalidTarget,
    #[error("GitHub installation token is missing")]
    MissingToken,
    #[error("Git push requires the ephemeral credential-helper transport")]
    GitTransportRequired,
    #[error("repository requires native merge-queue handoff")]
    MergeQueueRequired,
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
