use serde::{Deserialize, Serialize};

/// Categories for organizing cache keys.
/// The data pipeline uses these when writing to the shared SQLite cache.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CacheCategory {
    MarketData,
    Indicator,
    ReferenceSymbol,
    Subscription,
    Sentiment,
}

/// The expected SQLite table schema that the data pipeline must write to
/// and TIRDS reads from.
///
/// ```sql
/// CREATE TABLE IF NOT EXISTS cache_entries (
///     key         TEXT PRIMARY KEY,
///     category    TEXT NOT NULL,
///     value_json  TEXT NOT NULL,
///     source      TEXT NOT NULL,
///     symbol      TEXT,
///     created_at  TEXT NOT NULL,
///     expires_at  TEXT NOT NULL,
///     updated_at  TEXT NOT NULL
/// );
///
/// CREATE INDEX IF NOT EXISTS idx_cache_category ON cache_entries(category);
/// CREATE INDEX IF NOT EXISTS idx_cache_symbol ON cache_entries(symbol);
/// CREATE INDEX IF NOT EXISTS idx_cache_expires ON cache_entries(expires_at);
/// ```
pub const CACHE_TABLE_DDL: &str = "\
CREATE TABLE IF NOT EXISTS cache_entries (
    key         TEXT PRIMARY KEY,
    category    TEXT NOT NULL,
    value_json  TEXT NOT NULL,
    source      TEXT NOT NULL,
    symbol      TEXT,
    created_at  TEXT NOT NULL,
    expires_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_cache_category ON cache_entries(category);
CREATE INDEX IF NOT EXISTS idx_cache_symbol ON cache_entries(symbol);
CREATE INDEX IF NOT EXISTS idx_cache_expires ON cache_entries(expires_at);
";

/// Key pattern conventions for the cache.
///
/// Data pipelines should use these patterns when writing cache entries
/// so that TIRDS agents can predictably query them.
///
/// - Market data bars: `bars:{symbol}:{timeframe}` (e.g., `bars:AAPL:1d`)
/// - Market data quotes: `quote:{symbol}` (e.g., `quote:AAPL`)
/// - Indicators: `indicator:{name}:{symbol}` (e.g., `indicator:rsi_14:AAPL`)
/// - Reference symbols: `ref:{symbol}` (e.g., `ref:SPY`, `ref:VIX`)
/// - Sentiment: `sentiment:{source}:{symbol}` (e.g., `sentiment:twitter:AAPL`)
pub mod key_patterns {
    pub fn bars(symbol: &str, timeframe: &str) -> String {
        format!("bars:{symbol}:{timeframe}")
    }

    pub fn quote(symbol: &str) -> String {
        format!("quote:{symbol}")
    }

    pub fn indicator(name: &str, symbol: &str) -> String {
        format!("indicator:{name}:{symbol}")
    }

    pub fn reference_symbol(symbol: &str) -> String {
        format!("ref:{symbol}")
    }

    pub fn sentiment(source: &str, symbol: &str) -> String {
        format!("sentiment:{source}:{symbol}")
    }
}

/// A raw cache row as read from SQLite.
#[derive(Debug, Clone)]
pub struct CacheRow {
    pub key: String,
    pub category: String,
    pub value_json: String,
    pub source: String,
    pub symbol: Option<String>,
    pub created_at: String,
    pub expires_at: String,
    pub updated_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_pattern_bars() {
        assert_eq!(key_patterns::bars("AAPL", "1d"), "bars:AAPL:1d");
    }

    #[test]
    fn key_pattern_quote() {
        assert_eq!(key_patterns::quote("SPY"), "quote:SPY");
    }

    #[test]
    fn key_pattern_indicator() {
        assert_eq!(
            key_patterns::indicator("rsi_14", "AAPL"),
            "indicator:rsi_14:AAPL"
        );
    }

    #[test]
    fn key_pattern_reference() {
        assert_eq!(key_patterns::reference_symbol("VIX"), "ref:VIX");
    }

    #[test]
    fn key_pattern_sentiment() {
        assert_eq!(
            key_patterns::sentiment("twitter", "TSLA"),
            "sentiment:twitter:TSLA"
        );
    }

    #[test]
    fn cache_category_roundtrip() {
        let categories = vec![
            CacheCategory::MarketData,
            CacheCategory::Indicator,
            CacheCategory::ReferenceSymbol,
            CacheCategory::Subscription,
            CacheCategory::Sentiment,
        ];
        for cat in categories {
            let json = serde_json::to_string(&cat).unwrap();
            let parsed: CacheCategory = serde_json::from_str(&json).unwrap();
            assert_eq!(cat, parsed);
        }
    }
}
