#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

mod app_auth;
mod github;
mod installation;
mod mutations;
mod webhook;

pub use app_auth::{
    AppAuthenticator, GitHubAppConfiguration, GitHubAppError, KeyReference, KeychainKeyProvider,
    PrivateKeyProvider, ProtectedFileKeyProvider, SecretBytes, SecretString,
    import_private_key_to_keychain,
};
pub use github::{GitHubClient, GitHubError};
pub use installation::{
    InstallationBroker, InstallationBrokerError, InstallationPermissions, InstallationToken,
};
pub use mutations::{GitHubMutationClient, MutationError, MutationResult};
pub use webhook::{RelayState, router, verify_signature};
