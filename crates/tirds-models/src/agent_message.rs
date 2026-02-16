use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::trade_input::TradeProposal;

/// Request sent to a specialist agent (serialized as JSON to Claude CLI).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentRequest {
    pub request_id: Uuid,
    pub proposal: TradeProposal,
    /// Domain-specific data snapshot from cache, pre-fetched by orchestrator.
    pub domain_data: serde_json::Value,
    /// The agent's assigned domain (e.g., "technical", "macro", "sentiment").
    pub domain: String,
}

/// Response parsed from a specialist agent's Claude CLI stdout.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentResponse {
    pub request_id: Uuid,
    pub agent_name: String,
    pub domain: String,
    /// 0.0 to 1.0 confidence score from this specialist.
    pub confidence: Decimal,
    pub reasoning: String,
    /// Domain-specific structured analysis data.
    pub analysis: serde_json::Value,
    /// Which cache keys the agent considered.
    pub data_sources_consulted: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trade_input::{LegSide, TradeLeg, INPUT_SCHEMA_VERSION};
    use chrono::Utc;
    use rust_decimal_macros::dec;

    fn sample_proposal() -> TradeProposal {
        TradeProposal {
            id: Uuid::new_v4(),
            schema_version: INPUT_SCHEMA_VERSION,
            symbol: "TSLA".to_string(),
            legs: vec![TradeLeg {
                side: LegSide::Buy,
                price: Some(dec!(200.00)),
                quantity: Some(dec!(10)),
                time_in_force: Some("day".to_string()),
            }],
            proposed_at: Utc::now(),
            context: None,
        }
    }

    #[test]
    fn roundtrip_agent_request() {
        let request = AgentRequest {
            request_id: Uuid::new_v4(),
            proposal: sample_proposal(),
            domain_data: serde_json::json!({
                "rsi_14": 42.5,
                "sma_20": 205.30,
                "atr_14": 8.75
            }),
            domain: "technical".to_string(),
        };

        let json = serde_json::to_string(&request).unwrap();
        let deserialized: AgentRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(request, deserialized);
    }

    #[test]
    fn roundtrip_agent_response() {
        let response = AgentResponse {
            request_id: Uuid::new_v4(),
            agent_name: "technical".to_string(),
            domain: "technical".to_string(),
            confidence: dec!(0.75),
            reasoning: "RSI indicates oversold conditions".to_string(),
            analysis: serde_json::json!({
                "rsi_signal": "oversold",
                "price_vs_sma": "below",
                "trend": "bearish_reversal"
            }),
            data_sources_consulted: vec!["rsi_14_TSLA".to_string(), "sma_20_TSLA".to_string()],
        };

        let json = serde_json::to_string(&response).unwrap();
        let deserialized: AgentResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(response, deserialized);
    }

    #[test]
    fn agent_response_with_empty_analysis() {
        let response = AgentResponse {
            request_id: Uuid::new_v4(),
            agent_name: "sentiment".to_string(),
            domain: "sentiment".to_string(),
            confidence: dec!(0.50),
            reasoning: "No relevant sentiment data available".to_string(),
            analysis: serde_json::Value::Null,
            data_sources_consulted: vec![],
        };

        let json = serde_json::to_string(&response).unwrap();
        let deserialized: AgentResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(response, deserialized);
    }
}
