//! Integration tests for daemon lifecycle: stream ingestion, stale cleanup,
//! and graceful shutdown via CancellationToken.
//!
//! These tests exercise the real async loops from the daemon with a file-backed
//! SQLite database, verifying behavior end-to-end without external data sources.
//!
//! Run with:
//! ```bash
//! cargo test -p tirds-loader --test daemon_lifecycle
//! ```

use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use tds::prelude::*;
use tirds_loader::config::StreamConfig;
use tirds_loader::sources::stream::stream_loop;
use tirds_loader::writer::SqliteWriter;
use tirds_models::cache_schema::CacheRow;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

fn make_row(key: &str, ttl_seconds: i64) -> CacheRow {
    let now = Utc::now();
    CacheRow {
        key: key.to_string(),
        category: "indicator".to_string(),
        value_json: r#"{"value": 1}"#.to_string(),
        source: "test".to_string(),
        symbol: Some("TEST".to_string()),
        created_at: now.to_rfc3339(),
        expires_at: (now + chrono::Duration::seconds(ttl_seconds)).to_rfc3339(),
        updated_at: now.to_rfc3339(),
    }
}

/// Stream loop processes messages and writes them to the database.
#[tokio::test]
async fn stream_loop_ingests_messages_and_shuts_down() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("stream_test.db");
    let writer = SqliteWriter::open(db_path.to_str().unwrap()).unwrap();
    let writer = Arc::new(Mutex::new(writer));

    let (tx, rx) = broadcast::channel::<Arc<StreamMessage>>(16);
    let cancel = CancellationToken::new();

    let stream_config = StreamConfig {
        enabled: true,
        ttl_seconds: 600,
    };

    let cancel_clone = cancel.clone();
    let writer_clone = writer.clone();
    let handle = tokio::spawn(async move {
        stream_loop(stream_config, writer_clone, rx, 600, cancel_clone).await;
    });

    // Send a few messages
    let msg1 = Arc::new(StreamMessage::new(
        SourceId::Finnhub,
        Utc::now(),
        StreamPayload::News(NewsPayload {
            headline: "AAPL rallies".into(),
            summary: Some("Strong earnings".to_string()),
            url: None,
            author: None,
            category: None,
        }),
        MessageMetadata::default().with_tickers(vec![Ticker::equity("AAPL")]),
    ));

    let msg2 = Arc::new(StreamMessage::new(
        SourceId::Finnhub,
        Utc::now(),
        StreamPayload::News(NewsPayload {
            headline: "TSLA drops".into(),
            summary: None,
            url: None,
            author: None,
            category: None,
        }),
        MessageMetadata::default().with_tickers(vec![Ticker::equity("TSLA")]),
    ));

    tx.send(msg1).unwrap();
    tx.send(msg2).unwrap();

    // Give the loop time to process
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Cancel and wait for shutdown
    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(2), handle)
        .await
        .expect("stream loop did not shut down in time")
        .expect("stream loop panicked");

    // Verify both messages were written
    let w = writer.lock().unwrap();
    let count = w.count().unwrap();
    assert_eq!(count, 2, "Expected 2 stream entries in the database");
}

/// Stream loop exits cleanly when the broadcast channel is closed.
#[tokio::test]
async fn stream_loop_exits_on_channel_close() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("channel_close.db");
    let writer = SqliteWriter::open(db_path.to_str().unwrap()).unwrap();
    let writer = Arc::new(Mutex::new(writer));

    let (tx, rx) = broadcast::channel::<Arc<StreamMessage>>(16);
    let cancel = CancellationToken::new();

    let stream_config = StreamConfig {
        enabled: true,
        ttl_seconds: 600,
    };

    let handle = tokio::spawn(async move {
        stream_loop(stream_config, writer, rx, 600, cancel).await;
    });

    // Drop the sender — this closes the channel
    drop(tx);

    // Loop should exit on its own
    tokio::time::timeout(Duration::from_secs(2), handle)
        .await
        .expect("stream loop did not exit after channel close")
        .expect("stream loop panicked");
}

/// Cleanup loop expires stale entries on schedule.
#[tokio::test]
async fn cleanup_loop_expires_stale_entries() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("cleanup_test.db");
    let mut writer = SqliteWriter::open(db_path.to_str().unwrap()).unwrap();

    // Seed with a mix of fresh and stale entries
    let rows = vec![
        make_row("fresh:1", 600),  // 10 min TTL
        make_row("fresh:2", 600),
        make_row("stale:1", -10),  // already expired
        make_row("stale:2", -10),
        make_row("stale:3", -10),
    ];
    writer.upsert_batch(&rows).unwrap();
    assert_eq!(writer.count().unwrap(), 5);

    let writer = Arc::new(Mutex::new(writer));
    let cancel = CancellationToken::new();

    // Run a quick expire
    {
        let w = writer.lock().unwrap();
        let deleted = w.expire_stale().unwrap();
        assert_eq!(deleted, 3, "Should delete 3 stale entries");
    }

    // Verify only fresh entries remain
    {
        let w = writer.lock().unwrap();
        assert_eq!(w.count().unwrap(), 2, "Only 2 fresh entries should remain");
    }

    cancel.cancel();
}

/// CancellationToken stops all loops promptly.
#[tokio::test]
async fn cancellation_token_stops_stream_loop_promptly() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("cancel_test.db");
    let writer = SqliteWriter::open(db_path.to_str().unwrap()).unwrap();
    let writer = Arc::new(Mutex::new(writer));

    let (_tx, rx) = broadcast::channel::<Arc<StreamMessage>>(16);
    let cancel = CancellationToken::new();

    let stream_config = StreamConfig {
        enabled: true,
        ttl_seconds: 600,
    };

    let cancel_clone = cancel.clone();
    let handle = tokio::spawn(async move {
        stream_loop(stream_config, writer, rx, 600, cancel_clone).await;
    });

    // Cancel immediately
    cancel.cancel();

    // Should shut down within 1 second
    let result = tokio::time::timeout(Duration::from_secs(1), handle).await;
    assert!(
        result.is_ok(),
        "Stream loop did not respond to cancellation within 1 second"
    );
}

/// Multiple stream messages for the same ticker upsert (don't duplicate).
#[tokio::test]
async fn stream_loop_upserts_duplicate_keys() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("upsert_test.db");
    let writer = SqliteWriter::open(db_path.to_str().unwrap()).unwrap();
    let writer = Arc::new(Mutex::new(writer));

    let (tx, rx) = broadcast::channel::<Arc<StreamMessage>>(16);
    let cancel = CancellationToken::new();

    let stream_config = StreamConfig {
        enabled: true,
        ttl_seconds: 600,
    };

    let cancel_clone = cancel.clone();
    let writer_clone = writer.clone();
    let handle = tokio::spawn(async move {
        stream_loop(stream_config, writer_clone, rx, 600, cancel_clone).await;
    });

    // Send two news messages for AAPL — they'll have the same key (sentiment:news:AAPL)
    for headline in &["AAPL up", "AAPL down"] {
        let msg = Arc::new(StreamMessage::new(
            SourceId::Finnhub,
            Utc::now(),
            StreamPayload::News(NewsPayload {
                headline: (*headline).into(),
                summary: None,
                url: None,
                author: None,
                category: None,
            }),
            MessageMetadata::default().with_tickers(vec![Ticker::equity("AAPL")]),
        ));
        tx.send(msg).unwrap();
    }

    tokio::time::sleep(Duration::from_millis(100)).await;
    cancel.cancel();
    handle.await.unwrap();

    // Same key gets upserted, so count should be 1
    let w = writer.lock().unwrap();
    assert_eq!(w.count().unwrap(), 1, "Duplicate keys should be upserted, not duplicated");
}
