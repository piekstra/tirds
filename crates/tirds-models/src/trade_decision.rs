use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const OUTPUT_SCHEMA_VERSION: u32 = 1;

/// The complete decision output for a TradeProposal.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TradeDecision {
    pub id: Uuid,
    pub schema_version: u32,
    /// ID of the TradeProposal this decision responds to.
    pub proposal_id: Uuid,
    pub symbol: String,
    pub decided_at: DateTime<Utc>,
    pub leg_assessments: Vec<LegAssessment>,
    pub overall_confidence: ConfidenceScore,
    pub information_relevance: InformationRelevance,
    pub confidence_decay: DecayProfile,
    pub price_target_decay: Option<DecayProfile>,
    pub trade_intelligence: TradeIntelligence,
    pub timeline: Vec<TimelinePoint>,
    pub agent_reports: Vec<AgentReport>,
    pub processing_time_ms: u64,
}

/// Assessment of a single trade leg.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LegAssessment {
    pub side: String,
    pub confidence: ConfidenceScore,
    pub price_assessment: PriceAssessment,
}

/// A confidence score with reasoning.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConfidenceScore {
    /// 0.0 to 1.0 representing probability of success.
    pub score: Decimal,
    pub reasoning: String,
}

/// Assessment of a price relative to market conditions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PriceAssessment {
    /// Positive = favorable, negative = unfavorable.
    pub favorability: Decimal,
    /// Suggested alternative price if current is suboptimal.
    pub suggested_price: Option<Decimal>,
    pub reasoning: String,
}

/// How relevant the cached data was to this specific trade.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InformationRelevance {
    /// 0.0 to 1.0 - how applicable the available data was.
    pub score: Decimal,
    pub source_contributions: Vec<SourceContribution>,
}

/// A single data source's contribution to the decision.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceContribution {
    pub source_name: String,
    pub relevance: Decimal,
    pub freshness_seconds: u64,
}

/// Decay profile for confidence or price targets over time.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DecayProfile {
    /// Per-day decay rate (e.g., 0.30 = 30% per day).
    pub daily_rate: Decimal,
    pub model: DecayModel,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DecayModel {
    /// Simple percentage reduction per period.
    Linear,
    /// Multiplied by (1 - rate) each day.
    Exponential,
}

/// Intelligence about trade "smartness", especially for one-sided trades.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TradeIntelligence {
    /// Overall smartness score 0.0 to 1.0.
    pub smartness_score: Decimal,
    /// Structured assessments (e.g., "sell price is 3% below market").
    pub assessments: Vec<String>,
}

/// A single point on the projected timeline.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TimelinePoint {
    /// Offset from proposal time in hours.
    pub offset_hours: u32,
    pub projected_confidence: Decimal,
    pub projected_price_target: Option<Decimal>,
    pub note: Option<String>,
}

/// Metadata from an individual specialist agent's contribution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentReport {
    pub agent_name: String,
    pub domain: String,
    pub confidence: Decimal,
    pub reasoning: String,
    pub data_sources_used: Vec<String>,
    pub elapsed_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn sample_decision() -> TradeDecision {
        TradeDecision {
            id: Uuid::new_v4(),
            schema_version: OUTPUT_SCHEMA_VERSION,
            proposal_id: Uuid::new_v4(),
            symbol: "AAPL".to_string(),
            decided_at: Utc::now(),
            leg_assessments: vec![
                LegAssessment {
                    side: "buy".to_string(),
                    confidence: ConfidenceScore {
                        score: dec!(0.85),
                        reasoning: "Price is 2% below SMA-20".to_string(),
                    },
                    price_assessment: PriceAssessment {
                        favorability: dec!(0.02),
                        suggested_price: None,
                        reasoning: "Buy price is favorable relative to recent support".to_string(),
                    },
                },
                LegAssessment {
                    side: "sell".to_string(),
                    confidence: ConfidenceScore {
                        score: dec!(0.70),
                        reasoning: "Target is near resistance but achievable intraday".to_string(),
                    },
                    price_assessment: PriceAssessment {
                        favorability: dec!(0.05),
                        suggested_price: Some(dec!(156.00)),
                        reasoning: "Could target higher based on ATR".to_string(),
                    },
                },
            ],
            overall_confidence: ConfidenceScore {
                score: dec!(0.80),
                reasoning: "Strong technical setup with supportive macro conditions".to_string(),
            },
            information_relevance: InformationRelevance {
                score: dec!(0.90),
                source_contributions: vec![
                    SourceContribution {
                        source_name: "technical_indicators".to_string(),
                        relevance: dec!(0.95),
                        freshness_seconds: 30,
                    },
                    SourceContribution {
                        source_name: "macro_data".to_string(),
                        relevance: dec!(0.70),
                        freshness_seconds: 3600,
                    },
                ],
            },
            confidence_decay: DecayProfile {
                daily_rate: dec!(0.30),
                model: DecayModel::Exponential,
            },
            price_target_decay: Some(DecayProfile {
                daily_rate: dec!(0.10),
                model: DecayModel::Linear,
            }),
            trade_intelligence: TradeIntelligence {
                smartness_score: dec!(0.82),
                assessments: vec![
                    "Buy price is 2% below current market - favorable entry".to_string(),
                    "Sell target aligns with intraday resistance levels".to_string(),
                ],
            },
            timeline: vec![
                TimelinePoint {
                    offset_hours: 1,
                    projected_confidence: dec!(0.80),
                    projected_price_target: Some(dec!(155.00)),
                    note: None,
                },
                TimelinePoint {
                    offset_hours: 4,
                    projected_confidence: dec!(0.75),
                    projected_price_target: Some(dec!(155.00)),
                    note: Some("End of trading day approaching".to_string()),
                },
                TimelinePoint {
                    offset_hours: 24,
                    projected_confidence: dec!(0.56),
                    projected_price_target: Some(dec!(154.45)),
                    note: Some("Overnight gap risk".to_string()),
                },
            ],
            agent_reports: vec![AgentReport {
                agent_name: "technical".to_string(),
                domain: "technical".to_string(),
                confidence: dec!(0.85),
                reasoning: "RSI-14 at 35, oversold. Price near SMA-20 support.".to_string(),
                data_sources_used: vec!["rsi_14".to_string(), "sma_20".to_string()],
                elapsed_ms: 2500,
            }],
            processing_time_ms: 5000,
        }
    }

    #[test]
    fn roundtrip_trade_decision() {
        let decision = sample_decision();
        let json = serde_json::to_string(&decision).unwrap();
        let deserialized: TradeDecision = serde_json::from_str(&json).unwrap();
        assert_eq!(decision, deserialized);
    }

    #[test]
    fn decay_model_serialization() {
        assert_eq!(
            serde_json::to_string(&DecayModel::Linear).unwrap(),
            "\"linear\""
        );
        assert_eq!(
            serde_json::to_string(&DecayModel::Exponential).unwrap(),
            "\"exponential\""
        );
    }

    #[test]
    fn confidence_score_bounds() {
        let valid = ConfidenceScore {
            score: dec!(0.50),
            reasoning: "test".to_string(),
        };
        let json = serde_json::to_string(&valid).unwrap();
        let parsed: ConfidenceScore = serde_json::from_str(&json).unwrap();
        assert_eq!(valid, parsed);
    }

    #[test]
    fn decision_with_no_price_target_decay() {
        let mut decision = sample_decision();
        decision.price_target_decay = None;
        let json = serde_json::to_string(&decision).unwrap();
        let deserialized: TradeDecision = serde_json::from_str(&json).unwrap();
        assert_eq!(decision.price_target_decay, deserialized.price_target_decay);
    }
}
