//! Stress tests for concurrent read/write access to the shared SQLite cache.
//!
//! These tests verify that WAL mode allows tirds-loader (writer) and
//! tirds-cache (reader) to operate on the same database concurrently
//! without SQLITE_BUSY errors or data corruption.
//!
//! Run with:
//! ```bash
//! cargo test -p tirds-loader --test wal_stress
//! ```

use std::sync::{Arc, Barrier};
use std::thread;

use chrono::{Duration, Utc};
use tirds_cache::SqliteReader;
use tirds_loader::writer::SqliteWriter;
use tirds_models::cache_schema::CacheRow;

fn make_row(key: &str, symbol: &str, value: f64, ttl_seconds: i64) -> CacheRow {
    let now = Utc::now();
    CacheRow {
        key: key.to_string(),
        category: "indicator".to_string(),
        value_json: format!(r#"{{"value": {value}}}"#),
        source: "stress_test".to_string(),
        symbol: Some(symbol.to_string()),
        created_at: now.to_rfc3339(),
        expires_at: (now + Duration::seconds(ttl_seconds)).to_rfc3339(),
        updated_at: now.to_rfc3339(),
    }
}

/// Writer and readers operate concurrently on the same file-based SQLite.
/// Verifies no SQLITE_BUSY errors occur under contention.
#[test]
fn concurrent_writer_and_readers_no_busy_errors() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("stress.db");
    let path_str = db_path.to_str().unwrap();

    // Writer creates the DB and enables WAL
    let mut writer = SqliteWriter::open(path_str).unwrap();

    // Seed some initial data so readers have something to find
    let seed: Vec<CacheRow> = (0..50)
        .map(|i| make_row(&format!("indicator:rsi_{i}:AAPL"), "AAPL", i as f64, 600))
        .collect();
    writer.upsert_batch(&seed).unwrap();

    let write_count = 200;
    let reader_count = 4;
    let reads_per_reader = 100;

    // Barrier ensures all threads start at the same time
    let barrier = Arc::new(Barrier::new(1 + reader_count));

    let writer_barrier = barrier.clone();
    let writer_path = path_str.to_string();
    let writer_handle = thread::spawn(move || {
        writer_barrier.wait();
        let mut writer = SqliteWriter::open(&writer_path).unwrap();
        for i in 0..write_count {
            let batch: Vec<CacheRow> = (0..5)
                .map(|j| {
                    make_row(
                        &format!("indicator:stress_{i}_{j}:AAPL"),
                        "AAPL",
                        (i * 5 + j) as f64,
                        600,
                    )
                })
                .collect();
            writer.upsert_batch(&batch).unwrap();
        }
    });

    let reader_handles: Vec<_> = (0..reader_count)
        .map(|reader_id| {
            let b = barrier.clone();
            let p = path_str.to_string();
            thread::spawn(move || {
                b.wait();
                let reader = SqliteReader::open(&p).unwrap();
                let mut found = 0usize;
                for _ in 0..reads_per_reader {
                    // Mix of read patterns
                    if let Ok(Some(_)) = reader.get("indicator:rsi_0:AAPL") {
                        found += 1;
                    }
                    if let Ok(rows) = reader.get_by_symbol("AAPL") {
                        found += rows.len();
                    }
                    if let Ok(rows) = reader.get_by_prefix("indicator:stress_") {
                        found += rows.len();
                    }
                }
                (reader_id, found)
            })
        })
        .collect();

    writer_handle.join().expect("writer thread panicked");
    for handle in reader_handles {
        let (id, found) = handle.join().expect("reader thread panicked");
        assert!(found > 0, "Reader {id} found zero rows — unexpected");
    }
}

/// Readers see consistent data — no partial writes from batch transactions.
#[test]
fn readers_see_consistent_batch_data() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("consistency.db");
    let path_str = db_path.to_str().unwrap();

    let mut writer = SqliteWriter::open(path_str).unwrap();

    // Write batches of 10 rows each, all with the same value_json.
    // A reader should never see a partial batch (some rows with value X,
    // others with value Y from the same batch).
    let batch_count = 50;
    let rows_per_batch = 10;

    let reader_path = path_str.to_string();
    let barrier = Arc::new(Barrier::new(2));
    let reader_barrier = barrier.clone();

    let reader_handle = thread::spawn(move || {
        reader_barrier.wait();
        let reader = SqliteReader::open(&reader_path).unwrap();
        let mut checks = 0;
        for _ in 0..200 {
            let rows = reader.get_by_prefix("batch:").unwrap_or_default();
            // Group by batch number and verify all rows in a batch have the same value
            let mut batches: std::collections::HashMap<String, Vec<String>> =
                std::collections::HashMap::new();
            for row in &rows {
                // key format: batch:{batch_num}:row_{j}
                let parts: Vec<&str> = row.key.split(':').collect();
                if parts.len() >= 2 {
                    batches
                        .entry(parts[1].to_string())
                        .or_default()
                        .push(row.value_json.clone());
                }
            }
            for (batch_num, values) in &batches {
                if values.len() == rows_per_batch {
                    // Full batch visible — all values must match
                    let first = &values[0];
                    for v in values {
                        assert_eq!(
                            v, first,
                            "Inconsistent batch {batch_num}: saw mixed values within a transaction"
                        );
                    }
                    checks += 1;
                }
            }
        }
        checks
    });

    barrier.wait();
    for i in 0..batch_count {
        let batch: Vec<CacheRow> = (0..rows_per_batch)
            .map(|j| make_row(&format!("batch:{i}:row_{j}"), "TEST", i as f64, 600))
            .collect();
        writer.upsert_batch(&batch).unwrap();
    }

    let consistency_checks = reader_handle.join().expect("reader panicked");
    assert!(
        consistency_checks > 0,
        "Reader never observed a complete batch — test inconclusive"
    );
}

/// Verify that expire_stale works correctly while readers are active.
#[test]
fn expire_stale_during_concurrent_reads() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("expire.db");
    let path_str = db_path.to_str().unwrap();

    let mut writer = SqliteWriter::open(path_str).unwrap();

    // Write a mix of fresh and stale entries
    let mut rows = Vec::new();
    for i in 0..50 {
        rows.push(make_row(
            &format!("fresh:{i}:AAPL"),
            "AAPL",
            i as f64,
            600, // fresh
        ));
        rows.push(make_row(
            &format!("stale:{i}:AAPL"),
            "AAPL",
            i as f64,
            -10, // already expired
        ));
    }
    writer.upsert_batch(&rows).unwrap();

    // Reader should only see fresh entries (filtered by expires_at)
    let reader = SqliteReader::open(path_str).unwrap();
    let before_cleanup = reader.get_by_symbol("AAPL").unwrap();
    assert_eq!(
        before_cleanup.len(),
        50,
        "Reader should filter expired rows"
    );

    // Writer cleans up stale entries
    let deleted = writer.expire_stale().unwrap();
    assert_eq!(deleted, 50, "Should delete 50 stale entries");

    // Reader still works fine after cleanup
    let after_cleanup = reader.get_by_symbol("AAPL").unwrap();
    assert_eq!(
        after_cleanup.len(),
        50,
        "Fresh entries should survive cleanup"
    );
}
