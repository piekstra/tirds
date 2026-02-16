use anyhow::{Context, Result};
use clap::Parser;
use tracing_subscriber::EnvFilter;

use tirds_loader::config::LoaderConfig;
use tirds_loader::daemon::Daemon;
use tirds_loader::writer::SqliteWriter;

#[derive(Parser, Debug)]
#[command(
    name = "tirds-loader",
    about = "TIRDS cache loader daemon - populates the shared SQLite cache from market data, calculations, and streaming sources"
)]
struct Cli {
    /// Path to loader configuration file
    #[arg(short, long, default_value = "config/tirds-loader.toml")]
    config: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    let config_str = std::fs::read_to_string(&cli.config)
        .with_context(|| format!("Failed to read config: {}", cli.config))?;
    let config: LoaderConfig =
        toml::from_str(&config_str).with_context(|| "Failed to parse loader config")?;

    let writer = SqliteWriter::open(&config.cache.sqlite_path)
        .with_context(|| format!("Failed to open cache DB: {}", config.cache.sqlite_path))?;

    let daemon = Daemon::new(config, writer);
    let cancel = daemon.cancel_token();

    // Handle shutdown signals
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        tracing::info!("Received shutdown signal");
        cancel.cancel();
    });

    daemon
        .run()
        .await
        .map_err(|e| anyhow::anyhow!("Daemon error: {e}"))?;

    Ok(())
}
