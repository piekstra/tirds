use serde::{Deserialize, Serialize};

/// Which market data provider to use for fetching missing data.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    #[default]
    Yahoo,
    Alpaca,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoaderConfig {
    pub cache: LoaderCacheConfig,
    pub market_data: MarketDataConfig,
    pub calculations: CalculationsConfig,
    pub stream: StreamConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoaderCacheConfig {
    /// Path to the shared SQLite cache file.
    pub sqlite_path: String,
    /// Interval in seconds between stale entry cleanup runs.
    #[serde(default = "default_cleanup_interval")]
    pub cleanup_interval_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketDataConfig {
    /// Path to the local market-data root directory (contains `data/` subdirectory).
    pub data_path: String,
    /// Symbols to load candle/quote data for.
    pub symbols: Vec<String>,
    /// Reference symbols always tracked (e.g., SPY, VIX, sector ETFs).
    #[serde(default = "default_reference_symbols")]
    pub reference_symbols: Vec<String>,
    /// Refresh interval in seconds.
    #[serde(default = "default_refresh_interval")]
    pub refresh_interval_seconds: u64,
    /// Number of recent trading days of candles to load per symbol.
    #[serde(default = "default_lookback_days")]
    pub lookback_days: u32,
    /// TTL in seconds for market data cache entries.
    #[serde(default = "default_market_ttl")]
    pub ttl_seconds: u64,
    /// Which provider to use for fetching missing market data.
    #[serde(default)]
    pub provider: ProviderKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalculationsConfig {
    /// Which indicators to compute. Format: "name" uses defaults, or "name_period" (e.g., "sma_20", "rsi_14").
    pub indicators: Vec<String>,
    /// TTL in seconds for indicator cache entries.
    #[serde(default = "default_indicator_ttl")]
    pub ttl_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamConfig {
    /// Enable/disable the streaming data source.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// TTL in seconds for streaming data cache entries.
    #[serde(default = "default_stream_ttl")]
    pub ttl_seconds: u64,
}

fn default_cleanup_interval() -> u64 {
    300
}
fn default_reference_symbols() -> Vec<String> {
    vec!["SPY".to_string(), "VIX".to_string(), "QQQ".to_string()]
}
fn default_refresh_interval() -> u64 {
    300
}
fn default_lookback_days() -> u32 {
    5
}
fn default_market_ttl() -> u64 {
    600
}
fn default_indicator_ttl() -> u64 {
    600
}
fn default_stream_ttl() -> u64 {
    1800
}
fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_example_config() {
        let toml_str = r#"
[cache]
sqlite_path = "data/tirds_cache.db"

[market_data]
data_path = "/path/to/market-data"
symbols = ["AAPL", "TSLA"]
reference_symbols = ["SPY", "VIX"]
refresh_interval_seconds = 300
lookback_days = 5
ttl_seconds = 600

[calculations]
indicators = ["sma", "rsi"]
ttl_seconds = 600

[stream]
enabled = true
ttl_seconds = 1800
"#;
        let config: LoaderConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.cache.sqlite_path, "data/tirds_cache.db");
        assert_eq!(config.market_data.symbols, vec!["AAPL", "TSLA"]);
        assert_eq!(config.calculations.indicators, vec!["sma", "rsi"]);
        assert!(config.stream.enabled);
        // Provider defaults to Yahoo when omitted
        assert_eq!(config.market_data.provider, ProviderKind::Yahoo);
    }

    #[test]
    fn deserialize_minimal_config() {
        let toml_str = r#"
[cache]
sqlite_path = "data/tirds_cache.db"

[market_data]
data_path = "/data"
symbols = ["AAPL"]

[calculations]
indicators = ["rsi"]

[stream]
"#;
        let config: LoaderConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.market_data.refresh_interval_seconds, 300);
        assert_eq!(config.market_data.lookback_days, 5);
        assert_eq!(config.stream.ttl_seconds, 1800);
        assert!(config.stream.enabled);
        assert_eq!(config.market_data.provider, ProviderKind::Yahoo);
    }

    #[test]
    fn deserialize_explicit_provider() {
        let toml_str = r#"
[cache]
sqlite_path = "data/tirds_cache.db"

[market_data]
data_path = "/data"
symbols = ["AAPL"]
provider = "alpaca"

[calculations]
indicators = ["rsi"]

[stream]
"#;
        let config: LoaderConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.market_data.provider, ProviderKind::Alpaca);
    }

    #[test]
    fn roundtrip_config() {
        let config = LoaderConfig {
            cache: LoaderCacheConfig {
                sqlite_path: "test.db".to_string(),
                cleanup_interval_seconds: 300,
            },
            market_data: MarketDataConfig {
                data_path: "/data".to_string(),
                symbols: vec!["AAPL".to_string()],
                reference_symbols: vec!["SPY".to_string()],
                refresh_interval_seconds: 300,
                lookback_days: 5,
                ttl_seconds: 600,
                provider: ProviderKind::Yahoo,
            },
            calculations: CalculationsConfig {
                indicators: vec!["sma".to_string()],
                ttl_seconds: 600,
            },
            stream: StreamConfig {
                enabled: true,
                ttl_seconds: 1800,
            },
        };
        let toml_str = toml::to_string(&config).unwrap();
        let parsed: LoaderConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.cache.sqlite_path, config.cache.sqlite_path);
        assert_eq!(parsed.market_data.symbols, config.market_data.symbols);
    }
}
