use clap::Parser;
use std::net::SocketAddr;

#[derive(Parser)]
#[command(name = "patchwright-relay", about = "Verified GitHub webhook relay")]
struct Arguments {
    #[arg(
        long,
        env = "PATCHWRIGHT_RELAY_ADDRESS",
        default_value = "127.0.0.1:8787"
    )]
    address: SocketAddr,
    #[arg(
        long,
        env = "PATCHWRIGHT_GITHUB_WEBHOOK_SECRET",
        hide_env_values = true
    )]
    webhook_secret: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_target(false)
        .init();
    let arguments = Arguments::parse();
    let listener = tokio::net::TcpListener::bind(arguments.address).await?;
    tracing::info!(address = %arguments.address, "relay listening on loopback");
    axum::serve(
        listener,
        patchwright_relay::router(patchwright_relay::RelayState::new(
            arguments.webhook_secret.into_bytes(),
        )),
    )
    .await?;
    Ok(())
}
