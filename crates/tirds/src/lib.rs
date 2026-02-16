//! TIRDS - Trading Information Relevance Decider System
//!
//! An agentic trade decision system that evaluates proposed trades using
//! specialist Claude CLI agents and a shared cache of market data.
//!
//! # Library Usage
//!
//! ```rust,no_run
//! use tirds::models::{TradeProposal, TradeLeg, LegSide, TradeDecision};
//! use tirds::agents::{Orchestrator, ClaudeSpecialist, SpecialistAgent};
//! use tirds::cache::{CacheReader, SqliteReader};
//! use tirds::models::config::{TirdsConfig, AgentsConfig};
//! ```

pub use tirds_agents as agents;
pub use tirds_cache as cache;
pub use tirds_models as models;

use std::sync::Arc;
use std::time::Duration;

use tirds_agents::{ClaudeSpecialist, Orchestrator, SpecialistAgent};
use tirds_cache::{CacheReader, SqliteReader};
use tirds_models::config::TirdsConfig;
use tirds_models::trade_decision::TradeDecision;
use tirds_models::trade_input::TradeProposal;

/// Build an Orchestrator from configuration.
pub fn build_orchestrator(config: &TirdsConfig) -> Result<Orchestrator, anyhow::Error> {
    let sqlite = SqliteReader::open(&config.cache.sqlite_path)?;
    let cache = Arc::new(CacheReader::new(
        sqlite,
        config.cache.memory_max_capacity,
        Duration::from_secs(config.cache.memory_ttl_seconds),
    ));

    let specialists: Vec<Arc<dyn SpecialistAgent>> = config
        .agents
        .specialists
        .iter()
        .filter(|s| s.enabled)
        .map(|s| {
            let model = s
                .model
                .clone()
                .unwrap_or_else(|| config.agents.specialist_model.clone());
            let timeout = Duration::from_secs(config.agents.specialist_timeout_seconds);
            Arc::new(ClaudeSpecialist::new(
                s.name.clone(),
                s.domain.clone(),
                model,
                timeout,
            )) as Arc<dyn SpecialistAgent>
        })
        .collect();

    Ok(Orchestrator::new(specialists, cache, config.agents.clone()))
}

/// Evaluate a trade proposal using the given orchestrator.
pub async fn evaluate(
    orchestrator: &Orchestrator,
    proposal: &TradeProposal,
) -> Result<TradeDecision, tirds_agents::AgentError> {
    orchestrator.evaluate(proposal).await
}
