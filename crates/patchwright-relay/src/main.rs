use clap::{Parser, Subcommand};
use serde::Deserialize;
use std::net::SocketAddr;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "patchwright-relay", about = "Verified GitHub App relay")]
struct Arguments {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Serve {
        #[arg(
            long,
            env = "PATCHWRIGHT_RELAY_ADDRESS",
            default_value = "127.0.0.1:8787"
        )]
        address: SocketAddr,
        #[arg(long, env = "PATCHWRIGHT_GITHUB_WEBHOOK_SECRET_FILE")]
        webhook_secret_file: PathBuf,
        #[arg(long, env = "PATCHWRIGHT_RELAY_DATABASE")]
        database: PathBuf,
    },
    ImportGithubAppKey {
        #[arg(long)]
        path: PathBuf,
        #[arg(long, default_value = "ai.patchwright.github-app.private-key")]
        service: String,
        #[arg(long)]
        account: String,
    },
    GithubAppHealth {
        #[arg(long, default_value = "~/.patchwright/github-app.json")]
        config: PathBuf,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeConfiguration {
    app_id: u64,
    client_id: String,
    key_reference: String,
    api_base_url: String,
}

#[derive(Deserialize, serde::Serialize)]
struct AppIdentity {
    id: u64,
    slug: String,
    name: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_target(false)
        .init();
    match Arguments::parse().command {
        Command::Serve {
            address,
            webhook_secret_file,
            database,
        } => {
            let secret = std::fs::read(webhook_secret_file)?;
            let state = patchwright_relay::RelayState::open(secret, expand_home(database)?)?;
            let listener = tokio::net::TcpListener::bind(address).await?;
            tracing::info!(address = %address, "relay listening on loopback");
            axum::serve(listener, patchwright_relay::router(state)).await?;
        }
        Command::ImportGithubAppKey {
            path,
            service,
            account,
        } => {
            let pem = std::fs::read(path)?;
            let reference =
                patchwright_relay::import_private_key_to_keychain(&service, &account, &pem)?;
            println!("keychain reference stored: {reference:?}");
        }
        Command::GithubAppHealth { config } => {
            let config = expand_home(config)?;
            let metadata = std::fs::symlink_metadata(&config)?;
            anyhow::ensure!(
                metadata.file_type().is_file()
                    && !metadata.file_type().is_symlink()
                    && is_owner_only(metadata.permissions().mode()),
                "GitHub App metadata must be an owner-only regular file"
            );
            let runtime: RuntimeConfiguration = serde_json::from_slice(&std::fs::read(config)?)?;
            let key_reference = patchwright_relay::KeyReference::parse(&runtime.key_reference)?;
            let configuration = patchwright_relay::GitHubAppConfiguration::new(
                runtime.app_id,
                runtime.client_id,
                key_reference,
                runtime.api_base_url,
            )?;
            let api_base_url = configuration.api_base_url.clone();
            let authenticator = patchwright_relay::AppAuthenticator::new(
                configuration,
                patchwright_relay::ConfiguredKeyProvider,
            )?;
            let jwt = authenticator.app_jwt(chrono::Utc::now().timestamp())?;
            let identity: AppIdentity = reqwest::Client::builder()
                .user_agent("Patchwright/0.1")
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(15))
                .build()?
                .get(format!("{api_base_url}/app"))
                .bearer_auth(jwt.expose_for_authorization_header())
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28")
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            anyhow::ensure!(
                identity.id == authenticator.configuration().app_id,
                "GitHub returned a different App identity"
            );
            println!("{}", serde_json::to_string(&identity)?);
        }
    }
    Ok(())
}

fn expand_home(path: PathBuf) -> anyhow::Result<PathBuf> {
    let value = path.to_string_lossy();
    if value == "~" || value.starts_with("~/") {
        let home =
            std::env::var_os("HOME").ok_or_else(|| anyhow::anyhow!("HOME is unavailable"))?;
        return Ok(PathBuf::from(home).join(value.trim_start_matches("~/")));
    }
    Ok(path)
}

#[allow(clippy::verbose_bit_mask)]
const fn is_owner_only(mode: u32) -> bool {
    mode & 0o077 == 0
}

#[cfg(test)]
mod tests {
    use super::{Arguments, expand_home, is_owner_only};
    use clap::Parser;
    use std::path::PathBuf;

    #[test]
    fn explicit_path_is_unchanged() {
        let path = PathBuf::from("/tmp/patchwright-github-app.json");
        assert_eq!(expand_home(path.clone()).expect("path"), path);
    }

    #[test]
    fn tilde_path_uses_current_home() {
        let home = std::env::var_os("HOME").expect("HOME");
        assert_eq!(
            expand_home(PathBuf::from("~/.patchwright/github-app.json")).expect("path"),
            PathBuf::from(home).join(".patchwright/github-app.json")
        );
    }

    #[test]
    fn owner_only_mode_rejects_group_and_other_access() {
        assert!(is_owner_only(0o600));
        assert!(is_owner_only(0o400));
        assert!(!is_owner_only(0o640));
        assert!(!is_owner_only(0o604));
    }

    #[test]
    fn relay_serve_requires_an_explicit_durable_database_path() {
        let result = Arguments::try_parse_from([
            "patchwright-relay",
            "serve",
            "--webhook-secret-file",
            "/tmp/webhook-secret",
        ]);
        assert!(result.is_err());
    }
}
