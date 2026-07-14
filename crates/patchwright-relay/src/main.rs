use clap::{Parser, Subcommand};
use std::net::SocketAddr;
use std::path::PathBuf;

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
    },
    ImportGithubAppKey {
        #[arg(long)]
        path: PathBuf,
        #[arg(long, default_value = "ai.patchwright.github-app.private-key")]
        service: String,
        #[arg(long)]
        account: String,
    },
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
        } => {
            let secret = std::fs::read(webhook_secret_file)?;
            let listener = tokio::net::TcpListener::bind(address).await?;
            tracing::info!(address = %address, "relay listening on loopback");
            axum::serve(
                listener,
                patchwright_relay::router(patchwright_relay::RelayState::new(secret)),
            )
            .await?;
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
    }
    Ok(())
}
