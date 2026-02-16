use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const INPUT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LegSide {
    Buy,
    Sell,
}

/// A single leg of a trade (buy or sell).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TradeLeg {
    pub side: LegSide,
    /// Target price for this leg. None = market order.
    pub price: Option<Decimal>,
    /// Quantity (shares). None = "evaluate at any quantity".
    pub quantity: Option<Decimal>,
    /// Time-in-force hint (e.g., "day", "gtc").
    pub time_in_force: Option<String>,
}

/// Optional context the caller can provide alongside the proposal.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TradeContext {
    /// The algorithm or rule that generated this proposal.
    pub source_rule_id: Option<String>,
    /// Current market price at proposal time.
    pub current_market_price: Option<Decimal>,
    /// Any additional key-value metadata.
    pub metadata: Option<serde_json::Value>,
}

/// A proposed trade submitted for relevance analysis.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TradeProposal {
    pub id: Uuid,
    pub schema_version: u32,
    pub symbol: String,
    /// Individual legs of the trade. One (buy-only/sell-only) or two (buy+sell).
    pub legs: Vec<TradeLeg>,
    pub proposed_at: DateTime<Utc>,
    pub context: Option<TradeContext>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn roundtrip_trade_proposal_full() {
        let proposal = TradeProposal {
            id: Uuid::new_v4(),
            schema_version: INPUT_SCHEMA_VERSION,
            symbol: "AAPL".to_string(),
            legs: vec![
                TradeLeg {
                    side: LegSide::Buy,
                    price: Some(dec!(150.25)),
                    quantity: Some(dec!(100)),
                    time_in_force: Some("day".to_string()),
                },
                TradeLeg {
                    side: LegSide::Sell,
                    price: Some(dec!(155.00)),
                    quantity: Some(dec!(100)),
                    time_in_force: Some("day".to_string()),
                },
            ],
            proposed_at: Utc::now(),
            context: Some(TradeContext {
                source_rule_id: Some("scalp_rule_1".to_string()),
                current_market_price: Some(dec!(151.00)),
                metadata: Some(serde_json::json!({"strategy": "dip_buy"})),
            }),
        };

        let json = serde_json::to_string(&proposal).unwrap();
        let deserialized: TradeProposal = serde_json::from_str(&json).unwrap();
        assert_eq!(proposal, deserialized);
    }

    #[test]
    fn roundtrip_trade_proposal_minimal() {
        let proposal = TradeProposal {
            id: Uuid::new_v4(),
            schema_version: INPUT_SCHEMA_VERSION,
            symbol: "SPY".to_string(),
            legs: vec![TradeLeg {
                side: LegSide::Sell,
                price: None,
                quantity: None,
                time_in_force: None,
            }],
            proposed_at: Utc::now(),
            context: None,
        };

        let json = serde_json::to_string(&proposal).unwrap();
        let deserialized: TradeProposal = serde_json::from_str(&json).unwrap();
        assert_eq!(proposal, deserialized);
    }

    #[test]
    fn leg_side_serialization() {
        assert_eq!(serde_json::to_string(&LegSide::Buy).unwrap(), "\"buy\"");
        assert_eq!(serde_json::to_string(&LegSide::Sell).unwrap(), "\"sell\"");
    }
}
