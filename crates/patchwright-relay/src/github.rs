use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GitHubError {
    #[error("GitHub transport failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("GitHub rejected request with status {status}: {message}")]
    Rejected { status: StatusCode, message: String },
}

#[derive(Clone)]
pub struct GitHubClient {
    client: reqwest::Client,
    base_url: String,
    token: String,
}

impl GitHubClient {
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Result<Self, GitHubError> {
        let client = reqwest::Client::builder()
            .user_agent("Patchwright/0.1")
            .build()?;
        Ok(Self {
            client,
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            token: token.into(),
        })
    }

    pub async fn create_draft_pull_request(
        &self,
        owner: &str,
        repository: &str,
        title: &str,
        head: &str,
        base: &str,
        body: &str,
    ) -> Result<PullRequest, GitHubError> {
        self.request(
            Method::POST,
            &format!("/repos/{owner}/{repository}/pulls"),
            &serde_json::json!({"title":title,"head":head,"base":base,"body":body,"draft":true}),
        )
        .await
    }

    pub async fn create_check_run(
        &self,
        owner: &str,
        repository: &str,
        name: &str,
        head_sha: &str,
        status: &str,
    ) -> Result<CheckRun, GitHubError> {
        self.request(
            Method::POST,
            &format!("/repos/{owner}/{repository}/check-runs"),
            &serde_json::json!({"name":name,"head_sha":head_sha,"status":status}),
        )
        .await
    }

    async fn request<T: DeserializeOwned, B: Serialize>(
        &self,
        method: Method,
        path: &str,
        body: &B,
    ) -> Result<T, GitHubError> {
        let response = self
            .client
            .request(method, format!("{}{}", self.base_url, path))
            .bearer_auth(&self.token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2026-03-10")
            .json(body)
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "response body unavailable".into());
            return Err(GitHubError::Rejected { status, message });
        }
        Ok(response.json().await?)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct PullRequest {
    pub number: u64,
    pub html_url: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct CheckRun {
    pub id: u64,
    pub html_url: Option<String>,
}
