use std::sync::{Arc, Mutex};

use chrono::{Duration, NaiveDate, Utc};
use market_data_core::candle::Candle;
use market_data_core::store::CandleStore;
use market_data_providers::provider::CandleProvider;
use tirds_models::cache_schema::{key_patterns, CacheRow};
use tokio_util::sync::CancellationToken;
use tracing;

use crate::config::{MarketDataConfig, ProviderKind};
use crate::error::LoaderError;
use crate::writer::SqliteWriter;

/// Create a market data provider based on the configured kind.
pub fn create_provider(kind: &ProviderKind) -> Result<Box<dyn CandleProvider>, LoaderError> {
    match kind {
        ProviderKind::Yahoo => Ok(Box::new(market_data_providers::yahoo::YahooProvider::new())),
        ProviderKind::Alpaca => {
            let provider = market_data_providers::alpaca::AlpacaProvider::from_env()
                .map_err(|e| LoaderError::Provider(format!("Alpaca provider: {e}")))?;
            Ok(Box::new(provider))
        }
    }
}

/// Fill missing market data by fetching from the configured provider.
/// Returns the number of days fetched and written to the local store.
pub async fn fill_missing_data(
    store: &CandleStore,
    provider: &dyn CandleProvider,
    symbol: &str,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<usize, LoaderError> {
    let missing = store.missing_dates(symbol, start, end);
    if missing.is_empty() {
        return Ok(0);
    }

    tracing::info!(
        symbol,
        missing_days = missing.len(),
        provider = provider.name(),
        "Fetching missing market data"
    );

    // Fetch the contiguous range covering all missing dates
    let fetch_start = missing[0];
    let fetch_end = missing[missing.len() - 1];

    let fetched = provider
        .fetch_candles_range(symbol, fetch_start, fetch_end)
        .await
        .map_err(|e| LoaderError::Provider(format!("{symbol}: {e}")))?;

    let mut days_written = 0;
    for (date, candles) in &fetched {
        if missing.contains(date) && !candles.is_empty() {
            store
                .write_day(symbol, *date, candles)
                .map_err(|e| LoaderError::Provider(format!("{symbol} write {date}: {e}")))?;
            days_written += 1;
        }
    }

    tracing::info!(symbol, days_written, "Finished filling missing data");
    Ok(days_written)
}

/// Convert a market-data `Candle` to a JSON-serializable value.
fn candle_to_json(candle: &Candle) -> serde_json::Value {
    serde_json::json!({
        "timestamp": candle.timestamp.to_rfc3339(),
        "open": candle.open.to_string(),
        "high": candle.high.to_string(),
        "low": candle.low.to_string(),
        "close": candle.close.to_string(),
        "volume": candle.volume,
    })
}

/// Build cache rows from candles for a single symbol.
pub fn candles_to_cache_rows(
    symbol: &str,
    candles: &[Candle],
    category: &str,
    ttl_seconds: u64,
) -> Vec<CacheRow> {
    let now = Utc::now();
    let expires_at = (now + Duration::seconds(ttl_seconds as i64)).to_rfc3339();
    let now_str = now.to_rfc3339();
    let mut rows = Vec::new();

    // Write bars entry with all candles
    if !candles.is_empty() {
        let bars_json: Vec<serde_json::Value> = candles.iter().map(candle_to_json).collect();
        rows.push(CacheRow {
            key: key_patterns::bars(symbol, "5m"),
            category: category.to_string(),
            value_json: serde_json::to_string(&bars_json).unwrap_or_default(),
            source: "market-data".to_string(),
            symbol: Some(symbol.to_string()),
            created_at: now_str.clone(),
            expires_at: expires_at.clone(),
            updated_at: now_str.clone(),
        });

        // Write quote entry from the most recent candle
        let latest = &candles[candles.len() - 1];
        let quote_json = serde_json::json!({
            "price": latest.close.to_string(),
            "volume": latest.volume,
            "timestamp": latest.timestamp.to_rfc3339(),
        });
        rows.push(CacheRow {
            key: key_patterns::quote(symbol),
            category: category.to_string(),
            value_json: serde_json::to_string(&quote_json).unwrap_or_default(),
            source: "market-data".to_string(),
            symbol: Some(symbol.to_string()),
            created_at: now_str.clone(),
            expires_at: expires_at.clone(),
            updated_at: now_str,
        });
    }

    rows
}

/// Refresh market data for all configured symbols.
fn refresh_market_data(
    config: &MarketDataConfig,
    writer: &Arc<Mutex<SqliteWriter>>,
) -> Result<usize, LoaderError> {
    let store = CandleStore::new(&config.data_path);

    let end_date = Utc::now().date_naive();
    let start_date = end_date - Duration::days(config.lookback_days as i64);

    let all_symbols: Vec<&str> = config
        .symbols
        .iter()
        .chain(config.reference_symbols.iter())
        .map(|s| s.as_str())
        .collect();

    let mut total_rows = 0;

    for symbol in &all_symbols {
        let category = if config.reference_symbols.iter().any(|s| s == *symbol) {
            "reference_symbol"
        } else {
            "market_data"
        };

        match load_symbol(
            &store,
            symbol,
            start_date,
            end_date,
            category,
            config.ttl_seconds,
        ) {
            Ok(rows) => {
                if !rows.is_empty() {
                    let mut w = writer
                        .lock()
                        .map_err(|e| LoaderError::MarketData(format!("Writer lock: {e}")))?;
                    w.upsert_batch(&rows)?;
                    total_rows += rows.len();
                    tracing::debug!(symbol, count = rows.len(), "Wrote market data entries");
                }
            }
            Err(e) => {
                tracing::warn!(symbol, error = %e, "Failed to load market data");
            }
        }
    }

    Ok(total_rows)
}

fn load_symbol(
    store: &CandleStore,
    symbol: &str,
    start: NaiveDate,
    end: NaiveDate,
    category: &str,
    ttl_seconds: u64,
) -> Result<Vec<CacheRow>, LoaderError> {
    let candles = store
        .read_range(symbol, start, end)
        .map_err(|e| LoaderError::MarketData(format!("{symbol}: {e}")))?;

    Ok(candles_to_cache_rows(
        symbol,
        &candles,
        category,
        ttl_seconds,
    ))
}

/// Run the periodic market data refresh loop.
pub async fn market_data_loop(
    config: MarketDataConfig,
    writer: Arc<Mutex<SqliteWriter>>,
    cancel: CancellationToken,
) {
    let interval = std::time::Duration::from_secs(config.refresh_interval_seconds);

    // Refresh immediately on startup
    match refresh_market_data(&config, &writer) {
        Ok(count) => tracing::info!(count, "Initial market data refresh complete"),
        Err(e) => tracing::error!(error = %e, "Initial market data refresh failed"),
    }

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                tracing::info!("Market data loop shutting down");
                break;
            }
            _ = tokio::time::sleep(interval) => {
                match refresh_market_data(&config, &writer) {
                    Ok(count) => tracing::debug!(count, "Market data refresh complete"),
                    Err(e) => tracing::error!(error = %e, "Market data refresh failed"),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::TimeZone;
    use market_data_providers::error::ProviderError;
    use rust_decimal_macros::dec;
    use std::collections::HashMap;
    use std::sync::Mutex as StdMutex;

    /// Mock provider that returns pre-configured candles per (symbol, date).
    struct MockProvider {
        data: HashMap<(String, NaiveDate), Vec<Candle>>,
        fetch_count: StdMutex<usize>,
    }

    impl MockProvider {
        fn new(data: HashMap<(String, NaiveDate), Vec<Candle>>) -> Self {
            Self {
                data,
                fetch_count: StdMutex::new(0),
            }
        }

        fn fetch_count(&self) -> usize {
            *self.fetch_count.lock().unwrap_or_else(|e| e.into_inner())
        }
    }

    #[async_trait]
    impl CandleProvider for MockProvider {
        fn name(&self) -> &str {
            "mock"
        }

        async fn fetch_candles(
            &self,
            symbol: &str,
            date: NaiveDate,
        ) -> Result<Vec<Candle>, ProviderError> {
            *self.fetch_count.lock().unwrap_or_else(|e| e.into_inner()) += 1;
            Ok(self
                .data
                .get(&(symbol.to_string(), date))
                .cloned()
                .unwrap_or_default())
        }
    }

    fn sample_candles() -> Vec<Candle> {
        vec![
            Candle {
                timestamp: Utc.with_ymd_and_hms(2024, 1, 15, 14, 30, 0).unwrap(),
                open: dec!(150.00),
                high: dec!(151.50),
                low: dec!(149.50),
                close: dec!(151.00),
                volume: 100_000,
            },
            Candle {
                timestamp: Utc.with_ymd_and_hms(2024, 1, 15, 14, 35, 0).unwrap(),
                open: dec!(151.00),
                high: dec!(152.00),
                low: dec!(150.50),
                close: dec!(151.75),
                volume: 85_000,
            },
        ]
    }

    #[test]
    fn candles_to_rows_produces_bars_and_quote() {
        let candles = sample_candles();
        let rows = candles_to_cache_rows("AAPL", &candles, "market_data", 600);
        assert_eq!(rows.len(), 2); // bars + quote

        assert_eq!(rows[0].key, "bars:AAPL:5m");
        assert_eq!(rows[0].category, "market_data");
        assert!(rows[0].value_json.contains("151.00"));

        assert_eq!(rows[1].key, "quote:AAPL");
        assert!(rows[1].value_json.contains("151.75")); // latest close
    }

    #[test]
    fn empty_candles_produce_no_rows() {
        let rows = candles_to_cache_rows("AAPL", &[], "market_data", 600);
        assert!(rows.is_empty());
    }

    #[test]
    fn reference_symbol_uses_correct_category() {
        let candles = sample_candles();
        let rows = candles_to_cache_rows("SPY", &candles, "reference_symbol", 600);
        assert_eq!(rows[0].category, "reference_symbol");
    }

    #[test]
    fn create_provider_yahoo_default() {
        let provider = create_provider(&ProviderKind::Yahoo).unwrap();
        assert_eq!(provider.name(), "yahoo");
    }

    #[test]
    fn create_provider_alpaca_fails_without_env() {
        // Alpaca requires ALPACA_API_KEY_ID and ALPACA_API_SECRET_KEY
        let result = create_provider(&ProviderKind::Alpaca);
        assert!(result.is_err());
    }

    fn sample_candles_for_date(date: NaiveDate) -> Vec<Candle> {
        vec![Candle {
            timestamp: date.and_hms_opt(14, 30, 0).unwrap().and_utc(),
            open: dec!(150.00),
            high: dec!(151.50),
            low: dec!(149.50),
            close: dec!(151.00),
            volume: 100_000,
        }]
    }

    #[tokio::test]
    async fn fill_missing_data_fetches_and_writes() {
        let dir = tempfile::tempdir().unwrap();
        let store = CandleStore::new(dir.path().to_str().unwrap());

        // Monday Jan 13, 2025 and Tuesday Jan 14, 2025 (both weekdays)
        let date1 = NaiveDate::from_ymd_opt(2025, 1, 13).unwrap();
        let date2 = NaiveDate::from_ymd_opt(2025, 1, 14).unwrap();

        let mut data = HashMap::new();
        data.insert(("TEST".to_string(), date1), sample_candles_for_date(date1));
        data.insert(("TEST".to_string(), date2), sample_candles_for_date(date2));
        let provider = MockProvider::new(data);

        let days = fill_missing_data(&store, &provider, "TEST", date1, date2)
            .await
            .unwrap();

        assert_eq!(days, 2);
        assert!(provider.fetch_count() > 0);

        // Data should now be readable from the store
        let candles = store.read_range("TEST", date1, date2).unwrap();
        assert!(!candles.is_empty());
    }

    #[tokio::test]
    async fn fill_missing_data_no_gaps() {
        let dir = tempfile::tempdir().unwrap();
        let store = CandleStore::new(dir.path().to_str().unwrap());

        let date = NaiveDate::from_ymd_opt(2025, 1, 13).unwrap();

        // Pre-populate the store
        store
            .write_day("TEST", date, &sample_candles_for_date(date))
            .unwrap();

        let provider = MockProvider::new(HashMap::new());

        let days = fill_missing_data(&store, &provider, "TEST", date, date)
            .await
            .unwrap();

        assert_eq!(days, 0);
        assert_eq!(provider.fetch_count(), 0);
    }
}
