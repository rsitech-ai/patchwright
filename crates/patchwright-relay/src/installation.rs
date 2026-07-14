use crate::{AppAuthenticator, GitHubAppError, PrivateKeyProvider, SecretString};
use chrono::{DateTime, Utc};
use reqwest::{Client, StatusCode, Url};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use tokio::sync::Mutex;

const API_VERSION: &str = "2022-11-28";
const CACHE_SAFETY_SECONDS: i64 = 60;

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Hash)]
#[serde(transparent)]
pub struct InstallationPermissions(BTreeMap<String, String>);

impl InstallationPermissions {
    #[must_use]
    pub fn delivery() -> Self {
        Self(BTreeMap::from([
            ("checks".into(), "write".into()),
            ("contents".into(), "write".into()),
            ("issues".into(), "write".into()),
            ("metadata".into(), "read".into()),
            ("pull_requests".into(), "write".into()),
        ]))
    }

    #[must_use]
    pub const fn as_map(&self) -> &BTreeMap<String, String> {
        &self.0
    }
}

pub struct InstallationToken {
    token: SecretString,
    installation_id: u64,
    repository_ids: Vec<u64>,
    permissions: InstallationPermissions,
    expires_at_epoch_seconds: i64,
}

impl Clone for InstallationToken {
    fn clone(&self) -> Self {
        Self {
            token: SecretString::new(self.token.expose_for_authorization_header()),
            installation_id: self.installation_id,
            repository_ids: self.repository_ids.clone(),
            permissions: self.permissions.clone(),
            expires_at_epoch_seconds: self.expires_at_epoch_seconds,
        }
    }
}

impl InstallationToken {
    #[must_use]
    pub fn expose_for_authorization_header(&self) -> &str {
        self.token.expose_for_authorization_header()
    }

    #[must_use]
    pub const fn installation_id(&self) -> u64 {
        self.installation_id
    }

    #[must_use]
    pub fn repository_ids(&self) -> &[u64] {
        &self.repository_ids
    }

    #[must_use]
    pub const fn permissions(&self) -> &InstallationPermissions {
        &self.permissions
    }
}

impl fmt::Debug for InstallationToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InstallationToken")
            .field("token", &"[REDACTED]")
            .field("installation_id", &self.installation_id)
            .field("repository_ids", &self.repository_ids)
            .field("permissions", &self.permissions)
            .field("expires_at_epoch_seconds", &self.expires_at_epoch_seconds)
            .finish()
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct CacheKey {
    owner: String,
    repository: String,
    repository_id: u64,
    permissions: InstallationPermissions,
}

pub struct InstallationBroker<P> {
    authenticator: AppAuthenticator<P>,
    client: Client,
    api_base_url: Url,
    cache: Mutex<HashMap<CacheKey, InstallationToken>>,
}

impl<P: fmt::Debug> fmt::Debug for InstallationBroker<P> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InstallationBroker")
            .field("authenticator", &self.authenticator)
            .field("client", &"reqwest::Client")
            .field("api_base_url", &self.api_base_url)
            .field("cache", &"[IN-MEMORY TOKENS REDACTED]")
            .finish()
    }
}

impl<P: PrivateKeyProvider> InstallationBroker<P> {
    pub fn new(
        authenticator: AppAuthenticator<P>,
        api_base_url: impl AsRef<str>,
    ) -> Result<Self, InstallationBrokerError> {
        let api_base_url = Url::parse(api_base_url.as_ref())
            .map_err(|_| InstallationBrokerError::InvalidApiUrl)?;
        if api_base_url.scheme() != "https"
            && !(api_base_url.scheme() == "http"
                && matches!(api_base_url.host_str(), Some("127.0.0.1" | "localhost")))
        {
            return Err(InstallationBrokerError::InvalidApiUrl);
        }
        Ok(Self {
            authenticator,
            client: Client::builder().user_agent("Patchwright/0.1").build()?,
            api_base_url,
            cache: Mutex::new(HashMap::new()),
        })
    }

    pub async fn token_for_repository(
        &self,
        owner: &str,
        repository: &str,
        repository_id: u64,
        permissions: InstallationPermissions,
        now_epoch_seconds: i64,
    ) -> Result<InstallationToken, InstallationBrokerError> {
        validate_name(owner)?;
        validate_name(repository)?;
        if repository_id == 0 {
            return Err(InstallationBrokerError::InvalidRepository);
        }
        let key = CacheKey {
            owner: owner.to_owned(),
            repository: repository.to_owned(),
            repository_id,
            permissions,
        };
        let mut cache = self.cache.lock().await;
        if let Some(cached) = cache.get(&key)
            && cached.expires_at_epoch_seconds - CACHE_SAFETY_SECONDS > now_epoch_seconds
        {
            return Ok(cached.clone());
        }
        let app_jwt = self.authenticator.app_jwt(now_epoch_seconds)?;
        let installation = self
            .discover_installation(owner, repository, &app_jwt)
            .await?;
        let token = self
            .mint_token(
                installation.id,
                repository_id,
                key.permissions.clone(),
                &app_jwt,
            )
            .await?;
        cache.insert(key, token.clone());
        Ok(token)
    }

    pub async fn revoke_cached_tokens(&self) {
        self.cache.lock().await.clear();
    }

    async fn discover_installation(
        &self,
        owner: &str,
        repository: &str,
        app_jwt: &SecretString,
    ) -> Result<InstallationResponse, InstallationBrokerError> {
        let url = self
            .api_base_url
            .join(&format!("repos/{owner}/{repository}/installation"))
            .map_err(|_| InstallationBrokerError::InvalidApiUrl)?;
        let response = self
            .client
            .get(url)
            .bearer_auth(app_jwt.expose_for_authorization_header())
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", API_VERSION)
            .send()
            .await?;
        parse_response(response).await
    }

    async fn mint_token(
        &self,
        installation_id: u64,
        repository_id: u64,
        permissions: InstallationPermissions,
        app_jwt: &SecretString,
    ) -> Result<InstallationToken, InstallationBrokerError> {
        let url = self
            .api_base_url
            .join(&format!(
                "app/installations/{installation_id}/access_tokens"
            ))
            .map_err(|_| InstallationBrokerError::InvalidApiUrl)?;
        let response = self
            .client
            .post(url)
            .bearer_auth(app_jwt.expose_for_authorization_header())
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", API_VERSION)
            .json(&MintTokenRequest {
                repository_ids: [repository_id],
                permissions: permissions.as_map(),
            })
            .send()
            .await?;
        let response: MintTokenResponse = parse_response(response).await?;
        let expires_at = DateTime::parse_from_rfc3339(&response.expires_at)
            .map_err(|_| InstallationBrokerError::InvalidResponse)?
            .with_timezone(&Utc)
            .timestamp();
        Ok(InstallationToken {
            token: SecretString::new(response.token),
            installation_id,
            repository_ids: vec![repository_id],
            permissions,
            expires_at_epoch_seconds: expires_at,
        })
    }
}

fn validate_name(value: &str) -> Result<(), InstallationBrokerError> {
    if value.is_empty()
        || value.len() > 100
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(InstallationBrokerError::InvalidRepository);
    }
    Ok(())
}

async fn parse_response<T: for<'de> Deserialize<'de>>(
    response: reqwest::Response,
) -> Result<T, InstallationBrokerError> {
    let status = response.status();
    if !status.is_success() {
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse().ok());
        return Err(InstallationBrokerError::GitHubRejected {
            status,
            retry_after,
        });
    }
    response
        .json()
        .await
        .map_err(|_| InstallationBrokerError::InvalidResponse)
}

#[derive(Deserialize)]
struct InstallationResponse {
    id: u64,
}

#[derive(Serialize)]
struct MintTokenRequest<'a> {
    repository_ids: [u64; 1],
    permissions: &'a BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct MintTokenResponse {
    token: String,
    expires_at: String,
}

#[derive(Debug, thiserror::Error)]
pub enum InstallationBrokerError {
    #[error(transparent)]
    App(#[from] GitHubAppError),
    #[error("invalid GitHub API URL")]
    InvalidApiUrl,
    #[error("invalid repository identity")]
    InvalidRepository,
    #[error("GitHub App transport failed")]
    Transport(#[from] reqwest::Error),
    #[error("GitHub App request was rejected with status {status}")]
    GitHubRejected {
        status: StatusCode,
        retry_after: Option<u64>,
    },
    #[error("GitHub App returned an invalid response")]
    InvalidResponse,
}
