use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::Duration;
use market_data_core::store::CandleStore;
use tokio_util::sync::CancellationToken;
use tracing;

use crate::config::LoaderConfig;
use crate::error::LoaderError;
use crate::sources::{calculations, market_data, stream};
use crate::writer::SqliteWriter;

/// The loader daemon. Orchestrates periodic market data/calculation refreshes
/// and real-time stream ingestion.
pub struct Daemon {
    config: LoaderConfig,
    writer: Arc<Mutex<SqliteWriter>>,
    cancel: CancellationToken,
}

impl Daemon {
    pub fn new(config: LoaderConfig, writer: SqliteWriter) -> Self {
        Self {
            config,
            writer: Arc::new(Mutex::new(writer)),
            cancel: CancellationToken::new(),
        }
    }

    /// Returns a CancellationToken that can be used to trigger shutdown.
    pub fn cancel_token(&self) -> CancellationToken {
        self.cancel.clone()
    }

    /// Run the daemon until cancelled.
    pub async fn run(&self) -> Result<(), LoaderError> {
        tracing::info!("TIRDS loader daemon starting");

        let mut join_set = tokio::task::JoinSet::new();

        // Task 1: Combined market data + calculations periodic refresh
        {
            let config = self.config.clone();
            let writer = self.writer.clone();
            let cancel = self.cancel.clone();
            join_set.spawn(async move {
                combined_refresh_loop(config, writer, cancel).await;
            });
        }

        // Task 2: Stream ingestion (if enabled)
        if self.config.stream.enabled {
            let stream_config = self.config.stream.clone();
            let writer = self.writer.clone();
            let cancel = self.cancel.clone();
            join_set.spawn(async move {
                // Create StreamManager and subscribe
                let manager = tds::core::manager::StreamManager::new(
                    tds::core::manager::ManagerConfig::default(),
                );

                // Start all registered sources
                if let Err(e) = manager.start_all().await {
                    tracing::error!(error = %e, "Failed to start stream sources");
                    return;
                }

                let rx = manager.subscribe();
                stream::stream_loop(
                    stream_config.clone(),
                    writer,
                    rx,
                    stream_config.ttl_seconds,
                    cancel,
                )
                .await;

                manager.shutdown().await;
            });
        }

        // Task 3: Stale entry cleanup
        {
            let writer = self.writer.clone();
            let cancel = self.cancel.clone();
            let interval_secs = self.config.cache.cleanup_interval_seconds;
            join_set.spawn(async move {
                cleanup_loop(writer, interval_secs, cancel).await;
            });
        }

        tracing::info!("All loader tasks started");

        // Wait for all tasks to complete (they run until cancelled)
        while join_set.join_next().await.is_some() {}

        tracing::info!("TIRDS loader daemon stopped");
        Ok(())
    }
}

/// Combined periodic loop: fetch candles, write market data, compute indicators, write indicators.
async fn combined_refresh_loop(
    config: LoaderConfig,
    writer: Arc<Mutex<SqliteWriter>>,
    cancel: CancellationToken,
) {
    let interval = std::time::Duration::from_secs(config.market_data.refresh_interval_seconds);

    // Run immediately on startup
    run_combined_refresh(&config, &writer);

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                tracing::info!("Combined refresh loop shutting down");
                break;
            }
            _ = tokio::time::sleep(interval) => {
                run_combined_refresh(&config, &writer);
            }
        }
    }
}

/// Execute one refresh cycle: market data + calculations.
fn run_combined_refresh(config: &LoaderConfig, writer: &Arc<Mutex<SqliteWriter>>) {
    let store = CandleStore::new(&config.market_data.data_path);
    let end_date = chrono::Utc::now().date_naive();
    let start_date = end_date - Duration::days(config.market_data.lookback_days as i64);

    let all_symbols: Vec<String> = config
        .market_data
        .symbols
        .iter()
        .chain(config.market_data.reference_symbols.iter())
        .cloned()
        .collect();

    // Collect candles for all symbols (used by both market data writes and calculations)
    let mut candle_data: HashMap<String, Vec<market_data_core::candle::Candle>> = HashMap::new();
    let mut total_market_rows = 0;

    for symbol in &all_symbols {
        let category = if config
            .market_data
            .reference_symbols
            .iter()
            .any(|s| s == symbol)
        {
            "reference_symbol"
        } else {
            "market_data"
        };

        match store.read_range(symbol, start_date, end_date) {
            Ok(candles) => {
                let rows = market_data::candles_to_cache_rows(
                    symbol,
                    &candles,
                    category,
                    config.market_data.ttl_seconds,
                );
                if !rows.is_empty() {
                    match writer.lock() {
                        Ok(mut w) => {
                            if let Err(e) = w.upsert_batch(&rows) {
                                tracing::error!(symbol, error = %e, "Failed to write market data");
                            } else {
                                total_market_rows += rows.len();
                            }
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "Writer lock poisoned");
                        }
                    }
                }
                candle_data.insert(symbol.clone(), candles);
            }
            Err(e) => {
                tracing::warn!(symbol, error = %e, "Failed to read candles");
            }
        }
    }

    tracing::info!(count = total_market_rows, "Market data refresh complete");

    // Now compute indicators using the fetched candles
    match calculations::refresh_calculations(
        &all_symbols,
        &candle_data,
        &config.calculations,
        writer,
    ) {
        Ok(count) => {
            tracing::info!(count, "Indicator refresh complete");
        }
        Err(e) => {
            tracing::error!(error = %e, "Indicator refresh failed");
        }
    }
}

/// Periodically clean up expired cache entries.
async fn cleanup_loop(
    writer: Arc<Mutex<SqliteWriter>>,
    interval_secs: u64,
    cancel: CancellationToken,
) {
    let interval = std::time::Duration::from_secs(interval_secs);

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                tracing::info!("Cleanup loop shutting down");
                break;
            }
            _ = tokio::time::sleep(interval) => {
                match writer.lock() {
                    Ok(w) => {
                        match w.expire_stale() {
                            Ok(deleted) if deleted > 0 => {
                                tracing::info!(deleted, "Cleaned up stale cache entries");
                            }
                            Ok(_) => {}
                            Err(e) => {
                                tracing::error!(error = %e, "Stale cleanup failed");
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Writer lock poisoned during cleanup");
                    }
                }
            }
        }
    }
}
