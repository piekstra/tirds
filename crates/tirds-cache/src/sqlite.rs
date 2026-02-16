use chrono::Utc;
use rusqlite::Connection;
use tirds_models::cache_schema::CacheRow;

use crate::error::CacheError;

/// Read-only SQLite cache accessor.
///
/// The shared SQLite database is written by external data pipeline(s)
/// and read by TIRDS. This struct provides read-only access.
pub struct SqliteReader {
    conn: Connection,
}

impl SqliteReader {
    /// Open a read-only connection to the shared cache database.
    pub fn open(path: &str) -> Result<Self, CacheError> {
        let conn = Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        Ok(Self { conn })
    }

    /// Open an in-memory database. Useful for testing - creates the schema automatically.
    /// The in-memory DB is writable so tests can seed data.
    pub fn open_in_memory() -> Result<Self, CacheError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(tirds_models::cache_schema::CACHE_TABLE_DDL)?;
        Ok(Self { conn })
    }

    /// Get a single cache entry by key. Returns None if not found or expired.
    pub fn get(&self, key: &str) -> Result<Option<CacheRow>, CacheError> {
        let now = Utc::now().to_rfc3339();
        let mut stmt = self.conn.prepare_cached(
            "SELECT key, category, value_json, source, symbol, created_at, expires_at, updated_at \
             FROM cache_entries WHERE key = ?1 AND expires_at > ?2",
        )?;

        let result = stmt.query_row(rusqlite::params![key, now], |row| {
            Ok(CacheRow {
                key: row.get(0)?,
                category: row.get(1)?,
                value_json: row.get(2)?,
                source: row.get(3)?,
                symbol: row.get(4)?,
                created_at: row.get(5)?,
                expires_at: row.get(6)?,
                updated_at: row.get(7)?,
            })
        });

        match result {
            Ok(row) => Ok(Some(row)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(CacheError::Sqlite(e)),
        }
    }

    /// Get all cache entries for a given symbol. Only returns non-expired entries.
    pub fn get_by_symbol(&self, symbol: &str) -> Result<Vec<CacheRow>, CacheError> {
        let now = Utc::now().to_rfc3339();
        let mut stmt = self.conn.prepare_cached(
            "SELECT key, category, value_json, source, symbol, created_at, expires_at, updated_at \
             FROM cache_entries WHERE symbol = ?1 AND expires_at > ?2",
        )?;

        let rows = stmt
            .query_map(rusqlite::params![symbol, now], |row| {
                Ok(CacheRow {
                    key: row.get(0)?,
                    category: row.get(1)?,
                    value_json: row.get(2)?,
                    source: row.get(3)?,
                    symbol: row.get(4)?,
                    created_at: row.get(5)?,
                    expires_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(rows)
    }

    /// Get all cache entries matching a key prefix. Only returns non-expired entries.
    pub fn get_by_prefix(&self, prefix: &str) -> Result<Vec<CacheRow>, CacheError> {
        let now = Utc::now().to_rfc3339();
        let like_pattern = format!("{prefix}%");
        let mut stmt = self.conn.prepare_cached(
            "SELECT key, category, value_json, source, symbol, created_at, expires_at, updated_at \
             FROM cache_entries WHERE key LIKE ?1 AND expires_at > ?2",
        )?;

        let rows = stmt
            .query_map(rusqlite::params![like_pattern, now], |row| {
                Ok(CacheRow {
                    key: row.get(0)?,
                    category: row.get(1)?,
                    value_json: row.get(2)?,
                    source: row.get(3)?,
                    symbol: row.get(4)?,
                    created_at: row.get(5)?,
                    expires_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(rows)
    }

    /// Insert a cache entry. In production, the data pipeline writes directly to SQLite.
    /// This method is available for testing and for the data pipeline crate to use.
    pub fn insert(&self, row: &CacheRow) -> Result<(), CacheError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO cache_entries \
             (key, category, value_json, source, symbol, created_at, expires_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                row.key,
                row.category,
                row.value_json,
                row.source,
                row.symbol,
                row.created_at,
                row.expires_at,
                row.updated_at,
            ],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn make_row(key: &str, symbol: &str, ttl_seconds: i64) -> CacheRow {
        let now = Utc::now();
        CacheRow {
            key: key.to_string(),
            category: "indicator".to_string(),
            value_json: r#"{"value": 42.5}"#.to_string(),
            source: "test".to_string(),
            symbol: Some(symbol.to_string()),
            created_at: now.to_rfc3339(),
            expires_at: (now + Duration::seconds(ttl_seconds)).to_rfc3339(),
            updated_at: now.to_rfc3339(),
        }
    }

    #[test]
    fn get_existing_key() {
        let reader = SqliteReader::open_in_memory().unwrap();
        let row = make_row("indicator:rsi_14:AAPL", "AAPL", 300);
        reader.insert(&row).unwrap();

        let result = reader.get("indicator:rsi_14:AAPL").unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().value_json, r#"{"value": 42.5}"#);
    }

    #[test]
    fn get_missing_key() {
        let reader = SqliteReader::open_in_memory().unwrap();
        let result = reader.get("nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn get_expired_key() {
        let reader = SqliteReader::open_in_memory().unwrap();
        let row = make_row("indicator:rsi_14:AAPL", "AAPL", -10); // expired 10s ago
        reader.insert(&row).unwrap();

        let result = reader.get("indicator:rsi_14:AAPL").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn get_by_symbol() {
        let reader = SqliteReader::open_in_memory().unwrap();
        reader
            .insert(&make_row("indicator:rsi_14:AAPL", "AAPL", 300))
            .unwrap();
        reader
            .insert(&make_row("indicator:sma_20:AAPL", "AAPL", 300))
            .unwrap();
        reader
            .insert(&make_row("indicator:rsi_14:TSLA", "TSLA", 300))
            .unwrap();

        let results = reader.get_by_symbol("AAPL").unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn get_by_prefix() {
        let reader = SqliteReader::open_in_memory().unwrap();
        reader
            .insert(&make_row("indicator:rsi_14:AAPL", "AAPL", 300))
            .unwrap();
        reader
            .insert(&make_row("indicator:sma_20:AAPL", "AAPL", 300))
            .unwrap();
        reader
            .insert(&make_row("bars:AAPL:1d", "AAPL", 300))
            .unwrap();

        let results = reader.get_by_prefix("indicator:").unwrap();
        assert_eq!(results.len(), 2);
    }
}
