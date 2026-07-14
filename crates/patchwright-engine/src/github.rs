use anyhow::{Context, Result, bail};
use reqwest::{Client, Url, header::LINK};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::DeserializeOwned};
use std::{
    collections::{HashMap, HashSet},
    fmt,
    path::PathBuf,
    process::Command,
    sync::Arc,
    time::Duration,
};

const API_VERSION: &str = "2026-03-10";

#[derive(Clone)]
pub struct GitHubToken(String);

impl GitHubToken {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub(crate) fn expose_for_authorization_header(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for GitHubToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("GitHubToken([REDACTED])")
    }
}

#[derive(Clone, Debug)]
pub struct GhCliCredentialBroker {
    executable: PathBuf,
}

impl GhCliCredentialBroker {
    #[must_use]
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
        }
    }

    pub fn token(&self) -> Result<GitHubToken> {
        let output = Command::new(&self.executable)
            .args(["auth", "token"])
            .output()
            .context("run GitHub credential broker")?;
        if !output.status.success() {
            bail!("GitHub CLI is not authenticated");
        }
        let token = String::from_utf8(output.stdout)
            .context("GitHub credential was not UTF-8")?
            .trim()
            .to_owned();
        if token.is_empty() {
            bail!("GitHub CLI returned an empty credential");
        }
        Ok(GitHubToken::new(token))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GitHubAccount {
    pub login: String,
    #[serde(alias = "avatar_url")]
    pub avatar_url: String,
    #[serde(alias = "html_url")]
    pub html_url: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GitHubRepository {
    pub id: u64,
    #[serde(alias = "full_name")]
    pub full_name: String,
    pub description: Option<String>,
    pub private: bool,
    pub archived: bool,
    #[serde(alias = "default_branch")]
    pub default_branch: String,
    #[serde(alias = "html_url")]
    pub html_url: String,
    #[serde(alias = "updated_at")]
    pub updated_at: String,
    #[serde(default, alias = "pushed_at")]
    pub pushed_at: Option<String>,
    #[serde(alias = "open_issues_count")]
    pub open_issues_count: u64,
    #[serde(default)]
    pub open_pull_request_count: u64,
    #[serde(default)]
    pub failing_check_count: u64,
    #[serde(default)]
    pub default_branch_sha: Option<String>,
    #[serde(default)]
    pub default_branch_committed_at: Option<String>,
    #[serde(default, alias = "installation_id")]
    pub installation_id: Option<u64>,
    #[serde(default)]
    pub permissions: GitHubRepositoryPermissions,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GitHubRepositoryPermissions {
    #[serde(default)]
    pub admin: GitHubPermission,
    #[serde(default)]
    pub maintain: GitHubPermission,
    #[serde(default)]
    pub push: GitHubPermission,
    #[serde(default)]
    pub triage: GitHubPermission,
    #[serde(default)]
    pub pull: GitHubPermission,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum GitHubPermission {
    #[default]
    Denied,
    Granted,
}

impl GitHubPermission {
    #[must_use]
    pub const fn is_granted(self) -> bool {
        matches!(self, Self::Granted)
    }
}

impl Serialize for GitHubPermission {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bool(self.is_granted())
    }
}

impl<'de> Deserialize<'de> for GitHubPermission {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(if bool::deserialize(deserializer)? {
            Self::Granted
        } else {
            Self::Denied
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum WorkItemKind {
    Issue,
    PullRequest,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GitHubWorkItem {
    pub id: u64,
    pub repository_full_name: String,
    pub number: u64,
    pub kind: WorkItemKind,
    pub title: String,
    pub state: String,
    #[serde(default)]
    pub state_reason: Option<String>,
    pub body: Option<String>,
    pub author: String,
    pub html_url: String,
    pub draft: bool,
    pub comments_count: u64,
    #[serde(default)]
    pub base_ref: Option<String>,
    #[serde(default)]
    pub base_sha: Option<String>,
    #[serde(default)]
    pub head_ref: Option<String>,
    pub head_sha: Option<String>,
    #[serde(default)]
    pub merged: Option<bool>,
    #[serde(default)]
    pub merge_commit_sha: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub head_committed_at: Option<String>,
    #[serde(default)]
    pub latest_review_at: Option<String>,
    pub updated_at: String,
    #[serde(default)]
    pub review_decision: Option<String>,
    #[serde(default)]
    pub ci_health: Option<String>,
    #[serde(default)]
    pub mergeable: Option<bool>,
    #[serde(default)]
    pub mergeable_state: Option<String>,
    #[serde(default)]
    pub rebaseable: Option<bool>,
    #[serde(default)]
    pub has_conflicts: Option<bool>,
    #[serde(default)]
    pub head_repository_full_name: Option<String>,
    #[serde(default)]
    pub head_repository_fork: bool,
    #[serde(default)]
    pub maintainer_can_modify: bool,
    #[serde(default)]
    pub additions: u64,
    #[serde(default)]
    pub deletions: u64,
    #[serde(default)]
    pub changed_files: u64,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub assignees: Vec<String>,
    #[serde(default)]
    pub milestone: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GitHubDiscussion {
    pub id: u64,
    pub item_number: u64,
    pub kind: String,
    pub author: String,
    pub body: Option<String>,
    pub state: Option<String>,
    pub path: Option<String>,
    pub line: Option<u64>,
    pub html_url: String,
    pub updated_at: Option<String>,
    #[serde(default)]
    pub thread_node_id: Option<String>,
    #[serde(default)]
    pub thread_resolved: Option<bool>,
    #[serde(default)]
    pub thread_outdated: Option<bool>,
    #[serde(default)]
    pub viewer_can_resolve: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GitHubCheckRun {
    pub id: u64,
    pub item_number: u64,
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub html_url: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GitHubWorkflowRun {
    pub id: u64,
    pub name: Option<String>,
    pub status: Option<String>,
    pub conclusion: Option<String>,
    pub event: String,
    #[serde(alias = "head_sha")]
    pub head_sha: String,
    #[serde(alias = "html_url")]
    pub html_url: String,
    #[serde(alias = "updated_at")]
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GitHubRepositorySnapshot {
    pub repository: GitHubRepository,
    pub work_items: Vec<GitHubWorkItem>,
    pub discussions: Vec<GitHubDiscussion>,
    pub checks: Vec<GitHubCheckRun>,
    pub workflow_runs: Vec<GitHubWorkflowRun>,
}

#[derive(Clone, Debug, Serialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GitHubSyncSummary {
    pub account: GitHubAccount,
    pub repositories_discovered: usize,
    pub repositories_synced: usize,
    pub work_items: usize,
    pub discussions: usize,
    pub checks: usize,
    pub workflow_runs: usize,
    pub failures: Vec<String>,
}

#[derive(Clone)]
pub struct GitHubSource {
    client: Client,
    base_url: Url,
    token: GitHubToken,
}

impl GitHubSource {
    pub fn new(base_url: impl AsRef<str>, token: GitHubToken) -> Result<Self> {
        let client = Client::builder()
            .user_agent("Patchwright/0.1")
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()?;
        Ok(Self {
            client,
            base_url: Url::parse(base_url.as_ref()).context("invalid GitHub API URL")?,
            token,
        })
    }

    pub async fn account(&self) -> Result<GitHubAccount> {
        self.get("/user").await
    }

    pub async fn repositories(&self, limit: usize) -> Result<Vec<GitHubRepository>> {
        self.paginated("/user/repos?affiliation=owner,collaborator,organization_member&sort=updated&direction=desc&per_page=100", limit).await
    }

    pub async fn repository(&self, full_name: &str) -> Result<GitHubRepository> {
        let (owner, name) = full_name
            .split_once('/')
            .context("repository full name lacks owner")?;
        if owner.is_empty() || name.is_empty() || name.contains('/') {
            bail!("repository full name is invalid");
        }
        let repository: GitHubRepository = self
            .get(&format!(
                "/repos/{}/{}",
                encode_path_segment(owner),
                encode_path_segment(name)
            ))
            .await?;
        if repository.full_name != full_name {
            bail!("GitHub returned a different repository identity");
        }
        Ok(repository)
    }

    pub async fn repository_snapshot(
        &self,
        repository: &GitHubRepository,
        resource_limit: usize,
    ) -> Result<GitHubRepositorySnapshot> {
        let (owner, name) = repository
            .full_name
            .split_once('/')
            .context("repository full name lacks owner")?;
        let base = format!("/repos/{owner}/{name}");
        let issue_rows: Vec<WireItem> = self
            .paginated(
                &format!("{base}/issues?state=all&per_page=100"),
                resource_limit,
            )
            .await?;
        let pull_rows: Vec<WirePull> = self
            .paginated(
                &format!("{base}/pulls?state=all&per_page=100"),
                resource_limit,
            )
            .await?;
        let default_branch_commit: WireCommit = self
            .get(&format!(
                "{base}/commits/{}",
                encode_path_segment(&repository.default_branch)
            ))
            .await?;
        let enriched_pulls = self.enrich_pulls(&base, pull_rows).await?;
        let pull_rows = enriched_pulls
            .iter()
            .map(|pull| pull.pull.clone())
            .collect::<Vec<_>>();
        let discussions = self.discussions(&base, &pull_rows, resource_limit).await?;
        let checks = self.checks(&base, &pull_rows, resource_limit).await?;
        let mut work_items = issue_rows
            .into_iter()
            .filter(|item| item.pull_request.is_none())
            .map(|item| item.into_item(&repository.full_name, WorkItemKind::Issue))
            .collect::<Vec<_>>();
        work_items.extend(enriched_pulls.into_iter().map(|pull| {
            let item_number = pull.pull.item.number;
            let item_discussions = discussions
                .iter()
                .filter(|entry| entry.item_number == item_number)
                .collect::<Vec<_>>();
            let item_checks = checks
                .iter()
                .filter(|entry| entry.item_number == item_number)
                .collect::<Vec<_>>();
            pull.into_item(
                &repository.full_name,
                latest_review_at(&item_discussions),
                review_decision(&item_discussions),
                ci_health(&item_checks),
            )
        }));
        let workflow_runs = self
            .paginated_field(
                &format!("{base}/actions/runs?per_page=100"),
                "workflow_runs",
                resource_limit,
            )
            .await?;
        let mut repository = repository.clone();
        repository.default_branch_sha = Some(default_branch_commit.sha);
        repository.default_branch_committed_at = Some(default_branch_commit.commit.committer.date);
        repository.open_pull_request_count = pull_rows
            .iter()
            .filter(|pull| pull.item.state == "open")
            .count() as u64;
        let open_pull_requests = pull_rows
            .iter()
            .filter(|pull| pull.item.state == "open")
            .map(|pull| pull.item.number)
            .collect::<HashSet<_>>();
        repository.failing_check_count = checks
            .iter()
            .filter(|check| {
                open_pull_requests.contains(&check.item_number)
                    && is_failing_conclusion(check.conclusion.as_deref())
            })
            .count() as u64;
        Ok(GitHubRepositorySnapshot {
            repository,
            work_items,
            discussions,
            checks,
            workflow_runs,
        })
    }

    async fn enrich_pulls(
        &self,
        base: &str,
        pull_rows: Vec<WirePull>,
    ) -> Result<Vec<EnrichedPull>> {
        let source = Arc::new(self.clone());
        let concurrency = Arc::new(tokio::sync::Semaphore::new(8));
        let mut jobs = tokio::task::JoinSet::new();
        for pull in &pull_rows {
            let source = Arc::clone(&source);
            let concurrency = Arc::clone(&concurrency);
            let base = base.to_owned();
            let detail_path = format!("{base}/pulls/{}", pull.item.number);
            jobs.spawn(async move {
                let _permit = concurrency.acquire_owned().await?;
                let detail: WirePull = source.get(&detail_path).await?;
                let commit: WireCommit = source
                    .get(&format!("{base}/commits/{}", detail.head.sha))
                    .await?;
                Ok::<_, anyhow::Error>(EnrichedPull {
                    pull: detail,
                    head_committed_at: Some(commit.commit.committer.date),
                })
            });
        }
        let mut enriched = Vec::new();
        while let Some(result) = jobs.join_next().await {
            enriched.push(result.context("join GitHub pull enrichment")??);
        }
        enriched.sort_by_key(|pull| pull.pull.item.number);
        Ok(enriched)
    }

    async fn discussions(
        &self,
        base: &str,
        pull_rows: &[WirePull],
        resource_limit: usize,
    ) -> Result<Vec<GitHubDiscussion>> {
        let issue_comments: Vec<WireComment> = self
            .paginated(
                &format!("{base}/issues/comments?per_page=100"),
                resource_limit,
            )
            .await?;
        let review_comments: Vec<WireReviewComment> = self
            .paginated(
                &format!("{base}/pulls/comments?per_page=100"),
                resource_limit,
            )
            .await?;
        let mut discussions = issue_comments
            .into_iter()
            .filter_map(|value| value.into_discussion("issueComment"))
            .collect::<Vec<_>>();
        discussions.extend(
            review_comments
                .into_iter()
                .filter_map(WireReviewComment::into_discussion),
        );
        let source = Arc::new(self.clone());
        let concurrency = Arc::new(tokio::sync::Semaphore::new(8));
        let mut jobs = tokio::task::JoinSet::new();
        for pull in pull_rows {
            let source = Arc::clone(&source);
            let concurrency = Arc::clone(&concurrency);
            let path = format!("{base}/pulls/{}/reviews?per_page=100", pull.item.number);
            let number = pull.item.number;
            jobs.spawn(async move {
                let _permit = concurrency.acquire_owned().await?;
                let reviews: Vec<WireReview> = source.paginated(&path, resource_limit).await?;
                Ok::<_, anyhow::Error>((number, reviews))
            });
        }
        while let Some(result) = jobs.join_next().await {
            let (number, reviews) = result.context("join GitHub review fetch")??;
            discussions.extend(
                reviews
                    .into_iter()
                    .map(|value| value.into_discussion(number)),
            );
        }

        discussions.extend(self.review_threads(base, pull_rows, resource_limit).await?);

        Ok(discussions)
    }

    async fn review_threads(
        &self,
        base: &str,
        pull_rows: &[WirePull],
        resource_limit: usize,
    ) -> Result<Vec<GitHubDiscussion>> {
        let repository = base
            .strip_prefix("/repos/")
            .context("review thread repository path is invalid")?;
        let (owner, name) = repository
            .split_once('/')
            .context("review thread repository identity is invalid")?;
        let source = Arc::new(self.clone());
        let concurrency = Arc::new(tokio::sync::Semaphore::new(8));
        let mut jobs = tokio::task::JoinSet::new();
        for pull in pull_rows.iter().filter(|pull| pull.item.state == "open") {
            let source = Arc::clone(&source);
            let concurrency = Arc::clone(&concurrency);
            let owner = owner.to_owned();
            let name = name.to_owned();
            let number = pull.item.number;
            jobs.spawn(async move {
                let _permit = concurrency.acquire_owned().await?;
                source
                    .review_threads_for_pull(&owner, &name, number, resource_limit)
                    .await
            });
        }
        let mut threads = Vec::new();
        while let Some(result) = jobs.join_next().await {
            threads.extend(result.context("join GitHub review thread fetch")??);
        }
        Ok(threads)
    }

    async fn review_threads_for_pull(
        &self,
        owner: &str,
        name: &str,
        number: u64,
        resource_limit: usize,
    ) -> Result<Vec<GitHubDiscussion>> {
        let mut after: Option<String> = None;
        let mut result = Vec::new();
        loop {
            let response = self
                .graphql(&serde_json::json!({
                    "query":"query PatchwrightReviewThreads($owner: String!, $name: String!, $number: Int!, $after: String) { repository(owner: $owner, name: $name) { pullRequest(number: $number) { reviewThreads(first: 100, after: $after) { nodes { id isResolved isOutdated viewerCanResolve path line comments(first: 1) { nodes { databaseId body author { login } url updatedAt } } } pageInfo { hasNextPage endCursor } } } } }",
                    "variables":{"owner":owner,"name":name,"number":number,"after":after}
                }))
                .await?;
            if response.get("errors").is_some() {
                bail!("GitHub GraphQL review thread query failed");
            }
            let connection = response
                .pointer("/data/repository/pullRequest/reviewThreads")
                .context("GitHub review thread response was incomplete")?;
            let nodes = connection
                .get("nodes")
                .and_then(serde_json::Value::as_array)
                .context("GitHub review thread nodes were missing")?;
            for node in nodes {
                let Some(comment) = node.pointer("/comments/nodes/0") else {
                    continue;
                };
                let Some(id) = comment
                    .get("databaseId")
                    .and_then(serde_json::Value::as_u64)
                else {
                    continue;
                };
                result.push(GitHubDiscussion {
                    id,
                    item_number: number,
                    kind: "reviewThread".into(),
                    author: comment
                        .pointer("/author/login")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("ghost")
                        .to_owned(),
                    body: comment
                        .get("body")
                        .and_then(serde_json::Value::as_str)
                        .map(ToOwned::to_owned),
                    state:
                        Some(
                            if node.get("isResolved").and_then(serde_json::Value::as_bool)
                                == Some(true)
                            {
                                "resolved"
                            } else {
                                "unresolved"
                            }
                            .into(),
                        ),
                    path: node
                        .get("path")
                        .and_then(serde_json::Value::as_str)
                        .map(ToOwned::to_owned),
                    line: node.get("line").and_then(serde_json::Value::as_u64),
                    html_url: comment
                        .get("url")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    updated_at: comment
                        .get("updatedAt")
                        .and_then(serde_json::Value::as_str)
                        .map(ToOwned::to_owned),
                    thread_node_id: node
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                        .map(ToOwned::to_owned),
                    thread_resolved: node.get("isResolved").and_then(serde_json::Value::as_bool),
                    thread_outdated: node.get("isOutdated").and_then(serde_json::Value::as_bool),
                    viewer_can_resolve: node
                        .get("viewerCanResolve")
                        .and_then(serde_json::Value::as_bool),
                });
                if result.len() >= resource_limit {
                    return Ok(result);
                }
            }
            if connection
                .pointer("/pageInfo/hasNextPage")
                .and_then(serde_json::Value::as_bool)
                != Some(true)
            {
                break;
            }
            after = connection
                .pointer("/pageInfo/endCursor")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned);
            if after.is_none() {
                bail!("GitHub review thread pagination cursor was missing");
            }
        }
        Ok(result)
    }

    async fn checks(
        &self,
        base: &str,
        pull_rows: &[WirePull],
        resource_limit: usize,
    ) -> Result<Vec<GitHubCheckRun>> {
        let mut checks = Vec::new();
        let source = Arc::new(self.clone());
        let concurrency = Arc::new(tokio::sync::Semaphore::new(8));
        let mut jobs = tokio::task::JoinSet::new();
        for pull in pull_rows {
            let source = Arc::clone(&source);
            let concurrency = Arc::clone(&concurrency);
            let path = format!("{base}/commits/{}/check-runs?per_page=100", pull.head.sha);
            let number = pull.item.number;
            jobs.spawn(async move {
                let _permit = concurrency.acquire_owned().await?;
                let response: Vec<WireCheckRun> = source
                    .paginated_field(&path, "check_runs", resource_limit)
                    .await?;
                Ok::<_, anyhow::Error>((number, response))
            });
        }
        while let Some(result) = jobs.join_next().await {
            let (number, response) = result.context("join GitHub check fetch")??;
            checks.extend(response.into_iter().map(|run| GitHubCheckRun {
                id: run.id,
                item_number: number,
                name: run.name,
                status: run.status,
                conclusion: run.conclusion,
                html_url: run.html_url,
            }));
        }
        Ok(checks)
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = self.base_url.join(path).context("build GitHub API URL")?;
        let response = self
            .client
            .get(url)
            .bearer_auth(&self.token.0)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", API_VERSION)
            .send()
            .await
            .context("GitHub request failed")?;
        let status = response.status();
        if !status.is_success() {
            bail!("GitHub API returned {status}");
        }
        response.json().await.context("decode GitHub response")
    }

    async fn graphql(&self, body: &serde_json::Value) -> Result<serde_json::Value> {
        let url = self
            .base_url
            .join("/graphql")
            .context("build GraphQL URL")?;
        let response = self
            .client
            .post(url)
            .bearer_auth(&self.token.0)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", API_VERSION)
            .json(body)
            .send()
            .await
            .context("GitHub GraphQL request failed")?;
        let status = response.status();
        if !status.is_success() {
            bail!("GitHub GraphQL returned {status}");
        }
        response
            .json()
            .await
            .context("decode GitHub GraphQL response")
    }

    async fn paginated<T: DeserializeOwned>(
        &self,
        initial_path: &str,
        limit: usize,
    ) -> Result<Vec<T>> {
        let mut next = Some(
            self.base_url
                .join(initial_path)
                .context("build paginated GitHub URL")?,
        );
        let mut values = Vec::new();
        while let Some(url) = next.take() {
            let response = self
                .client
                .get(url)
                .bearer_auth(&self.token.0)
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", API_VERSION)
                .send()
                .await
                .context("GitHub paginated request failed")?;
            let status = response.status();
            if !status.is_success() {
                bail!("GitHub API returned {status}");
            }
            next = response
                .headers()
                .get(LINK)
                .and_then(|value| value.to_str().ok())
                .and_then(next_link)
                .and_then(|path| self.same_origin_url(&path));
            values.extend(
                response
                    .json::<Vec<T>>()
                    .await
                    .context("decode GitHub page")?,
            );
            if values.len() >= limit {
                values.truncate(limit);
                break;
            }
        }
        Ok(values)
    }

    async fn paginated_field<T: DeserializeOwned>(
        &self,
        initial_path: &str,
        field: &str,
        limit: usize,
    ) -> Result<Vec<T>> {
        let mut next = Some(
            self.base_url
                .join(initial_path)
                .context("build paginated GitHub URL")?,
        );
        let mut values = Vec::new();
        while let Some(url) = next.take() {
            let response = self
                .client
                .get(url)
                .bearer_auth(&self.token.0)
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", API_VERSION)
                .send()
                .await
                .context("GitHub paginated request failed")?;
            let status = response.status();
            if !status.is_success() {
                bail!("GitHub API returned {status}");
            }
            next = response
                .headers()
                .get(LINK)
                .and_then(|value| value.to_str().ok())
                .and_then(next_link)
                .and_then(|path| self.same_origin_url(&path));
            let mut object = response
                .json::<serde_json::Map<String, serde_json::Value>>()
                .await
                .context("decode GitHub page")?;
            let page = object
                .remove(field)
                .with_context(|| format!("GitHub response omitted {field}"))?;
            values.extend(
                serde_json::from_value::<Vec<T>>(page)
                    .with_context(|| format!("decode GitHub {field} page"))?,
            );
            if values.len() >= limit {
                values.truncate(limit);
                break;
            }
        }
        Ok(values)
    }

    fn same_origin_url(&self, value: &str) -> Option<Url> {
        let candidate = self.base_url.join(value).ok()?;
        (candidate.origin() == self.base_url.origin()).then_some(candidate)
    }
}

fn next_link(header: &str) -> Option<String> {
    header.split(',').find_map(|part| {
        let (url, relation) = part.trim().split_once(';')?;
        (relation.trim() == "rel=\"next\"").then(|| {
            url.trim()
                .trim_start_matches('<')
                .trim_end_matches('>')
                .to_owned()
        })
    })
}

fn latest_review_at(discussions: &[&GitHubDiscussion]) -> Option<String> {
    discussions
        .iter()
        .filter(|entry| entry.kind.starts_with("review"))
        .filter_map(|entry| entry.updated_at.as_ref())
        .max()
        .cloned()
}

fn review_decision(discussions: &[&GitHubDiscussion]) -> Option<String> {
    let mut latest_by_reviewer = HashMap::<&str, (&str, &str)>::new();
    for entry in discussions.iter().filter(|entry| entry.kind == "review") {
        let Some(state) = entry.state.as_deref() else {
            continue;
        };
        let submitted_at = entry.updated_at.as_deref().unwrap_or_default();
        let replace = latest_by_reviewer
            .get(entry.author.as_str())
            .is_none_or(|(current, _)| submitted_at >= *current);
        if replace {
            latest_by_reviewer.insert(&entry.author, (submitted_at, state));
        }
    }
    let states = latest_by_reviewer
        .values()
        .map(|(_, state)| state.to_ascii_lowercase())
        .collect::<Vec<_>>();
    if states
        .iter()
        .any(|state| matches!(state.as_str(), "changes_requested" | "changesrequested"))
    {
        Some("changesRequested".into())
    } else if states.iter().any(|state| state == "approved") {
        Some("approved".into())
    } else if states.is_empty() {
        None
    } else {
        Some("reviewRequired".into())
    }
}

fn ci_health(checks: &[&GitHubCheckRun]) -> String {
    if checks.is_empty() {
        return "unknown".into();
    }
    if checks.iter().any(|check| check.status != "completed") {
        return "pending".into();
    }
    if checks
        .iter()
        .any(|check| is_failing_conclusion(check.conclusion.as_deref()))
    {
        return "failing".into();
    }
    if checks.iter().all(|check| {
        matches!(
            check.conclusion.as_deref(),
            Some("success" | "neutral" | "skipped")
        )
    }) {
        "passing".into()
    } else {
        "unknown".into()
    }
}

fn is_failing_conclusion(conclusion: Option<&str>) -> bool {
    matches!(
        conclusion,
        Some(
            "failure" | "timed_out" | "cancelled" | "action_required" | "startup_failure" | "stale"
        )
    )
}

fn encode_path_segment(value: &str) -> String {
    value
        .bytes()
        .fold(String::with_capacity(value.len()), |mut encoded, byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
                encoded.push(char::from(byte));
            } else {
                use std::fmt::Write as _;
                write!(encoded, "%{byte:02X}").expect("writing to String cannot fail");
            }
            encoded
        })
}

#[derive(Clone, Deserialize)]
struct User {
    login: String,
}
#[derive(Clone, Deserialize)]
struct WireRef {
    sha: String,
    #[serde(rename = "ref")]
    ref_name: String,
    #[serde(default)]
    repo: Option<WireRepositoryIdentity>,
}
#[derive(Clone, Deserialize)]
struct WireRepositoryIdentity {
    full_name: String,
    #[serde(default)]
    fork: bool,
}
#[derive(Clone, Deserialize)]
struct WireItem {
    id: u64,
    number: u64,
    title: String,
    state: String,
    #[serde(default)]
    state_reason: Option<String>,
    body: Option<String>,
    user: User,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    comments: u64,
    #[serde(default)]
    created_at: Option<String>,
    updated_at: String,
    #[serde(default)]
    pull_request: Option<serde_json::Value>,
    #[serde(default)]
    labels: Vec<WireLabel>,
    #[serde(default)]
    assignees: Vec<User>,
    #[serde(default)]
    milestone: Option<WireMilestone>,
}
impl WireItem {
    fn into_item(self, repository: &str, kind: WorkItemKind) -> GitHubWorkItem {
        GitHubWorkItem {
            id: self.id,
            repository_full_name: repository.into(),
            number: self.number,
            kind,
            title: self.title,
            state: self.state,
            state_reason: self.state_reason,
            body: self.body,
            author: self.user.login,
            html_url: self.html_url,
            draft: self.draft,
            comments_count: self.comments,
            base_ref: None,
            base_sha: None,
            head_ref: None,
            head_sha: None,
            merged: None,
            merge_commit_sha: None,
            created_at: self.created_at,
            head_committed_at: None,
            latest_review_at: None,
            updated_at: self.updated_at,
            review_decision: None,
            ci_health: None,
            mergeable: None,
            mergeable_state: None,
            rebaseable: None,
            has_conflicts: None,
            head_repository_full_name: None,
            head_repository_fork: false,
            maintainer_can_modify: false,
            additions: 0,
            deletions: 0,
            changed_files: 0,
            labels: self.labels.into_iter().map(|label| label.name).collect(),
            assignees: self.assignees.into_iter().map(|user| user.login).collect(),
            milestone: self.milestone.map(|milestone| milestone.title),
        }
    }
}

#[derive(Clone, Deserialize)]
struct WireLabel {
    name: String,
}

#[derive(Clone, Deserialize)]
struct WireMilestone {
    title: String,
}
#[derive(Clone, Deserialize)]
struct WirePull {
    #[serde(flatten)]
    item: WireItem,
    head: WireRef,
    base: WireRef,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    maintainer_can_modify: bool,
    #[serde(default)]
    mergeable: Option<bool>,
    #[serde(default)]
    mergeable_state: Option<String>,
    #[serde(default)]
    rebaseable: Option<bool>,
    #[serde(default)]
    merged: Option<bool>,
    #[serde(default)]
    merge_commit_sha: Option<String>,
    #[serde(default)]
    additions: u64,
    #[serde(default)]
    deletions: u64,
    #[serde(default)]
    changed_files: u64,
}

struct EnrichedPull {
    pull: WirePull,
    head_committed_at: Option<String>,
}

impl EnrichedPull {
    fn into_item(
        self,
        repository: &str,
        latest_review_at: Option<String>,
        review_decision: Option<String>,
        ci_health: String,
    ) -> GitHubWorkItem {
        let head_repository_full_name = self
            .pull
            .head
            .repo
            .as_ref()
            .map(|repository| repository.full_name.clone());
        let head_repository_fork = self
            .pull
            .head
            .repo
            .as_ref()
            .is_some_and(|repository| repository.fork);
        GitHubWorkItem {
            id: self.pull.item.id,
            repository_full_name: repository.into(),
            number: self.pull.item.number,
            kind: WorkItemKind::PullRequest,
            title: self.pull.item.title,
            state: self.pull.item.state,
            state_reason: self.pull.item.state_reason,
            body: self.pull.item.body,
            author: self.pull.item.user.login,
            html_url: self.pull.item.html_url,
            draft: self.pull.draft,
            comments_count: self.pull.item.comments,
            base_ref: Some(self.pull.base.ref_name),
            base_sha: Some(self.pull.base.sha),
            head_ref: Some(self.pull.head.ref_name),
            head_sha: Some(self.pull.head.sha),
            merged: self.pull.merged,
            merge_commit_sha: self.pull.merge_commit_sha,
            created_at: self.pull.item.created_at,
            head_committed_at: self.head_committed_at,
            latest_review_at,
            updated_at: self.pull.item.updated_at,
            review_decision,
            ci_health: Some(ci_health),
            mergeable: self.pull.mergeable,
            mergeable_state: self.pull.mergeable_state.clone(),
            rebaseable: self.pull.rebaseable,
            has_conflicts: self.pull.mergeable.map(|mergeable| {
                !mergeable && self.pull.mergeable_state.as_deref() == Some("dirty")
            }),
            head_repository_full_name,
            head_repository_fork,
            maintainer_can_modify: self.pull.maintainer_can_modify,
            additions: self.pull.additions,
            deletions: self.pull.deletions,
            changed_files: self.pull.changed_files,
            labels: self
                .pull
                .item
                .labels
                .into_iter()
                .map(|label| label.name)
                .collect(),
            assignees: self
                .pull
                .item
                .assignees
                .into_iter()
                .map(|user| user.login)
                .collect(),
            milestone: self.pull.item.milestone.map(|milestone| milestone.title),
        }
    }
}

#[derive(Deserialize)]
struct WireCommit {
    sha: String,
    commit: WireCommitMetadata,
}

#[derive(Deserialize)]
struct WireCommitMetadata {
    committer: WireCommitter,
}

#[derive(Deserialize)]
struct WireCommitter {
    date: String,
}
#[derive(Deserialize)]
struct WireComment {
    id: u64,
    body: Option<String>,
    user: User,
    html_url: String,
    issue_url: String,
    updated_at: Option<String>,
}
impl WireComment {
    fn into_discussion(self, kind: &str) -> Option<GitHubDiscussion> {
        Some(GitHubDiscussion {
            id: self.id,
            item_number: self.issue_url.rsplit('/').next()?.parse().ok()?,
            kind: kind.into(),
            author: self.user.login,
            body: self.body,
            state: None,
            path: None,
            line: None,
            html_url: self.html_url,
            updated_at: self.updated_at,
            thread_node_id: None,
            thread_resolved: None,
            thread_outdated: None,
            viewer_can_resolve: None,
        })
    }
}
#[derive(Deserialize)]
struct WireReviewComment {
    id: u64,
    body: Option<String>,
    user: User,
    html_url: String,
    pull_request_url: String,
    path: Option<String>,
    line: Option<u64>,
    updated_at: Option<String>,
}
impl WireReviewComment {
    fn into_discussion(self) -> Option<GitHubDiscussion> {
        Some(GitHubDiscussion {
            id: self.id,
            item_number: self.pull_request_url.rsplit('/').next()?.parse().ok()?,
            kind: "reviewComment".into(),
            author: self.user.login,
            body: self.body,
            state: None,
            path: self.path,
            line: self.line,
            html_url: self.html_url,
            updated_at: self.updated_at,
            thread_node_id: None,
            thread_resolved: None,
            thread_outdated: None,
            viewer_can_resolve: None,
        })
    }
}
#[derive(Deserialize)]
struct WireReview {
    id: u64,
    body: Option<String>,
    user: User,
    html_url: String,
    state: String,
    submitted_at: Option<String>,
}
impl WireReview {
    fn into_discussion(self, number: u64) -> GitHubDiscussion {
        GitHubDiscussion {
            id: self.id,
            item_number: number,
            kind: "review".into(),
            author: self.user.login,
            body: self.body,
            state: Some(self.state),
            path: None,
            line: None,
            html_url: self.html_url,
            updated_at: self.submitted_at,
            thread_node_id: None,
            thread_resolved: None,
            thread_outdated: None,
            viewer_can_resolve: None,
        }
    }
}
#[derive(Deserialize)]
struct WireCheckRun {
    id: u64,
    name: String,
    status: String,
    conclusion: Option<String>,
    html_url: Option<String>,
}
