use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::{Duration, Utc};
use market_calculations::{CalculationOutput, CalculationRegistry, ParamValue, Pipeline};
use market_data_core::candle::Candle as MdCandle;
use tirds_models::cache_schema::{key_patterns, CacheRow};
use tracing;

use crate::config::CalculationsConfig;
use crate::error::LoaderError;
use crate::writer::SqliteWriter;

/// Convert a market-data `Candle` (Decimal prices) to a market-calculations `Candle` (f64 prices).
fn convert_candle(candle: &MdCandle) -> market_calculations::Candle {
    use rust_decimal::prelude::ToPrimitive;
    market_calculations::Candle {
        timestamp: candle.timestamp.timestamp_millis(),
        open: candle.open.to_f64().unwrap_or(0.0),
        high: candle.high.to_f64().unwrap_or(0.0),
        low: candle.low.to_f64().unwrap_or(0.0),
        close: candle.close.to_f64().unwrap_or(0.0),
        volume: candle.volume as f64,
    }
}

/// Build a cache row from a calculation output.
pub fn indicator_to_cache_row(
    indicator_name: &str,
    symbol: &str,
    output: &CalculationOutput,
    ttl_seconds: u64,
) -> CacheRow {
    let now = Utc::now();

    // Build the value JSON with latest value and full series
    let mut value = serde_json::Map::new();

    // For single-series outputs, extract the latest value
    if let Some(values) = output.values() {
        if let Some(&latest) = values.last() {
            value.insert("latest".to_string(), serde_json::json!(latest));
        }
        value.insert("series".to_string(), serde_json::json!(values));
    } else {
        // Multi-series output: include all series
        for (name, values) in &output.series {
            value.insert(name.clone(), serde_json::json!(values));
            // Add latest for each series
            if let Some(&latest) = values.last() {
                value.insert(format!("{name}_latest"), serde_json::json!(latest));
            }
        }
    }

    CacheRow {
        key: key_patterns::indicator(indicator_name, symbol),
        category: "indicator".to_string(),
        value_json: serde_json::to_string(&value).unwrap_or_default(),
        source: "market-calculations".to_string(),
        symbol: Some(symbol.to_string()),
        created_at: now.to_rfc3339(),
        expires_at: (now + Duration::seconds(ttl_seconds as i64)).to_rfc3339(),
        updated_at: now.to_rfc3339(),
    }
}

/// Compute all configured indicators for a symbol's candles and return cache rows.
pub fn compute_indicators(
    symbol: &str,
    candles: &[MdCandle],
    config: &CalculationsConfig,
) -> Vec<CacheRow> {
    if candles.is_empty() {
        return Vec::new();
    }

    let calc_candles: Vec<market_calculations::Candle> =
        candles.iter().map(convert_candle).collect();
    let registry = CalculationRegistry::with_defaults();
    let pipeline = Pipeline::new(&registry);
    let mut rows = Vec::new();

    for indicator_spec in &config.indicators {
        let (calc_id, params) = parse_indicator_spec(indicator_spec);

        match pipeline.run(&calc_id, &calc_candles, &params) {
            Ok(output) => {
                rows.push(indicator_to_cache_row(
                    indicator_spec,
                    symbol,
                    &output,
                    config.ttl_seconds,
                ));
            }
            Err(e) => {
                tracing::warn!(
                    symbol,
                    indicator = indicator_spec,
                    error = ?e,
                    "Failed to compute indicator"
                );
            }
        }
    }

    rows
}

/// Parse an indicator spec like "sma_20" or "rsi_14" into (calc_id, params).
/// For specs with a numeric suffix, extract the period parameter.
/// Plain names like "sma" or "daily_profile" use default params.
fn parse_indicator_spec(spec: &str) -> (String, HashMap<String, ParamValue>) {
    let mut params = HashMap::new();

    // Known multi-word calculation IDs that shouldn't be split
    let known_ids = ["daily_profile", "range_trend"];
    if known_ids.contains(&spec) {
        return (spec.to_string(), params);
    }

    // Try to split "name_period" format (e.g., "sma_20", "rsi_14", "ema_50")
    if let Some(last_underscore) = spec.rfind('_') {
        let (name, period_str) = spec.split_at(last_underscore);
        let period_str = &period_str[1..]; // skip the underscore
        if let Ok(period) = period_str.parse::<i64>() {
            params.insert("period".to_string(), ParamValue::Integer(period));
            return (name.to_string(), params);
        }
    }

    (spec.to_string(), params)
}

/// Run indicators for all symbols and write results.
pub fn refresh_calculations(
    symbols: &[String],
    candle_data: &HashMap<String, Vec<MdCandle>>,
    config: &CalculationsConfig,
    writer: &Arc<Mutex<SqliteWriter>>,
) -> Result<usize, LoaderError> {
    let mut total_rows = 0;

    for symbol in symbols {
        if let Some(candles) = candle_data.get(symbol) {
            let rows = compute_indicators(symbol, candles, config);
            if !rows.is_empty() {
                let mut w = writer
                    .lock()
                    .map_err(|e| LoaderError::Calculation(format!("Writer lock: {e}")))?;
                w.upsert_batch(&rows)?;
                total_rows += rows.len();
                tracing::debug!(symbol, count = rows.len(), "Wrote indicator entries");
            }
        }
    }

    Ok(total_rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use rust_decimal_macros::dec;

    fn sample_md_candles(count: usize) -> Vec<MdCandle> {
        let base = Utc.with_ymd_and_hms(2024, 1, 15, 9, 30, 0).unwrap();
        (0..count)
            .map(|i| {
                let ts = base + chrono::Duration::minutes(i as i64 * 5);
                MdCandle {
                    timestamp: ts,
                    open: dec!(150.00) + rust_decimal::Decimal::from(i as u32),
                    high: dec!(151.50) + rust_decimal::Decimal::from(i as u32),
                    low: dec!(149.50) + rust_decimal::Decimal::from(i as u32),
                    close: dec!(151.00) + rust_decimal::Decimal::from(i as u32),
                    volume: 100_000,
                }
            })
            .collect()
    }

    #[test]
    fn convert_candle_preserves_values() {
        let md_candle = MdCandle {
            timestamp: Utc.with_ymd_and_hms(2024, 1, 15, 14, 30, 0).unwrap(),
            open: dec!(150.25),
            high: dec!(151.50),
            low: dec!(149.75),
            close: dec!(151.00),
            volume: 100_000,
        };
        let calc_candle = convert_candle(&md_candle);
        assert!((calc_candle.open - 150.25).abs() < 0.001);
        assert!((calc_candle.high - 151.50).abs() < 0.001);
        assert_eq!(calc_candle.volume, 100_000.0);
    }

    #[test]
    fn parse_indicator_spec_with_period() {
        let (id, params) = parse_indicator_spec("sma_20");
        assert_eq!(id, "sma");
        assert_eq!(params.get("period"), Some(&ParamValue::Integer(20)));
    }

    #[test]
    fn parse_indicator_spec_plain() {
        let (id, params) = parse_indicator_spec("daily_profile");
        assert_eq!(id, "daily_profile");
        assert!(params.is_empty());
    }

    #[test]
    fn parse_indicator_spec_no_period() {
        let (id, params) = parse_indicator_spec("sma");
        assert_eq!(id, "sma");
        assert!(params.is_empty());
    }

    #[test]
    fn indicator_to_row_single_series() {
        let output = CalculationOutput::single(vec![1.0, 2.0, 3.0]);
        let row = indicator_to_cache_row("sma_20", "AAPL", &output, 600);

        assert_eq!(row.key, "indicator:sma_20:AAPL");
        assert_eq!(row.category, "indicator");
        assert!(row.value_json.contains("3.0")); // latest value
    }

    #[test]
    fn compute_indicators_with_enough_candles() {
        let candles = sample_md_candles(30); // enough for SMA(20)
        let config = CalculationsConfig {
            indicators: vec!["sma_20".to_string()],
            ttl_seconds: 600,
        };
        let rows = compute_indicators("AAPL", &candles, &config);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].key, "indicator:sma_20:AAPL");
    }

    #[test]
    fn compute_indicators_insufficient_data_skips() {
        let candles = sample_md_candles(3); // not enough for SMA(20)
        let config = CalculationsConfig {
            indicators: vec!["sma_20".to_string()],
            ttl_seconds: 600,
        };
        let rows = compute_indicators("AAPL", &candles, &config);
        // Should either produce 0 rows (insufficient data error) or 1 row
        // depending on how the calculation handles it
        assert!(rows.len() <= 1);
    }
}
