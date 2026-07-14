use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use reqwest::Url;
use security_framework::passwords::{get_generic_password, set_generic_password};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", tag = "source")]
pub enum KeyReference {
    ProtectedFile { path: PathBuf },
    Keychain { service: String, account: String },
}

impl KeyReference {
    #[must_use]
    pub fn protected_file(path: impl Into<PathBuf>) -> Self {
        Self::ProtectedFile { path: path.into() }
    }

    pub fn parse(value: &str) -> Result<Self, GitHubAppError> {
        if let Some(path) = value.strip_prefix("file:") {
            return Ok(Self::protected_file(path));
        }
        if let Some(reference) = value.strip_prefix("keychain:") {
            let (service, account) = reference
                .split_once('/')
                .ok_or(GitHubAppError::InvalidKeyReference)?;
            if service.is_empty() || account.is_empty() {
                return Err(GitHubAppError::InvalidKeyReference);
            }
            return Ok(Self::Keychain {
                service: service.to_owned(),
                account: account.to_owned(),
            });
        }
        Err(GitHubAppError::InvalidKeyReference)
    }
}

impl fmt::Debug for KeyReference {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProtectedFile { path } => formatter
                .debug_struct("ProtectedFile")
                .field("path", path)
                .finish(),
            Self::Keychain { service, account } => formatter
                .debug_struct("Keychain")
                .field("service", service)
                .field("account", account)
                .finish(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GitHubAppConfiguration {
    pub app_id: u64,
    pub client_id: String,
    pub key_reference: KeyReference,
    pub api_base_url: String,
}

impl GitHubAppConfiguration {
    pub fn new(
        app_id: u64,
        client_id: impl Into<String>,
        key_reference: KeyReference,
        api_base_url: impl Into<String>,
    ) -> Result<Self, GitHubAppError> {
        let client_id = client_id.into();
        let api_base_url = api_base_url.into();
        if app_id == 0 || client_id.trim().is_empty() {
            return Err(GitHubAppError::InvalidConfiguration);
        }
        let parsed = Url::parse(&api_base_url).map_err(|_| GitHubAppError::InvalidConfiguration)?;
        if parsed.scheme() != "https" || parsed.host_str().is_none() {
            return Err(GitHubAppError::InvalidConfiguration);
        }
        match &key_reference {
            KeyReference::ProtectedFile { path } if !path.is_absolute() => {
                return Err(GitHubAppError::InvalidKeyReference);
            }
            KeyReference::Keychain { service, account }
                if service.trim().is_empty() || account.trim().is_empty() =>
            {
                return Err(GitHubAppError::InvalidKeyReference);
            }
            _ => {}
        }
        Ok(Self {
            app_id,
            client_id,
            key_reference,
            api_base_url: api_base_url.trim_end_matches('/').to_owned(),
        })
    }
}

pub struct SecretBytes(Vec<u8>);

impl SecretBytes {
    fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

impl Drop for SecretBytes {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

impl fmt::Debug for SecretBytes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretBytes([REDACTED])")
    }
}

pub trait PrivateKeyProvider: fmt::Debug {
    fn load(&self, reference: &KeyReference) -> Result<SecretBytes, GitHubAppError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ProtectedFileKeyProvider;

impl PrivateKeyProvider for ProtectedFileKeyProvider {
    fn load(&self, reference: &KeyReference) -> Result<SecretBytes, GitHubAppError> {
        let KeyReference::ProtectedFile { path } = reference else {
            return Err(GitHubAppError::UnsupportedKeyProvider);
        };
        validate_protected_key_file(path)?;
        let bytes = fs::read(path).map_err(|_| GitHubAppError::KeyUnavailable)?;
        Ok(SecretBytes(bytes))
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct KeychainKeyProvider;

impl PrivateKeyProvider for KeychainKeyProvider {
    fn load(&self, reference: &KeyReference) -> Result<SecretBytes, GitHubAppError> {
        let KeyReference::Keychain { service, account } = reference else {
            return Err(GitHubAppError::UnsupportedKeyProvider);
        };
        get_generic_password(service, account)
            .map(SecretBytes)
            .map_err(|_| GitHubAppError::KeyUnavailable)
    }
}

pub fn import_private_key_to_keychain(
    service: &str,
    account: &str,
    pem: &[u8],
) -> Result<KeyReference, GitHubAppError> {
    if service.trim().is_empty() || account.trim().is_empty() {
        return Err(GitHubAppError::InvalidKeyReference);
    }
    EncodingKey::from_rsa_pem(pem).map_err(|_| GitHubAppError::InvalidPrivateKey)?;
    set_generic_password(service, account, pem).map_err(|_| GitHubAppError::KeychainWrite)?;
    Ok(KeyReference::Keychain {
        service: service.to_owned(),
        account: account.to_owned(),
    })
}

fn validate_protected_key_file(path: &Path) -> Result<(), GitHubAppError> {
    let symlink = fs::symlink_metadata(path).map_err(|_| GitHubAppError::KeyUnavailable)?;
    if symlink.file_type().is_symlink() || !symlink.file_type().is_file() {
        return Err(GitHubAppError::InsecureKeyFile);
    }
    if symlink.permissions().mode() & 0o077 != 0 {
        return Err(GitHubAppError::InsecureKeyFile);
    }
    Ok(())
}

pub struct AppAuthenticator<P> {
    configuration: GitHubAppConfiguration,
    provider: P,
    encoding_key: EncodingKey,
}

impl<P: PrivateKeyProvider> AppAuthenticator<P> {
    pub fn new(configuration: GitHubAppConfiguration, provider: P) -> Result<Self, GitHubAppError> {
        let secret = provider.load(&configuration.key_reference)?;
        let encoding_key = EncodingKey::from_rsa_pem(secret.as_slice())
            .map_err(|_| GitHubAppError::InvalidPrivateKey)?;
        Ok(Self {
            configuration,
            provider,
            encoding_key,
        })
    }

    pub fn app_jwt(&self, now_epoch_seconds: i64) -> Result<SecretString, GitHubAppError> {
        let claims = AppClaims {
            iat: now_epoch_seconds - 60,
            exp: now_epoch_seconds + 540,
            iss: self.configuration.app_id.to_string(),
        };
        let token = encode(&Header::new(Algorithm::RS256), &claims, &self.encoding_key)
            .map_err(|_| GitHubAppError::JwtSigning)?;
        Ok(SecretString::new(token))
    }

    #[must_use]
    pub const fn configuration(&self) -> &GitHubAppConfiguration {
        &self.configuration
    }
}

impl<P: fmt::Debug> fmt::Debug for AppAuthenticator<P> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AppAuthenticator")
            .field("configuration", &self.configuration)
            .field("provider", &self.provider)
            .field("encoding_key", &"[REDACTED]")
            .finish()
    }
}

#[derive(Serialize)]
struct AppClaims {
    iat: i64,
    exp: i64,
    iss: String,
}

pub struct SecretString(Vec<u8>);

impl SecretString {
    #[must_use]
    pub fn new(value: impl AsRef<str>) -> Self {
        Self(value.as_ref().as_bytes().to_vec())
    }

    #[must_use]
    pub fn expose_for_authorization_header(&self) -> &str {
        std::str::from_utf8(&self.0).expect("JWT encoding is UTF-8")
    }
}

impl Drop for SecretString {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretString([REDACTED])")
    }
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum GitHubAppError {
    #[error("invalid GitHub App configuration")]
    InvalidConfiguration,
    #[error(
        "invalid GitHub App key reference; use file:/absolute/path or keychain:service/account"
    )]
    InvalidKeyReference,
    #[error("GitHub App private key is unavailable")]
    KeyUnavailable,
    #[error(
        "GitHub App private-key file must be a regular non-symlink file readable only by its owner"
    )]
    InsecureKeyFile,
    #[error("GitHub App key provider does not match the configured reference")]
    UnsupportedKeyProvider,
    #[error("GitHub App private key is not a valid unencrypted RSA PEM")]
    InvalidPrivateKey,
    #[error("GitHub App JWT signing failed")]
    JwtSigning,
    #[error("GitHub App private key could not be stored in Keychain")]
    KeychainWrite,
}
