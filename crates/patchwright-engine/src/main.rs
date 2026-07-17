use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "patchwright-engine",
    about = "Local Patchwright execution engine"
)]
struct Arguments {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Serve {
        #[arg(long, env = "PATCHWRIGHT_SOCKET")]
        socket: PathBuf,
        #[arg(long, env = "PATCHWRIGHT_DATABASE")]
        database: PathBuf,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_target(false)
        .init();
    match Arguments::parse().command {
        Command::Serve { socket, database } => {
            patchwright_engine::serve_until(&socket, &database, shutdown_signal()).await
        }
    }
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
