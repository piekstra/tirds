//! Test support module providing scenario-based mock specialists.
//!
//! Unlike the simple `MockSpecialist` (which returns canned responses),
//! `ScenarioMockSpecialist` reads `domain_data` and applies the same
//! interpretation rules documented in the specialist prompts.

use async_trait::async_trait;
use rust_decimal::Decimal;
use tirds_models::agent_message::{AgentRequest, AgentResponse};

use crate::error::AgentError;
use crate::specialist::SpecialistAgent;

/// A mock specialist that reads domain_data and applies prompt-matching rules
/// to produce realistic confidence scores and reasoning.
pub struct ScenarioMockSpecialist {
    pub name: String,
    pub domain: String,
}

impl ScenarioMockSpecialist {
    pub fn new(name: &str, domain: &str) -> Self {
        Self {
            name: name.to_string(),
            domain: domain.to_string(),
        }
    }

    pub fn technical() -> Self {
        Self::new("technical_analyst", "technical")
    }

    pub fn macro_analyst() -> Self {
        Self::new("macro_analyst", "macro")
    }

    pub fn sentiment() -> Self {
        Self::new("sentiment_analyst", "sentiment")
    }

    pub fn sector() -> Self {
        Self::new("sector_analyst", "sector")
    }
}

/// Helper to get a nested last value: data[outer_key][inner_key].last()
fn last_nested_value(
    domain_data: &serde_json::Value,
    outer_key: &str,
    inner_key: &str,
) -> Option<f64> {
    domain_data
        .get(outer_key)
        .and_then(|obj| obj.get(inner_key))
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.last())
        .and_then(|v| v.as_f64())
}

/// Count consecutive direction from the end of a price array.
/// Returns positive for consecutive higher closes, negative for lower.
fn consecutive_trend(values: &[f64]) -> i32 {
    if values.len() < 2 {
        return 0;
    }
    let mut count = 0i32;
    let mut direction: Option<bool> = None; // true = up, false = down
    for i in (1..values.len()).rev() {
        let up = values[i] > values[i - 1];
        let down = values[i] < values[i - 1];
        match direction {
            None => {
                if up {
                    direction = Some(true);
                    count = 1;
                } else if down {
                    direction = Some(false);
                    count = -1;
                } else {
                    break;
                }
            }
            Some(true) => {
                if up {
                    count += 1;
                } else {
                    break;
                }
            }
            Some(false) => {
                if down {
                    count -= 1;
                } else {
                    break;
                }
            }
        }
    }
    count
}

/// Get close prices from a bars array.
fn extract_closes(domain_data: &serde_json::Value, bars_key: &str) -> Vec<f64> {
    domain_data
        .get(bars_key)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|bar| bar.get("close").and_then(|c| c.as_f64()))
                .collect()
        })
        .unwrap_or_default()
}

fn evaluate_technical(request: &AgentRequest) -> AgentResponse {
    let data = &request.domain_data;
    let symbol = &request.proposal.symbol;
    let mut confidence = 0.50f64;
    let mut reasoning_parts: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut sources: Vec<String> = Vec::new();

    // RSI
    let rsi_key = format!("indicator:rsi_14:{symbol}");
    if let Some(rsi) = last_nested_value(data, &rsi_key, "value") {
        sources.push(rsi_key);
        if rsi < 20.0 {
            confidence += 0.25;
            reasoning_parts.push(format!("RSI {rsi:.0} (extremely oversold, +0.25)"));
        } else if rsi < 30.0 {
            confidence += 0.15;
            reasoning_parts.push(format!("RSI {rsi:.0} (oversold, +0.15)"));
        } else if rsi > 80.0 {
            confidence -= 0.25;
            reasoning_parts.push(format!("RSI {rsi:.0} (extremely overbought, -0.25)"));
            warnings.push("Extremely overbought - high reversal risk".to_string());
        } else if rsi > 70.0 {
            confidence -= 0.15;
            reasoning_parts.push(format!("RSI {rsi:.0} (overbought, -0.15)"));
            if rsi > 75.0 {
                warnings.push("Extremely overbought - high reversal risk".to_string());
            }
        } else {
            reasoning_parts.push(format!("RSI {rsi:.0} (neutral)"));
        }
    }

    // MA Crossover: EMA vs SMA
    let sma_key = format!("indicator:sma_20:{symbol}");
    let ema_key = format!("indicator:ema_20:{symbol}");
    let sma = last_nested_value(data, &sma_key, "value");
    let ema = last_nested_value(data, &ema_key, "value");
    if let (Some(ema_val), Some(sma_val)) = (ema, sma) {
        sources.push(sma_key);
        sources.push(ema_key);
        if ema_val > sma_val {
            confidence += 0.10;
            reasoning_parts.push("EMA > SMA (golden cross, +0.10)".to_string());
        } else if ema_val < sma_val {
            confidence -= 0.10;
            reasoning_parts.push("EMA < SMA (death cross, -0.10)".to_string());
        }
    }

    // MACD
    let macd_key = format!("indicator:macd:{symbol}");
    let macd_line = last_nested_value(data, &macd_key, "macd_line");
    let signal_line = last_nested_value(data, &macd_key, "signal_line");
    if let (Some(macd), Some(signal)) = (macd_line, signal_line) {
        sources.push(macd_key);
        if macd > signal {
            confidence += 0.08;
            reasoning_parts.push("MACD > signal (bullish, +0.08)".to_string());
        } else {
            confidence -= 0.08;
            reasoning_parts.push("MACD < signal (bearish, -0.08)".to_string());
        }
    }

    // Bollinger Bands
    let bb_key = format!("indicator:bollinger_bands:{symbol}");
    let percent_b = last_nested_value(data, &bb_key, "percent_b");
    if let Some(pb) = percent_b {
        sources.push(bb_key);
        if pb > 1.0 {
            confidence -= 0.15;
            reasoning_parts.push(format!("%B {pb:.2} (above upper band, -0.15)"));
            warnings.push("Price extended beyond normal range - reversal risk high".to_string());
        } else if pb < 0.0 {
            confidence += 0.15;
            reasoning_parts.push(format!("%B {pb:.2} (below lower band, +0.15)"));
        }
    }

    // Trend from bars
    let bars_key = format!("bars:{symbol}:5m");
    let closes = extract_closes(data, &bars_key);
    if !closes.is_empty() {
        sources.push(bars_key);
        let trend = consecutive_trend(&closes);
        if trend >= 3 {
            confidence += 0.10;
            reasoning_parts.push(format!("{trend} consecutive higher closes (+0.10)"));
        } else if trend <= -3 {
            confidence -= 0.10;
            reasoning_parts.push(format!("{} consecutive lower closes (-0.10)", trend.abs()));
            if trend <= -4 {
                warnings.push("Sustained downtrend - don't enter yet".to_string());
            }
        }

        // Check death cross + downtrend combo warning
        if ema < sma && trend <= -3 {
            warnings.push("Death cross with active downtrend - avoid new long entries".to_string());
        }
    }

    confidence = confidence.clamp(0.0, 1.0);
    let confidence_dec = Decimal::from_f64_retain(confidence).unwrap_or(Decimal::new(50, 2));

    let reasoning = format!(
        "Base 0.50. {}. Final: {confidence:.2}.",
        reasoning_parts.join(". ")
    );

    AgentResponse {
        request_id: request.request_id,
        agent_name: "technical_analyst".to_string(),
        domain: "technical".to_string(),
        confidence: confidence_dec,
        reasoning,
        analysis: serde_json::json!({
            "warnings": warnings,
        }),
        data_sources_consulted: sources,
    }
}

fn evaluate_macro(request: &AgentRequest) -> AgentResponse {
    let data = &request.domain_data;
    let mut confidence = 0.50f64;
    let mut reasoning_parts: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut sources: Vec<String> = Vec::new();

    // VIX
    if let Some(vix) = last_nested_value(data, "ref:VIX", "value") {
        sources.push("ref:VIX".to_string());
        if vix < 15.0 {
            confidence += 0.05;
            reasoning_parts.push(format!("VIX {vix:.1} (low fear, +0.05)"));
        } else if vix > 35.0 {
            confidence -= 0.20;
            reasoning_parts.push(format!("VIX {vix:.1} (extreme fear, -0.20)"));
            warnings
                .push("Extreme market volatility - exercise caution on all positions".to_string());
        } else if vix > 25.0 {
            confidence -= 0.10;
            reasoning_parts.push(format!("VIX {vix:.1} (elevated fear, -0.10)"));
        } else {
            reasoning_parts.push(format!("VIX {vix:.1} (normal)"));
        }
    }

    // SPY Trend
    let spy_closes = extract_closes(data, "bars:SPY:1d");
    let spy_trend = consecutive_trend(&spy_closes);
    if !spy_closes.is_empty() {
        sources.push("bars:SPY:1d".to_string());
        if spy_trend >= 3 {
            confidence += 0.10;
            reasoning_parts.push(format!("SPY {spy_trend} consecutive up (+0.10)"));
        } else if spy_trend <= -3 {
            confidence -= 0.10;
            reasoning_parts.push(format!("SPY {} consecutive down (-0.10)", spy_trend.abs()));
        }
    }

    // Combined: VIX + SPY
    let vix_val = last_nested_value(data, "ref:VIX", "value");
    if let Some(vix) = vix_val {
        if vix < 15.0 && spy_trend >= 3 {
            confidence += 0.05;
            reasoning_parts.push("Low VIX + SPY uptrend combo (+0.05)".to_string());
        }
        if vix > 30.0 && spy_trend <= -3 {
            confidence -= 0.05;
            reasoning_parts.push("High VIX + SPY downtrend combo (-0.05)".to_string());
            warnings.push("High-volatility market downtrend - avoid new positions".to_string());
        }
    }

    confidence = confidence.clamp(0.0, 1.0);
    let confidence_dec = Decimal::from_f64_retain(confidence).unwrap_or(Decimal::new(50, 2));

    let reasoning = format!(
        "Base 0.50. {}. Final: {confidence:.2}.",
        reasoning_parts.join(". ")
    );

    AgentResponse {
        request_id: request.request_id,
        agent_name: "macro_analyst".to_string(),
        domain: "macro".to_string(),
        confidence: confidence_dec,
        reasoning,
        analysis: serde_json::json!({
            "warnings": warnings,
        }),
        data_sources_consulted: sources,
    }
}

fn evaluate_sentiment(request: &AgentRequest) -> AgentResponse {
    let data = &request.domain_data;
    let symbol = &request.proposal.symbol;
    let mut confidence = 0.50f64;
    let mut reasoning_parts: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut sources: Vec<String> = Vec::new();

    // News sentiment
    let news_key = format!("sentiment:news:{symbol}");
    let news_score = data
        .get(&news_key)
        .and_then(|v| v.get("score"))
        .and_then(|v| v.as_f64());
    if let Some(score) = news_score {
        sources.push(news_key);
        let adj = sentiment_adjustment(score) * 1.0; // weight 1.0x
        confidence += adj;
        reasoning_parts.push(format!("News sentiment {score:.2} (adj {adj:+.2})"));
    }

    // Social sentiment
    let social_key = format!("sentiment:social:{symbol}");
    let social_score = data
        .get(&social_key)
        .and_then(|v| v.get("score"))
        .and_then(|v| v.as_f64());
    if let Some(score) = social_score {
        sources.push(social_key);
        let adj = sentiment_adjustment(score) * 0.6; // weight 0.6x
        confidence += adj;
        reasoning_parts.push(format!(
            "Social sentiment {score:.2} (adj {adj:+.2}, 0.6x weight)"
        ));
    }

    // Analyst consensus
    let analyst_key = format!("sentiment:analyst:{symbol}");
    let analyst_consensus = data
        .get(&analyst_key)
        .and_then(|v| v.get("consensus"))
        .and_then(|v| v.as_f64());
    if let Some(consensus) = analyst_consensus {
        sources.push(analyst_key);
        // Map consensus 0-1 to sentiment score -1 to +1
        let mapped = (consensus - 0.5) * 2.0;
        let adj = sentiment_adjustment(mapped) * 0.8; // weight 0.8x
        confidence += adj;
        reasoning_parts.push(format!(
            "Analyst consensus {consensus:.2} (adj {adj:+.2}, 0.8x weight)"
        ));
    }

    // All sources negative warning
    let all_negative = [news_score, social_score]
        .iter()
        .flatten()
        .all(|s| *s < -0.5);
    if all_negative && news_score.is_some() {
        warnings.push("Uniformly negative sentiment across sources".to_string());
    }

    confidence = confidence.clamp(0.0, 1.0);
    let confidence_dec = Decimal::from_f64_retain(confidence).unwrap_or(Decimal::new(50, 2));

    let reasoning = format!(
        "Base 0.50. {}. Final: {confidence:.2}.",
        reasoning_parts.join(". ")
    );

    AgentResponse {
        request_id: request.request_id,
        agent_name: "sentiment_analyst".to_string(),
        domain: "sentiment".to_string(),
        confidence: confidence_dec,
        reasoning,
        analysis: serde_json::json!({
            "warnings": warnings,
        }),
        data_sources_consulted: sources,
    }
}

fn sentiment_adjustment(score: f64) -> f64 {
    if score > 0.5 {
        0.10
    } else if score > 0.2 {
        0.05
    } else if score < -0.5 {
        -0.10
    } else if score < -0.2 {
        -0.05
    } else {
        0.0
    }
}

fn evaluate_sector(request: &AgentRequest) -> AgentResponse {
    let data = &request.domain_data;
    let mut confidence = 0.50f64;
    let mut reasoning_parts: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut sources: Vec<String> = Vec::new();

    // Default to XLK (tech) since most test scenarios are tech stocks
    let sector_etf = "XLK";
    let sector_key = format!("ref:{sector_etf}");
    let spy_key = "ref:SPY";

    let sector_values: Vec<f64> = data
        .get(&sector_key)
        .and_then(|v| v.get("value"))
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_f64()).collect())
        .unwrap_or_default();

    let spy_values: Vec<f64> = data
        .get(spy_key)
        .and_then(|v| v.get("value"))
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_f64()).collect())
        .unwrap_or_default();

    if sector_values.len() >= 2 && spy_values.len() >= 2 {
        sources.push(sector_key);
        sources.push(spy_key.to_string());

        let sector_return = (sector_values.last().unwrap() - sector_values.first().unwrap())
            / sector_values.first().unwrap()
            * 100.0;
        let spy_return = (spy_values.last().unwrap() - spy_values.first().unwrap())
            / spy_values.first().unwrap()
            * 100.0;
        let relative = sector_return - spy_return;

        if relative > 3.0 {
            confidence += 0.12;
            reasoning_parts.push(format!(
                "Sector outperforming SPY by {relative:.1}% (+0.12)"
            ));
        } else if relative > 1.0 {
            confidence += 0.06;
            reasoning_parts.push(format!(
                "Sector outperforming SPY by {relative:.1}% (+0.06)"
            ));
        } else if relative < -3.0 {
            confidence -= 0.12;
            reasoning_parts.push(format!(
                "Sector underperforming SPY by {:.1}% (-0.12)",
                relative.abs()
            ));
            if relative < -5.0 {
                warnings.push("Sector significantly underperforming market".to_string());
            }
        } else if relative < -1.0 {
            confidence -= 0.06;
            reasoning_parts.push(format!(
                "Sector underperforming SPY by {:.1}% (-0.06)",
                relative.abs()
            ));
        } else {
            reasoning_parts.push(format!("Sector inline with SPY ({relative:+.1}%)"));
        }
    }

    // Sector trend from bars
    let bars_key = format!("bars:{sector_etf}:1d");
    let sector_closes = extract_closes(data, &bars_key);
    if !sector_closes.is_empty() {
        sources.push(bars_key);
        let trend = consecutive_trend(&sector_closes);
        if trend >= 3 {
            confidence += 0.08;
            reasoning_parts.push(format!("Sector {trend} consecutive up (+0.08)"));
        } else if trend <= -3 {
            confidence -= 0.08;
            reasoning_parts.push(format!("Sector {} consecutive down (-0.08)", trend.abs()));
        }
    }

    confidence = confidence.clamp(0.0, 1.0);
    let confidence_dec = Decimal::from_f64_retain(confidence).unwrap_or(Decimal::new(50, 2));

    let reasoning = format!(
        "Base 0.50. {}. Final: {confidence:.2}.",
        reasoning_parts.join(". ")
    );

    AgentResponse {
        request_id: request.request_id,
        agent_name: "sector_analyst".to_string(),
        domain: "sector".to_string(),
        confidence: confidence_dec,
        reasoning,
        analysis: serde_json::json!({
            "warnings": warnings,
        }),
        data_sources_consulted: sources,
    }
}

#[async_trait]
impl SpecialistAgent for ScenarioMockSpecialist {
    fn name(&self) -> &str {
        &self.name
    }

    fn domain(&self) -> &str {
        &self.domain
    }

    async fn evaluate(&self, request: &AgentRequest) -> Result<AgentResponse, AgentError> {
        let response = match self.domain.as_str() {
            "technical" => evaluate_technical(request),
            "macro" => evaluate_macro(request),
            "sentiment" => evaluate_sentiment(request),
            "sector" => evaluate_sector(request),
            _ => {
                return Err(AgentError::Cli(format!("Unknown domain: {}", self.domain)));
            }
        };
        Ok(response)
    }
}

/// Build a synthesized JSON value from specialist responses,
/// suitable for passing to `build_trade_decision()`.
pub fn build_synthesized_json(
    proposal: &tirds_models::trade_input::TradeProposal,
    responses: &[AgentResponse],
) -> serde_json::Value {
    // Weighted average: technical 0.35, macro 0.20, sentiment 0.20, sector 0.25
    let weights: std::collections::HashMap<&str, f64> = [
        ("technical", 0.35),
        ("macro", 0.20),
        ("sentiment", 0.20),
        ("sector", 0.25),
    ]
    .into();

    let mut weighted_sum = 0.0f64;
    let mut weight_total = 0.0f64;
    let mut all_warnings: Vec<String> = Vec::new();
    let mut all_reasoning: Vec<String> = Vec::new();

    for resp in responses {
        let w = weights.get(resp.domain.as_str()).copied().unwrap_or(0.25);
        let conf: f64 = resp.confidence.to_string().parse().unwrap_or(0.5);
        weighted_sum += conf * w;
        weight_total += w;

        all_reasoning.push(format!("[{}] {}", resp.domain, resp.reasoning));

        if let Some(warns) = resp.analysis.get("warnings").and_then(|v| v.as_array()) {
            for w_val in warns {
                if let Some(s) = w_val.as_str() {
                    if !s.is_empty() {
                        all_warnings.push(s.to_string());
                    }
                }
            }
        }
    }

    let overall = if weight_total > 0.0 {
        (weighted_sum / weight_total).clamp(0.0, 1.0)
    } else {
        0.50
    };

    let mut assessments = all_warnings.clone();
    if assessments.is_empty() {
        assessments.push("Trade appears reasonable".to_string());
    }

    // Build leg assessments
    let leg_assessments: Vec<serde_json::Value> = proposal
        .legs
        .iter()
        .map(|leg| {
            let side = serde_json::to_value(&leg.side).unwrap_or(serde_json::json!("buy"));
            let side_str = side.as_str().unwrap_or("buy");
            serde_json::json!({
                "side": side_str,
                "confidence": {
                    "score": format!("{overall:.2}"),
                    "reasoning": all_reasoning.join("; "),
                },
                "price_assessment": {
                    "favorability": "0.00",
                    "suggested_price": null,
                    "reasoning": "Mock assessment",
                }
            })
        })
        .collect();

    serde_json::json!({
        "overall_confidence": {
            "score": format!("{overall:.2}"),
            "reasoning": all_reasoning.join("; "),
        },
        "leg_assessments": leg_assessments,
        "information_relevance": {
            "score": "0.85",
            "source_contributions": [
                {"source_name": "technical", "relevance": "0.90", "freshness_seconds": 30},
                {"source_name": "macro", "relevance": "0.80", "freshness_seconds": 300},
                {"source_name": "sentiment", "relevance": "0.75", "freshness_seconds": 600},
                {"source_name": "sector", "relevance": "0.80", "freshness_seconds": 300},
            ]
        },
        "confidence_decay": {"daily_rate": "0.25", "model": "exponential"},
        "price_target_decay": null,
        "trade_intelligence": {
            "smartness_score": format!("{overall:.2}"),
            "assessments": assessments,
        },
        "timeline": [
            {"offset_hours": 1, "projected_confidence": format!("{overall:.2}"), "projected_price_target": null, "note": null},
            {"offset_hours": 24, "projected_confidence": format!("{:.2}", overall * 0.75), "projected_price_target": null, "note": "Overnight decay"},
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use tirds_models::trade_input::{LegSide, TradeLeg, TradeProposal, INPUT_SCHEMA_VERSION};
    use uuid::Uuid;

    fn test_proposal() -> TradeProposal {
        TradeProposal {
            id: Uuid::new_v4(),
            schema_version: INPUT_SCHEMA_VERSION,
            symbol: "AAPL".to_string(),
            legs: vec![TradeLeg {
                side: LegSide::Buy,
                price: Some(dec!(150.00)),
                quantity: Some(dec!(100)),
                time_in_force: Some("day".to_string()),
            }],
            proposed_at: chrono::Utc::now(),
            context: None,
        }
    }

    fn make_request(domain_data: serde_json::Value) -> AgentRequest {
        AgentRequest {
            request_id: Uuid::new_v4(),
            proposal: test_proposal(),
            domain_data,
            domain: "technical".to_string(),
        }
    }

    #[test]
    fn technical_oversold_boosts_confidence() {
        let data = serde_json::json!({
            "indicator:rsi_14:AAPL": {"value": [28.0]},
        });
        let request = make_request(data);
        let response = evaluate_technical(&request);
        // Base 0.50 + 0.15 oversold = 0.65
        let conf: f64 = response.confidence.to_string().parse().unwrap();
        assert!(conf > 0.60, "Expected > 0.60, got {conf}");
        assert!(response.reasoning.contains("oversold"));
    }

    #[test]
    fn technical_overbought_lowers_confidence() {
        let data = serde_json::json!({
            "indicator:rsi_14:AAPL": {"value": [78.0]},
        });
        let request = make_request(data);
        let response = evaluate_technical(&request);
        // Base 0.50 - 0.15 overbought = 0.35
        let conf: f64 = response.confidence.to_string().parse().unwrap();
        assert!(conf < 0.40, "Expected < 0.40, got {conf}");
        assert!(response.reasoning.contains("overbought"));
    }

    #[test]
    fn technical_death_cross_downtrend_warns() {
        let data = serde_json::json!({
            "indicator:sma_20:AAPL": {"value": [155.0]},
            "indicator:ema_20:AAPL": {"value": [150.0]},
            "bars:AAPL:5m": [
                {"open": 155.0, "high": 156.0, "low": 154.0, "close": 154.0, "volume": 1000.0, "timestamp": 0},
                {"open": 154.0, "high": 155.0, "low": 153.0, "close": 153.0, "volume": 1000.0, "timestamp": 1},
                {"open": 153.0, "high": 154.0, "low": 152.0, "close": 152.0, "volume": 1000.0, "timestamp": 2},
                {"open": 152.0, "high": 153.0, "low": 151.0, "close": 151.0, "volume": 1000.0, "timestamp": 3},
            ],
        });
        let request = make_request(data);
        let response = evaluate_technical(&request);
        let warns: Vec<String> = response
            .analysis
            .get("warnings")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        assert!(
            warns.iter().any(|w| w.contains("Death cross")),
            "Expected death cross warning, got: {warns:?}"
        );
    }

    #[test]
    fn macro_low_vix_boosts() {
        let data = serde_json::json!({
            "ref:VIX": {"value": [13.5]},
        });
        let request = AgentRequest {
            request_id: Uuid::new_v4(),
            proposal: test_proposal(),
            domain_data: data,
            domain: "macro".to_string(),
        };
        let response = evaluate_macro(&request);
        let conf: f64 = response.confidence.to_string().parse().unwrap();
        assert!(conf > 0.50, "Expected > 0.50 with low VIX, got {conf}");
    }

    #[test]
    fn macro_extreme_vix_warns() {
        let data = serde_json::json!({
            "ref:VIX": {"value": [38.0]},
        });
        let request = AgentRequest {
            request_id: Uuid::new_v4(),
            proposal: test_proposal(),
            domain_data: data,
            domain: "macro".to_string(),
        };
        let response = evaluate_macro(&request);
        let conf: f64 = response.confidence.to_string().parse().unwrap();
        assert!(conf < 0.35, "Expected < 0.35 with extreme VIX, got {conf}");
    }

    #[test]
    fn sentiment_positive_boosts() {
        let data = serde_json::json!({
            "sentiment:news:AAPL": {"score": 0.65, "count": 10, "timestamp": "2026-01-01T00:00:00Z"},
        });
        let request = AgentRequest {
            request_id: Uuid::new_v4(),
            proposal: test_proposal(),
            domain_data: data,
            domain: "sentiment".to_string(),
        };
        let response = evaluate_sentiment(&request);
        let conf: f64 = response.confidence.to_string().parse().unwrap();
        assert!(
            conf > 0.55,
            "Expected > 0.55 with positive sentiment, got {conf}"
        );
    }

    #[test]
    fn consecutive_trend_detection() {
        assert_eq!(consecutive_trend(&[100.0, 101.0, 102.0, 103.0]), 3);
        assert_eq!(consecutive_trend(&[103.0, 102.0, 101.0, 100.0]), -3);
        assert_eq!(consecutive_trend(&[100.0, 101.0, 100.0, 101.0]), 1);
        assert_eq!(consecutive_trend(&[100.0]), 0);
    }
}
