use std::io::Read;

use anyhow::{Context, Result};
use clap::Parser;
use tirds_models::config::TirdsConfig;
use tirds_models::trade_input::TradeProposal;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "tirds", about = "Trading Information Relevance Decider System")]
struct Cli {
    /// Path to configuration file
    #[arg(short, long, default_value = "config/tirds.toml")]
    config: String,

    /// Read TradeProposal JSON from a file instead of stdin
    #[arg(short, long)]
    input: Option<String>,

    /// Pretty-print the output JSON
    #[arg(long)]
    pretty: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing (respects RUST_LOG env var)
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    // Load config
    let config_str = std::fs::read_to_string(&cli.config)
        .with_context(|| format!("Failed to read config: {}", cli.config))?;
    let config: TirdsConfig =
        toml::from_str(&config_str).with_context(|| "Failed to parse config")?;

    // Read proposal
    let proposal_json = if let Some(input_path) = &cli.input {
        std::fs::read_to_string(input_path)
            .with_context(|| format!("Failed to read input: {input_path}"))?
    } else {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("Failed to read from stdin")?;
        buf
    };

    let proposal: TradeProposal =
        serde_json::from_str(&proposal_json).context("Failed to parse TradeProposal JSON")?;

    // Build orchestrator and evaluate
    let orchestrator =
        tirds::build_orchestrator(&config).context("Failed to build orchestrator")?;

    let decision = tirds::evaluate(&orchestrator, &proposal)
        .await
        .map_err(|e| anyhow::anyhow!("Evaluation failed: {e}"))?;

    // Output decision as JSON to stdout
    let output = if cli.pretty {
        serde_json::to_string_pretty(&decision)?
    } else {
        serde_json::to_string(&decision)?
    };
    println!("{output}");

    Ok(())
}
