use std::sync::Mutex;
use std::time::Duration;

use serde::de::DeserializeOwned;
use tirds_models::cache_schema::CacheRow;

use crate::error::CacheError;
use crate::memory::MemoryCache;
use crate::sqlite::SqliteReader;

/// Read-through cache: checks moka (hot) → SQLite (shared) → None.
///
/// On SQLite hit, promotes the entry to the moka hot cache for subsequent fast access.
/// This is a read-only consumer - the SQLite database is written by external data pipelines.
///
/// SQLite access is synchronized via `Mutex` since `rusqlite::Connection` is not `Sync`.
pub struct CacheReader {
    memory: MemoryCache,
    sqlite: Mutex<SqliteReader>,
}

impl CacheReader {
    pub fn new(sqlite: SqliteReader, max_capacity: u64, memory_ttl: Duration) -> Self {
        Self {
            memory: MemoryCache::new(max_capacity, memory_ttl),
            sqlite: Mutex::new(sqlite),
        }
    }

    /// Get a typed value by cache key.
    /// Checks moka first, then SQLite. Promotes SQLite hits to moka.
    pub async fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, CacheError> {
        // 1. Check moka hot cache
        if let Some(json) = self.memory.get(key).await {
            return Ok(Some(serde_json::from_str(&json)?));
        }

        // 2. Check SQLite (TTL filtering happens in the query)
        let row = {
            let sqlite = self
                .sqlite
                .lock()
                .map_err(|e| CacheError::Unavailable(format!("SQLite mutex poisoned: {e}")))?;
            sqlite.get(key)?
        };

        if let Some(row) = row {
            // Promote to moka
            self.memory
                .insert(key.to_string(), row.value_json.clone())
                .await;
            return Ok(Some(serde_json::from_str(&row.value_json)?));
        }

        Ok(None)
    }

    /// Get the raw JSON string for a cache key.
    pub async fn get_json(&self, key: &str) -> Result<Option<String>, CacheError> {
        if let Some(json) = self.memory.get(key).await {
            return Ok(Some(json));
        }

        let row = {
            let sqlite = self
                .sqlite
                .lock()
                .map_err(|e| CacheError::Unavailable(format!("SQLite mutex poisoned: {e}")))?;
            sqlite.get(key)?
        };

        if let Some(row) = row {
            self.memory
                .insert(key.to_string(), row.value_json.clone())
                .await;
            return Ok(Some(row.value_json));
        }

        Ok(None)
    }

    /// Get all cache entries for a symbol as raw CacheRows.
    pub fn get_by_symbol(&self, symbol: &str) -> Result<Vec<CacheRow>, CacheError> {
        let sqlite = self
            .sqlite
            .lock()
            .map_err(|e| CacheError::Unavailable(format!("SQLite mutex poisoned: {e}")))?;
        sqlite.get_by_symbol(symbol)
    }

    /// Get all cache entries matching a key prefix as raw CacheRows.
    pub fn get_by_prefix(&self, prefix: &str) -> Result<Vec<CacheRow>, CacheError> {
        let sqlite = self
            .sqlite
            .lock()
            .map_err(|e| CacheError::Unavailable(format!("SQLite mutex poisoned: {e}")))?;
        sqlite.get_by_prefix(prefix)
    }

    /// Build a domain data snapshot for a symbol.
    /// Collects all cache entries for the symbol and merges them into a single JSON object.
    pub fn build_domain_snapshot(&self, symbol: &str) -> Result<serde_json::Value, CacheError> {
        let sqlite = self
            .sqlite
            .lock()
            .map_err(|e| CacheError::Unavailable(format!("SQLite mutex poisoned: {e}")))?;
        let rows = sqlite.get_by_symbol(symbol)?;
        let mut map = serde_json::Map::new();
        for row in rows {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&row.value_json) {
                map.insert(row.key, value);
            }
        }
        Ok(serde_json::Value::Object(map))
    }

    /// Get the number of entries in the hot moka cache.
    pub fn hot_cache_size(&self) -> u64 {
        self.memory.entry_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration as ChronoDuration, Utc};

    fn make_row(key: &str, symbol: &str, value_json: &str, ttl_seconds: i64) -> CacheRow {
        let now = Utc::now();
        CacheRow {
            key: key.to_string(),
            category: "indicator".to_string(),
            value_json: value_json.to_string(),
            source: "test".to_string(),
            symbol: Some(symbol.to_string()),
            created_at: now.to_rfc3339(),
            expires_at: (now + ChronoDuration::seconds(ttl_seconds)).to_rfc3339(),
            updated_at: now.to_rfc3339(),
        }
    }

    fn setup_reader() -> CacheReader {
        let sqlite = SqliteReader::open_in_memory().unwrap();
        sqlite
            .insert(&make_row(
                "indicator:rsi_14:AAPL",
                "AAPL",
                r#"{"value": 35.5}"#,
                300,
            ))
            .unwrap();
        sqlite
            .insert(&make_row(
                "indicator:sma_20:AAPL",
                "AAPL",
                r#"{"value": 152.30}"#,
                300,
            ))
            .unwrap();
        sqlite
            .insert(&make_row(
                "quote:AAPL",
                "AAPL",
                r#"{"price": 150.25, "volume": 1000000}"#,
                300,
            ))
            .unwrap();

        CacheReader::new(sqlite, 100, Duration::from_secs(60))
    }

    #[tokio::test]
    async fn read_through_sqlite_to_moka() {
        let reader = setup_reader();

        // First read should come from SQLite
        let result: Option<serde_json::Value> = reader.get("indicator:rsi_14:AAPL").await.unwrap();
        assert!(result.is_some());
        let val = result.unwrap();
        assert_eq!(val["value"], serde_json::json!(35.5));

        // After first read, the entry should be promoted to moka.
        let result2: Option<serde_json::Value> = reader.get("indicator:rsi_14:AAPL").await.unwrap();
        assert_eq!(result2.unwrap()["value"], serde_json::json!(35.5));

        // Verify moka has the entry
        let raw = reader.memory.get("indicator:rsi_14:AAPL").await;
        assert!(raw.is_some());
    }

    #[tokio::test]
    async fn get_json_raw() {
        let reader = setup_reader();

        let json = reader.get_json("quote:AAPL").await.unwrap();
        assert!(json.is_some());
        assert!(json.unwrap().contains("150.25"));
    }

    #[tokio::test]
    async fn get_missing_returns_none() {
        let reader = setup_reader();

        let result: Option<serde_json::Value> = reader.get("nonexistent").await.unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn get_by_symbol() {
        let reader = setup_reader();

        let rows = reader.get_by_symbol("AAPL").unwrap();
        assert_eq!(rows.len(), 3); // rsi, sma, quote
    }

    #[test]
    fn build_domain_snapshot() {
        let reader = setup_reader();

        let snapshot = reader.build_domain_snapshot("AAPL").unwrap();
        assert!(snapshot.is_object());
        let obj = snapshot.as_object().unwrap();
        assert_eq!(obj.len(), 3);
        assert!(obj.contains_key("indicator:rsi_14:AAPL"));
        assert!(obj.contains_key("indicator:sma_20:AAPL"));
        assert!(obj.contains_key("quote:AAPL"));
    }
}
