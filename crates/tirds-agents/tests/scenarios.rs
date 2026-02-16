//! Integration tests for trading scenario evaluation.
//!
//! Each test seeds an in-memory SQLite cache with realistic market data,
//! runs ScenarioMockSpecialist agents to produce domain-aware responses,
//! then calls `build_trade_decision()` to produce a full TradeDecision.

use std::sync::Arc;
use std::time::Duration;

use chrono::{Duration as ChronoDuration, Utc};
use rust_decimal_macros::dec;
use tirds_agents::build_trade_decision;
use tirds_agents::test_support::{build_synthesized_json, ScenarioMockSpecialist};
use tirds_agents::SpecialistAgent;
use tirds_cache::{CacheReader, SqliteReader};
use tirds_models::agent_message::{AgentRequest, AgentResponse};
use tirds_models::cache_schema::CacheRow;
use tirds_models::trade_decision::AgentReport;
use tirds_models::trade_input::{LegSide, TradeLeg, TradeProposal, INPUT_SCHEMA_VERSION};
use uuid::Uuid;

fn make_cache_row(key: &str, category: &str, symbol: Option<&str>, value_json: &str) -> CacheRow {
    let now = Utc::now();
    CacheRow {
        key: key.to_string(),
        category: category.to_string(),
        value_json: value_json.to_string(),
        source: "test".to_string(),
        symbol: symbol.map(|s| s.to_string()),
        created_at: now.to_rfc3339(),
        expires_at: (now + ChronoDuration::hours(1)).to_rfc3339(),
        updated_at: now.to_rfc3339(),
    }
}

fn make_buy_proposal(symbol: &str, price: rust_decimal::Decimal) -> TradeProposal {
    TradeProposal {
        id: Uuid::new_v4(),
        schema_version: INPUT_SCHEMA_VERSION,
        symbol: symbol.to_string(),
        legs: vec![TradeLeg {
            side: LegSide::Buy,
            price: Some(price),
            quantity: Some(dec!(100)),
            time_in_force: Some("day".to_string()),
        }],
        proposed_at: Utc::now(),
        context: None,
    }
}

fn setup_cache(rows: Vec<CacheRow>) -> Arc<CacheReader> {
    let sqlite = SqliteReader::open_in_memory().unwrap();
    for row in &rows {
        sqlite.insert(row).unwrap();
    }
    Arc::new(CacheReader::new(sqlite, 100, Duration::from_secs(60)))
}

/// Make rising close bars (uptrend).
fn make_rising_bars(start: f64, count: usize) -> String {
    let bars: Vec<serde_json::Value> = (0..count)
        .map(|i| {
            let close = start + i as f64 * 1.0;
            serde_json::json!({
                "open": close - 0.5,
                "high": close + 0.5,
                "low": close - 1.0,
                "close": close,
                "volume": 10000.0,
                "timestamp": i * 60000,
            })
        })
        .collect();
    serde_json::to_string(&bars).unwrap()
}

/// Make falling close bars (downtrend).
fn make_falling_bars(start: f64, count: usize) -> String {
    let bars: Vec<serde_json::Value> = (0..count)
        .map(|i| {
            let close = start - i as f64 * 1.0;
            serde_json::json!({
                "open": close + 0.5,
                "high": close + 1.0,
                "low": close - 0.5,
                "close": close,
                "volume": 10000.0,
                "timestamp": i * 60000,
            })
        })
        .collect();
    serde_json::to_string(&bars).unwrap()
}

/// Make sideways bars.
fn make_sideways_bars(center: f64, count: usize) -> String {
    let bars: Vec<serde_json::Value> = (0..count)
        .map(|i| {
            // Alternate slightly up/down
            let offset = if i % 2 == 0 { 0.2 } else { -0.2 };
            let close = center + offset;
            serde_json::json!({
                "open": center,
                "high": close + 0.5,
                "low": close - 0.5,
                "close": close,
                "volume": 10000.0,
                "timestamp": i * 60000,
            })
        })
        .collect();
    serde_json::to_string(&bars).unwrap()
}

async fn run_scenario(
    proposal: &TradeProposal,
    cache: &Arc<CacheReader>,
) -> (Vec<AgentResponse>, Vec<AgentReport>) {
    let domain_snapshot = cache.build_domain_snapshot(&proposal.symbol).unwrap();

    let specialists: Vec<Box<dyn SpecialistAgent>> = vec![
        Box::new(ScenarioMockSpecialist::technical()),
        Box::new(ScenarioMockSpecialist::macro_analyst()),
        Box::new(ScenarioMockSpecialist::sentiment()),
        Box::new(ScenarioMockSpecialist::sector()),
    ];

    let mut responses = Vec::new();
    let mut reports = Vec::new();

    for spec in &specialists {
        let request = AgentRequest {
            request_id: Uuid::new_v4(),
            proposal: proposal.clone(),
            domain_data: domain_snapshot.clone(),
            domain: spec.domain().to_string(),
        };

        let response = spec.evaluate(&request).await.unwrap();
        reports.push(AgentReport {
            agent_name: spec.name().to_string(),
            domain: spec.domain().to_string(),
            confidence: response.confidence,
            reasoning: response.reasoning.clone(),
            data_sources_used: response.data_sources_consulted.clone(),
            elapsed_ms: 100,
        });
        responses.push(response);
    }

    (responses, reports)
}

// ============================================================
// Scenario 1: Oversold Bounce Buy
// RSI 28, EMA > SMA (golden cross), MACD bullish, VIX 14.5,
// Sentiment +0.65, Sector outperforming
// Expected: confidence > 0.65
// ============================================================

#[tokio::test]
async fn scenario_oversold_bounce_buy() {
    let cache = setup_cache(vec![
        // Technical
        make_cache_row(
            "indicator:rsi_14:AAPL",
            "indicator",
            Some("AAPL"),
            r#"{"value": [35.0, 30.0, 28.0]}"#,
        ),
        make_cache_row(
            "indicator:sma_20:AAPL",
            "indicator",
            Some("AAPL"),
            r#"{"value": [148.0, 149.0, 150.0]}"#,
        ),
        make_cache_row(
            "indicator:ema_20:AAPL",
            "indicator",
            Some("AAPL"),
            r#"{"value": [149.0, 150.5, 152.0]}"#,
        ),
        make_cache_row(
            "indicator:macd:AAPL",
            "indicator",
            Some("AAPL"),
            r#"{"macd_line": [-0.5, 0.2, 0.8], "signal_line": [0.1, 0.1, 0.3], "histogram": [-0.6, 0.1, 0.5]}"#,
        ),
        make_cache_row(
            "bars:AAPL:5m",
            "market_data",
            Some("AAPL"),
            &make_rising_bars(148.0, 5),
        ),
        // Macro
        make_cache_row(
            "ref:VIX",
            "reference_symbol",
            Some("AAPL"),
            r#"{"value": [15.0, 14.8, 14.5]}"#,
        ),
        make_cache_row(
            "bars:SPY:1d",
            "market_data",
            Some("AAPL"),
            &make_rising_bars(450.0, 5),
        ),
        // Sentiment
        make_cache_row(
            "sentiment:news:AAPL",
            "sentiment",
            Some("AAPL"),
            r#"{"score": 0.65, "count": 12, "timestamp": "2026-02-16T12:00:00Z"}"#,
        ),
        // Sector
        make_cache_row(
            "ref:XLK",
            "reference_symbol",
            Some("AAPL"),
            r#"{"value": [190.0, 193.0, 198.0]}"#,
        ),
        make_cache_row(
            "ref:SPY",
            "reference_symbol",
            Some("AAPL"),
            r#"{"value": [450.0, 451.0, 453.0]}"#,
        ),
        make_cache_row(
            "bars:XLK:1d",
            "market_data",
            Some("AAPL"),
            &make_rising_bars(190.0, 5),
        ),
    ]);

    let proposal = make_buy_proposal("AAPL", dec!(148.00));
    let (responses, reports) = run_scenario(&proposal, &cache).await;

    let synthesized = build_synthesized_json(&proposal, &responses);
    let decision =
        build_trade_decision(&proposal, &synthesized, &reports, Duration::from_secs(1)).unwrap();

    let overall: f64 = decision
        .overall_confidence
        .score
        .to_string()
        .parse()
        .unwrap();
    println!("Scenario 1 (Oversold Bounce): confidence = {overall:.4}");
    println!("Reasoning: {}", decision.overall_confidence.reasoning);

    assert!(
        overall > 0.65,
        "Oversold bounce should yield confidence > 0.65, got {overall:.4}"
    );
    assert_eq!(decision.symbol, "AAPL");
    assert_eq!(decision.leg_assessments.len(), 1);
    assert_eq!(decision.agent_reports.len(), 4);
}

// ============================================================
// Scenario 2: Overbought Warning
// RSI 78, price above upper Bollinger, VIX 28.5,
// Sentiment -0.55
// Expected: confidence < 0.45, warnings present
// ============================================================

#[tokio::test]
async fn scenario_overbought_warning() {
    let cache = setup_cache(vec![
        // Technical: very overbought
        make_cache_row(
            "indicator:rsi_14:AAPL",
            "indicator",
            Some("AAPL"),
            r#"{"value": [65.0, 72.0, 78.0]}"#,
        ),
        make_cache_row(
            "indicator:bollinger_bands:AAPL",
            "indicator",
            Some("AAPL"),
            r#"{"upper": [160.0], "middle": [155.0], "lower": [150.0], "bandwidth": [6.0], "percent_b": [1.15]}"#,
        ),
        make_cache_row(
            "indicator:macd:AAPL",
            "indicator",
            Some("AAPL"),
            r#"{"macd_line": [1.5, 1.2, 0.8], "signal_line": [0.5, 0.8, 1.0], "histogram": [1.0, 0.4, -0.2]}"#,
        ),
        make_cache_row(
            "bars:AAPL:5m",
            "market_data",
            Some("AAPL"),
            &make_sideways_bars(162.0, 5),
        ),
        // Macro: elevated VIX
        make_cache_row(
            "ref:VIX",
            "reference_symbol",
            Some("AAPL"),
            r#"{"value": [22.0, 25.0, 28.5]}"#,
        ),
        make_cache_row(
            "bars:SPY:1d",
            "market_data",
            Some("AAPL"),
            &make_falling_bars(455.0, 4),
        ),
        // Sentiment: negative
        make_cache_row(
            "sentiment:news:AAPL",
            "sentiment",
            Some("AAPL"),
            r#"{"score": -0.55, "count": 8, "timestamp": "2026-02-16T12:00:00Z"}"#,
        ),
        make_cache_row(
            "sentiment:social:AAPL",
            "sentiment",
            Some("AAPL"),
            r#"{"score": -0.60, "mentions": 500, "timestamp": "2026-02-16T12:00:00Z"}"#,
        ),
        // Sector: underperforming
        make_cache_row(
            "ref:XLK",
            "reference_symbol",
            Some("AAPL"),
            r#"{"value": [195.0, 193.0, 190.0]}"#,
        ),
        make_cache_row(
            "ref:SPY",
            "reference_symbol",
            Some("AAPL"),
            r#"{"value": [450.0, 451.0, 453.0]}"#,
        ),
        make_cache_row(
            "bars:XLK:1d",
            "market_data",
            Some("AAPL"),
            &make_falling_bars(195.0, 5),
        ),
    ]);

    let proposal = make_buy_proposal("AAPL", dec!(162.00));
    let (responses, reports) = run_scenario(&proposal, &cache).await;

    let synthesized = build_synthesized_json(&proposal, &responses);
    let decision =
        build_trade_decision(&proposal, &synthesized, &reports, Duration::from_secs(1)).unwrap();

    let overall: f64 = decision
        .overall_confidence
        .score
        .to_string()
        .parse()
        .unwrap();
    println!("Scenario 2 (Overbought Warning): confidence = {overall:.4}");
    println!("Reasoning: {}", decision.overall_confidence.reasoning);

    assert!(
        overall < 0.45,
        "Overbought scenario should yield confidence < 0.45, got {overall:.4}"
    );

    // Should have warnings
    let has_warnings = decision
        .trade_intelligence
        .assessments
        .iter()
        .any(|a| a.contains("overbought") || a.contains("reversal") || a.contains("extended"));
    assert!(
        has_warnings,
        "Expected warnings about overbought conditions, got: {:?}",
        decision.trade_intelligence.assessments
    );
}

// ============================================================
// Scenario 3: Death Cross Downtrend
// RSI 42, EMA < SMA (death cross), 4 consecutive lower closes,
// VIX 32, Sentiment -0.3
// Expected: confidence < 0.40, "avoid" or "death cross" warning
// ============================================================

#[tokio::test]
async fn scenario_death_cross_downtrend() {
    let cache = setup_cache(vec![
        // Technical: death cross + downtrend
        make_cache_row(
            "indicator:rsi_14:AAPL",
            "indicator",
            Some("AAPL"),
            r#"{"value": [50.0, 45.0, 42.0]}"#,
        ),
        make_cache_row(
            "indicator:sma_20:AAPL",
            "indicator",
            Some("AAPL"),
            r#"{"value": [155.0, 155.0, 155.0]}"#,
        ),
        make_cache_row(
            "indicator:ema_20:AAPL",
            "indicator",
            Some("AAPL"),
            r#"{"value": [153.0, 151.0, 149.0]}"#,
        ),
        make_cache_row(
            "indicator:macd:AAPL",
            "indicator",
            Some("AAPL"),
            r#"{"macd_line": [-0.5, -1.0, -1.5], "signal_line": [-0.2, -0.5, -0.8], "histogram": [-0.3, -0.5, -0.7]}"#,
        ),
        make_cache_row(
            "bars:AAPL:5m",
            "market_data",
            Some("AAPL"),
            &make_falling_bars(155.0, 6),
        ),
        // Macro: high VIX + SPY falling
        make_cache_row(
            "ref:VIX",
            "reference_symbol",
            Some("AAPL"),
            r#"{"value": [25.0, 28.0, 32.0]}"#,
        ),
        make_cache_row(
            "bars:SPY:1d",
            "market_data",
            Some("AAPL"),
            &make_falling_bars(455.0, 5),
        ),
        // Sentiment: mildly negative
        make_cache_row(
            "sentiment:news:AAPL",
            "sentiment",
            Some("AAPL"),
            r#"{"score": -0.30, "count": 5, "timestamp": "2026-02-16T12:00:00Z"}"#,
        ),
        // Sector: falling
        make_cache_row(
            "ref:XLK",
            "reference_symbol",
            Some("AAPL"),
            r#"{"value": [195.0, 192.0, 188.0]}"#,
        ),
        make_cache_row(
            "ref:SPY",
            "reference_symbol",
            Some("AAPL"),
            r#"{"value": [455.0, 453.0, 450.0]}"#,
        ),
        make_cache_row(
            "bars:XLK:1d",
            "market_data",
            Some("AAPL"),
            &make_falling_bars(195.0, 5),
        ),
    ]);

    let proposal = make_buy_proposal("AAPL", dec!(149.00));
    let (responses, reports) = run_scenario(&proposal, &cache).await;

    let synthesized = build_synthesized_json(&proposal, &responses);
    let decision =
        build_trade_decision(&proposal, &synthesized, &reports, Duration::from_secs(1)).unwrap();

    let overall: f64 = decision
        .overall_confidence
        .score
        .to_string()
        .parse()
        .unwrap();
    println!("Scenario 3 (Death Cross Downtrend): confidence = {overall:.4}");
    println!("Reasoning: {}", decision.overall_confidence.reasoning);

    assert!(
        overall < 0.40,
        "Death cross downtrend should yield confidence < 0.40, got {overall:.4}"
    );

    // Should have death cross or downtrend warning
    let has_death_cross = decision.trade_intelligence.assessments.iter().any(|a| {
        a.to_lowercase().contains("death cross")
            || a.to_lowercase().contains("avoid")
            || a.to_lowercase().contains("downtrend")
    });
    assert!(
        has_death_cross,
        "Expected death cross/downtrend warning, got: {:?}",
        decision.trade_intelligence.assessments
    );
}

// ============================================================
// Scenario 4: Golden Cross Recovery
// RSI 52, EMA > SMA (golden cross), 4 higher closes,
// VIX 13.5, Sentiment +0.75, Sector leading
// Expected: confidence > 0.65
// ============================================================

#[tokio::test]
async fn scenario_golden_cross_recovery() {
    let cache = setup_cache(vec![
        // Technical: golden cross + uptrend
        make_cache_row(
            "indicator:rsi_14:AAPL",
            "indicator",
            Some("AAPL"),
            r#"{"value": [45.0, 48.0, 52.0]}"#,
        ),
        make_cache_row(
            "indicator:sma_20:AAPL",
            "indicator",
            Some("AAPL"),
            r#"{"value": [148.0, 149.0, 150.0]}"#,
        ),
        make_cache_row(
            "indicator:ema_20:AAPL",
            "indicator",
            Some("AAPL"),
            r#"{"value": [149.0, 151.0, 153.0]}"#,
        ),
        make_cache_row(
            "indicator:macd:AAPL",
            "indicator",
            Some("AAPL"),
            r#"{"macd_line": [-0.2, 0.3, 0.9], "signal_line": [0.0, 0.1, 0.4], "histogram": [-0.2, 0.2, 0.5]}"#,
        ),
        make_cache_row(
            "bars:AAPL:5m",
            "market_data",
            Some("AAPL"),
            &make_rising_bars(148.0, 6),
        ),
        // Macro: very calm
        make_cache_row(
            "ref:VIX",
            "reference_symbol",
            Some("AAPL"),
            r#"{"value": [15.0, 14.0, 13.5]}"#,
        ),
        make_cache_row(
            "bars:SPY:1d",
            "market_data",
            Some("AAPL"),
            &make_rising_bars(448.0, 5),
        ),
        // Sentiment: very positive
        make_cache_row(
            "sentiment:news:AAPL",
            "sentiment",
            Some("AAPL"),
            r#"{"score": 0.75, "count": 15, "timestamp": "2026-02-16T12:00:00Z"}"#,
        ),
        make_cache_row(
            "sentiment:social:AAPL",
            "sentiment",
            Some("AAPL"),
            r#"{"score": 0.55, "mentions": 200, "timestamp": "2026-02-16T12:00:00Z"}"#,
        ),
        // Sector: strongly outperforming
        make_cache_row(
            "ref:XLK",
            "reference_symbol",
            Some("AAPL"),
            r#"{"value": [185.0, 190.0, 196.0]}"#,
        ),
        make_cache_row(
            "ref:SPY",
            "reference_symbol",
            Some("AAPL"),
            r#"{"value": [448.0, 450.0, 453.0]}"#,
        ),
        make_cache_row(
            "bars:XLK:1d",
            "market_data",
            Some("AAPL"),
            &make_rising_bars(185.0, 5),
        ),
    ]);

    let proposal = make_buy_proposal("AAPL", dec!(152.00));
    let (responses, reports) = run_scenario(&proposal, &cache).await;

    let synthesized = build_synthesized_json(&proposal, &responses);
    let decision =
        build_trade_decision(&proposal, &synthesized, &reports, Duration::from_secs(1)).unwrap();

    let overall: f64 = decision
        .overall_confidence
        .score
        .to_string()
        .parse()
        .unwrap();
    println!("Scenario 4 (Golden Cross Recovery): confidence = {overall:.4}");
    println!("Reasoning: {}", decision.overall_confidence.reasoning);

    assert!(
        overall > 0.65,
        "Golden cross recovery should yield confidence > 0.65, got {overall:.4}"
    );
}

// ============================================================
// Scenario 5: Mixed Signals
// RSI 50, EMA ~ SMA, sideways bars, VIX 18, Sentiment +0.1
// Expected: confidence 0.40-0.60 (near neutral)
// ============================================================

#[tokio::test]
async fn scenario_mixed_signals() {
    let cache = setup_cache(vec![
        // Technical: neutral everything
        make_cache_row(
            "indicator:rsi_14:AAPL",
            "indicator",
            Some("AAPL"),
            r#"{"value": [50.0, 50.0, 50.0]}"#,
        ),
        make_cache_row(
            "indicator:sma_20:AAPL",
            "indicator",
            Some("AAPL"),
            r#"{"value": [150.0, 150.0, 150.0]}"#,
        ),
        make_cache_row(
            "indicator:ema_20:AAPL",
            "indicator",
            Some("AAPL"),
            r#"{"value": [150.0, 150.0, 150.0]}"#,
        ),
        make_cache_row(
            "bars:AAPL:5m",
            "market_data",
            Some("AAPL"),
            &make_sideways_bars(150.0, 5),
        ),
        // Macro: normal
        make_cache_row(
            "ref:VIX",
            "reference_symbol",
            Some("AAPL"),
            r#"{"value": [18.0, 18.0, 18.0]}"#,
        ),
        make_cache_row(
            "bars:SPY:1d",
            "market_data",
            Some("AAPL"),
            &make_sideways_bars(450.0, 5),
        ),
        // Sentiment: barely positive
        make_cache_row(
            "sentiment:news:AAPL",
            "sentiment",
            Some("AAPL"),
            r#"{"score": 0.10, "count": 3, "timestamp": "2026-02-16T12:00:00Z"}"#,
        ),
        // Sector: inline
        make_cache_row(
            "ref:XLK",
            "reference_symbol",
            Some("AAPL"),
            r#"{"value": [190.0, 190.5, 191.0]}"#,
        ),
        make_cache_row(
            "ref:SPY",
            "reference_symbol",
            Some("AAPL"),
            r#"{"value": [450.0, 450.3, 450.7]}"#,
        ),
        make_cache_row(
            "bars:XLK:1d",
            "market_data",
            Some("AAPL"),
            &make_sideways_bars(190.0, 5),
        ),
    ]);

    let proposal = make_buy_proposal("AAPL", dec!(150.00));
    let (responses, reports) = run_scenario(&proposal, &cache).await;

    let synthesized = build_synthesized_json(&proposal, &responses);
    let decision =
        build_trade_decision(&proposal, &synthesized, &reports, Duration::from_secs(1)).unwrap();

    let overall: f64 = decision
        .overall_confidence
        .score
        .to_string()
        .parse()
        .unwrap();
    println!("Scenario 5 (Mixed Signals): confidence = {overall:.4}");
    println!("Reasoning: {}", decision.overall_confidence.reasoning);

    assert!(
        (0.40..=0.60).contains(&overall),
        "Mixed signals should yield confidence 0.40-0.60, got {overall:.4}"
    );
}

// ============================================================
// Verify full TradeDecision structure is complete
// ============================================================

#[tokio::test]
async fn decision_structure_complete() {
    let cache = setup_cache(vec![
        make_cache_row(
            "indicator:rsi_14:AAPL",
            "indicator",
            Some("AAPL"),
            r#"{"value": [50.0]}"#,
        ),
        make_cache_row(
            "ref:VIX",
            "reference_symbol",
            Some("AAPL"),
            r#"{"value": [18.0]}"#,
        ),
    ]);

    let proposal = make_buy_proposal("AAPL", dec!(150.00));
    let (responses, reports) = run_scenario(&proposal, &cache).await;

    let synthesized = build_synthesized_json(&proposal, &responses);
    let decision =
        build_trade_decision(&proposal, &synthesized, &reports, Duration::from_secs(1)).unwrap();

    // Verify all fields are populated
    assert_eq!(
        decision.schema_version,
        tirds_models::trade_decision::OUTPUT_SCHEMA_VERSION
    );
    assert_eq!(decision.proposal_id, proposal.id);
    assert_eq!(decision.symbol, "AAPL");
    assert!(!decision.leg_assessments.is_empty());
    assert!(!decision.timeline.is_empty());
    assert_eq!(decision.agent_reports.len(), 4);
    assert!(decision.processing_time_ms > 0);

    // TradeDecision should round-trip through JSON
    let json = serde_json::to_string_pretty(&decision).unwrap();
    let parsed: tirds_models::trade_decision::TradeDecision = serde_json::from_str(&json).unwrap();
    assert_eq!(decision.symbol, parsed.symbol);
    assert_eq!(
        decision.overall_confidence.score,
        parsed.overall_confidence.score
    );

    println!("Full TradeDecision JSON:\n{json}");
}
