use anyhow::{Context, Result, bail};
use reqwest::{Client, Url, header::LINK};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{fmt, path::PathBuf, process::Command, sync::Arc, time::Duration};

const API_VERSION: &str = "2026-03-10";

#[derive(Clone)]
pub struct GitHubToken(String);

impl GitHubToken {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
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
    #[serde(alias = "open_issues_count")]
    pub open_issues_count: u64,
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
    pub body: Option<String>,
    pub author: String,
    pub html_url: String,
    pub draft: bool,
    pub comments_count: u64,
    pub head_sha: Option<String>,
    pub updated_at: String,
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
        let mut work_items = issue_rows
            .into_iter()
            .filter(|item| item.pull_request.is_none())
            .map(|item| item.into_item(&repository.full_name, WorkItemKind::Issue, None))
            .collect::<Vec<_>>();
        work_items.extend(pull_rows.iter().cloned().map(|pull| {
            pull.item.into_item(
                &repository.full_name,
                WorkItemKind::PullRequest,
                Some((pull.draft, pull.head.sha)),
            )
        }));

        let discussions = self.discussions(&base, &pull_rows, resource_limit).await?;
        let checks = self.checks(&base, &pull_rows, resource_limit).await?;
        let workflow_runs = self
            .paginated_field(
                &format!("{base}/actions/runs?per_page=100"),
                "workflow_runs",
                resource_limit,
            )
            .await?;
        Ok(GitHubRepositorySnapshot {
            repository: repository.clone(),
            work_items,
            discussions,
            checks,
            workflow_runs,
        })
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

        Ok(discussions)
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

#[derive(Clone, Deserialize)]
struct User {
    login: String,
}
#[derive(Clone, Deserialize)]
struct Head {
    sha: String,
}
#[derive(Clone, Deserialize)]
struct WireItem {
    id: u64,
    number: u64,
    title: String,
    state: String,
    body: Option<String>,
    user: User,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    comments: u64,
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
    fn into_item(
        self,
        repository: &str,
        kind: WorkItemKind,
        pull: Option<(bool, String)>,
    ) -> GitHubWorkItem {
        let (draft, head_sha) = pull.map_or((self.draft, None), |(draft, sha)| (draft, Some(sha)));
        GitHubWorkItem {
            id: self.id,
            repository_full_name: repository.into(),
            number: self.number,
            kind,
            title: self.title,
            state: self.state,
            body: self.body,
            author: self.user.login,
            html_url: self.html_url,
            draft,
            comments_count: self.comments,
            head_sha,
            updated_at: self.updated_at,
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
    head: Head,
    #[serde(default)]
    draft: bool,
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
