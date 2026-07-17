use clap::{Parser, Subcommand};
use serde::Deserialize;
use std::fs::OpenOptions;
use std::future::Future;
use std::io::Read;
use std::net::SocketAddr;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
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
            default_value = "127.0.0.1:8787",
            value_parser = parse_loopback_address
        )]
        address: SocketAddr,
        #[arg(long, env = "PATCHWRIGHT_GITHUB_WEBHOOK_SECRET_FILE")]
        webhook_secret_file: PathBuf,
        #[arg(long, env = "PATCHWRIGHT_RELAY_DATABASE")]
        database: PathBuf,
        #[arg(long, env = "PATCHWRIGHT_ENGINE_SOCKET")]
        engine_socket: PathBuf,
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
            engine_socket,
        } => serve_relay(address, webhook_secret_file, database, engine_socket).await?,
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

async fn serve_relay(
    address: SocketAddr,
    webhook_secret_file: PathBuf,
    database: PathBuf,
    engine_socket: PathBuf,
) -> anyhow::Result<()> {
    serve_relay_until(
        address,
        webhook_secret_file,
        database,
        engine_socket,
        shutdown_signal(),
    )
    .await
}

async fn serve_relay_until<F>(
    address: SocketAddr,
    webhook_secret_file: PathBuf,
    database: PathBuf,
    engine_socket: PathBuf,
    shutdown: F,
) -> anyhow::Result<()>
where
    F: Future<Output = ()> + Send,
{
    let secret = read_webhook_secret(&webhook_secret_file)?;
    let state = patchwright_relay::RelayState::open(secret, expand_home(database)?)?;
    let engine_socket = expand_home(engine_socket)?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    tracing::info!(address = %address, "relay listening on loopback");
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let mut services = tokio::task::JoinSet::new();
    let server_shutdown = shutdown_rx.clone();
    let server_state = state.clone();
    services.spawn(async move {
        axum::serve(listener, patchwright_relay::router(server_state))
            .with_graceful_shutdown(wait_for_shutdown(server_shutdown))
            .await
            .map_err(anyhow::Error::from)
    });
    let forwarder_shutdown = shutdown_rx;
    services.spawn(async move {
        state
            .run_forwarder_until(&engine_socket, wait_for_shutdown(forwarder_shutdown))
            .await
    });
    tokio::pin!(shutdown);
    let primary = tokio::select! {
        () = &mut shutdown => Ok(()),
        result = services.join_next() => joined_service(result),
    };
    let _ = shutdown_tx.send(true);
    let drained = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(result) = services.join_next().await {
            joined_service(Some(result))?;
        }
        Ok::<(), anyhow::Error>(())
    })
    .await;
    if drained.is_err() {
        services.abort_all();
        anyhow::bail!("relay services did not stop within five seconds");
    }
    primary?;
    drained.expect("checked timeout")?;
    Ok(())
}

async fn shutdown_signal() {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("install SIGTERM handler");
    tokio::select! {
        result = tokio::signal::ctrl_c() => {
            if let Err(error) = result {
                tracing::error!(error = %error, "wait for Ctrl-C");
            }
        }
        _ = terminate.recv() => {}
    }
}

async fn wait_for_shutdown(mut receiver: tokio::sync::watch::Receiver<bool>) {
    while !*receiver.borrow_and_update() {
        if receiver.changed().await.is_err() {
            break;
        }
    }
}

fn joined_service(
    result: Option<Result<anyhow::Result<()>, tokio::task::JoinError>>,
) -> anyhow::Result<()> {
    result
        .ok_or_else(|| anyhow::anyhow!("relay services stopped unexpectedly"))?
        .map_err(anyhow::Error::from)?
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

fn parse_loopback_address(value: &str) -> Result<SocketAddr, String> {
    let address: SocketAddr = value
        .parse()
        .map_err(|_| "relay address must be a socket address".to_owned())?;
    if !address.ip().is_loopback() {
        return Err("relay address must use an IPv4 or IPv6 loopback address".to_owned());
    }
    Ok(address)
}

fn read_webhook_secret(path: &Path) -> anyhow::Result<Vec<u8>> {
    const MAX_WEBHOOK_SECRET_BYTES: u64 = 4096;
    anyhow::ensure!(path.is_absolute(), "webhook secret path must be absolute");
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| anyhow::anyhow!("webhook secret must be an accessible regular file"))?;
    let metadata = file
        .metadata()
        .map_err(|_| anyhow::anyhow!("webhook secret metadata is unavailable"))?;
    let mode = metadata.permissions().mode() & 0o777;
    anyhow::ensure!(
        metadata.file_type().is_file()
            && metadata.uid() == u32::from(nix::unistd::geteuid())
            && matches!(mode, 0o400 | 0o600),
        "webhook secret must be an owner-only regular file"
    );
    anyhow::ensure!(
        (1..=MAX_WEBHOOK_SECRET_BYTES).contains(&metadata.len()),
        "webhook secret size is invalid"
    );
    let mut secret = Vec::with_capacity(usize::try_from(metadata.len())?);
    file.by_ref()
        .take(MAX_WEBHOOK_SECRET_BYTES + 1)
        .read_to_end(&mut secret)?;
    anyhow::ensure!(
        u64::try_from(secret.len())? == metadata.len()
            && u64::try_from(secret.len())? <= MAX_WEBHOOK_SECRET_BYTES,
        "webhook secret changed while being read"
    );
    Ok(secret)
}

#[allow(clippy::verbose_bit_mask)]
const fn is_owner_only(mode: u32) -> bool {
    mode & 0o077 == 0
}

#[cfg(test)]
mod tests {
    use super::{Arguments, expand_home, is_owner_only, read_webhook_secret, serve_relay_until};
    use clap::Parser;
    use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};
    use tempfile::TempDir;

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

    #[test]
    fn relay_serve_requires_an_explicit_engine_socket() {
        let result = Arguments::try_parse_from([
            "patchwright-relay",
            "serve",
            "--webhook-secret-file",
            "/tmp/webhook-secret",
            "--database",
            "/tmp/relay.sqlite",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn relay_serve_rejects_non_loopback_addresses() {
        let result = Arguments::try_parse_from([
            "patchwright-relay",
            "serve",
            "--address",
            "0.0.0.0:8787",
            "--webhook-secret-file",
            "/tmp/webhook-secret",
            "--database",
            "/tmp/relay.sqlite",
            "--engine-socket",
            "/tmp/engine.sock",
        ]);
        assert!(result.is_err());
        for address in ["127.0.0.1:8787", "[::1]:8787"] {
            let result = Arguments::try_parse_from([
                "patchwright-relay",
                "serve",
                "--address",
                address,
                "--webhook-secret-file",
                "/tmp/webhook-secret",
                "--database",
                "/tmp/relay.sqlite",
                "--engine-socket",
                "/tmp/engine.sock",
            ]);
            assert!(result.is_ok(), "loopback address {address}");
        }
    }

    #[test]
    fn webhook_secret_requires_an_absolute_owner_only_regular_file() {
        let temporary = TempDir::new().unwrap();
        fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let valid = temporary.path().join("secret");
        fs::write(&valid, b"secret-value").unwrap();
        fs::set_permissions(&valid, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(read_webhook_secret(&valid).unwrap(), b"secret-value");
        assert!(read_webhook_secret(PathBuf::from("relative-secret").as_path()).is_err());

        fs::set_permissions(&valid, fs::Permissions::from_mode(0o640)).unwrap();
        assert!(read_webhook_secret(&valid).is_err());
    }

    #[test]
    fn webhook_secret_rejects_symlinks_empty_and_oversized_files() {
        use std::os::unix::fs::symlink;

        let temporary = TempDir::new().unwrap();
        fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let target = temporary.path().join("target");
        fs::write(&target, b"secret-value").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
        let link = temporary.path().join("secret-link");
        symlink(&target, &link).unwrap();
        assert!(read_webhook_secret(&link).is_err());

        let empty = temporary.path().join("empty");
        fs::write(&empty, []).unwrap();
        fs::set_permissions(&empty, fs::Permissions::from_mode(0o400)).unwrap();
        assert!(read_webhook_secret(&empty).is_err());

        let oversized = temporary.path().join("oversized");
        fs::write(&oversized, vec![b'x'; 4097]).unwrap();
        fs::set_permissions(&oversized, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(read_webhook_secret(&oversized).is_err());
    }

    #[tokio::test]
    async fn relay_services_stop_with_the_shared_shutdown_boundary() {
        let temporary = TempDir::new().unwrap();
        fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let secret = temporary.path().join("secret");
        fs::write(&secret, b"secret-value").unwrap();
        fs::set_permissions(&secret, fs::Permissions::from_mode(0o600)).unwrap();

        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            serve_relay_until(
                "127.0.0.1:0".parse().unwrap(),
                secret,
                temporary.path().join("relay.sqlite"),
                temporary.path().join("engine.sock"),
                std::future::ready(()),
            ),
        )
        .await
        .expect("relay shutdown timed out")
        .unwrap();
    }
}
