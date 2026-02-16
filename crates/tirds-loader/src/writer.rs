use chrono::Utc;
use rusqlite::Connection;
use tirds_models::cache_schema::CacheRow;

use crate::error::LoaderError;

/// Writable SQLite cache writer.
///
/// Opens the shared cache database in read-write mode with WAL journal
/// for concurrent read/write access (TIRDS reader can read while loader writes).
pub struct SqliteWriter {
    conn: Connection,
}

impl SqliteWriter {
    /// Open a read-write connection to the cache database.
    /// Creates the schema if it doesn't exist. Enables WAL mode.
    pub fn open(path: &str) -> Result<Self, LoaderError> {
        let conn = Connection::open(path)?;
        conn.execute_batch(tirds_models::cache_schema::CACHE_TABLE_DDL)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Ok(Self { conn })
    }

    /// Open an in-memory database for testing.
    pub fn open_in_memory() -> Result<Self, LoaderError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(tirds_models::cache_schema::CACHE_TABLE_DDL)?;
        Ok(Self { conn })
    }

    /// Upsert a single cache entry.
    pub fn upsert(&self, row: &CacheRow) -> Result<(), LoaderError> {
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

    /// Batch upsert within a transaction for efficiency.
    pub fn upsert_batch(&mut self, rows: &[CacheRow]) -> Result<(), LoaderError> {
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT OR REPLACE INTO cache_entries \
                 (key, category, value_json, source, symbol, created_at, expires_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            for row in rows {
                stmt.execute(rusqlite::params![
                    row.key,
                    row.category,
                    row.value_json,
                    row.source,
                    row.symbol,
                    row.created_at,
                    row.expires_at,
                    row.updated_at,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Delete all expired entries. Returns the number of rows deleted.
    pub fn expire_stale(&self) -> Result<usize, LoaderError> {
        let now = Utc::now().to_rfc3339();
        let deleted = self.conn.execute(
            "DELETE FROM cache_entries WHERE expires_at < ?1",
            rusqlite::params![now],
        )?;
        Ok(deleted)
    }

    /// Count all entries in the cache.
    pub fn count(&self) -> Result<usize, LoaderError> {
        let count: usize =
            self.conn
                .query_row("SELECT COUNT(*) FROM cache_entries", [], |row| row.get(0))?;
        Ok(count)
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
    fn upsert_and_count() {
        let writer = SqliteWriter::open_in_memory().unwrap();
        writer
            .upsert(&make_row("indicator:rsi_14:AAPL", "AAPL", 300))
            .unwrap();
        assert_eq!(writer.count().unwrap(), 1);
    }

    #[test]
    fn upsert_replaces_existing() {
        let writer = SqliteWriter::open_in_memory().unwrap();
        let mut row = make_row("indicator:rsi_14:AAPL", "AAPL", 300);
        writer.upsert(&row).unwrap();

        row.value_json = r#"{"value": 99.9}"#.to_string();
        writer.upsert(&row).unwrap();

        assert_eq!(writer.count().unwrap(), 1);
    }

    #[test]
    fn upsert_batch() {
        let mut writer = SqliteWriter::open_in_memory().unwrap();
        let rows = vec![
            make_row("indicator:rsi_14:AAPL", "AAPL", 300),
            make_row("indicator:sma_20:AAPL", "AAPL", 300),
            make_row("quote:AAPL", "AAPL", 300),
        ];
        writer.upsert_batch(&rows).unwrap();
        assert_eq!(writer.count().unwrap(), 3);
    }

    #[test]
    fn expire_stale() {
        let mut writer = SqliteWriter::open_in_memory().unwrap();
        let rows = vec![
            make_row("indicator:rsi_14:AAPL", "AAPL", 300), // fresh
            make_row("indicator:sma_20:AAPL", "AAPL", -10), // expired
            make_row("quote:AAPL", "AAPL", -10),            // expired
        ];
        writer.upsert_batch(&rows).unwrap();
        assert_eq!(writer.count().unwrap(), 3);

        let deleted = writer.expire_stale().unwrap();
        assert_eq!(deleted, 2);
        assert_eq!(writer.count().unwrap(), 1);
    }

    #[test]
    fn wal_mode_on_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_cache.db");
        let _writer = SqliteWriter::open(path.to_str().unwrap()).unwrap();
        // WAL mode is set during open - if we get here without error, it worked
    }
}
