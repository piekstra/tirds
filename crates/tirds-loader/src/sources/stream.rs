use std::sync::{Arc, Mutex};

use chrono::{Duration, Utc};
use tds::prelude::*;
use tirds_models::cache_schema::{key_patterns, CacheRow};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tracing;

use crate::config::StreamConfig;
use crate::writer::SqliteWriter;

/// Convert a StreamMessage into cache rows.
/// One row per ticker mentioned in the message metadata.
pub fn stream_message_to_cache_rows(msg: &StreamMessage, ttl_seconds: u64) -> Vec<CacheRow> {
    let now = Utc::now();
    let now_str = now.to_rfc3339();
    let expires_at = (now + Duration::seconds(ttl_seconds as i64)).to_rfc3339();

    let (key_prefix, category, value_json) = match &msg.payload {
        StreamPayload::News(news) => {
            let value = serde_json::json!({
                "headline": news.headline.as_str(),
                "summary": news.summary,
                "url": news.url.as_ref().map(|u| u.as_str()),
                "author": news.author.as_ref().map(|a| a.as_str()),
                "category": news.category.as_ref().map(|c| c.as_str()),
                "sentiment": msg.metadata.sentiment.as_ref().map(|s| s.score),
                "source_timestamp": msg.source_timestamp.to_rfc3339(),
            });
            ("news", "sentiment", value)
        }
        StreamPayload::SocialPost(post) => {
            let value = serde_json::json!({
                "platform": post.platform.as_str(),
                "author": post.author.as_str(),
                "content": post.content,
                "upvotes": post.upvotes,
                "comments_count": post.comments_count,
                "sentiment": msg.metadata.sentiment.as_ref().map(|s| s.score),
                "source_timestamp": msg.source_timestamp.to_rfc3339(),
            });
            ("social", "sentiment", value)
        }
        StreamPayload::Filing(filing) => {
            let value = serde_json::json!({
                "filing_type": filing.filing_type.as_str(),
                "filer_name": filing.filer_name.as_str(),
                "description": filing.description,
                "url": filing.url.as_str(),
                "filing_date": filing.filing_date.map(|d| d.to_rfc3339()),
                "source_timestamp": msg.source_timestamp.to_rfc3339(),
            });
            ("filing", "subscription", value)
        }
        StreamPayload::EconomicData(econ) => {
            let value = serde_json::json!({
                "indicator": econ.indicator.as_str(),
                "title": econ.title.as_str(),
                "value": econ.value,
                "previous_value": econ.previous_value,
                "unit": econ.unit.as_ref().map(|u| u.as_str()),
                "frequency": econ.frequency.as_ref().map(|f| f.as_str()),
                "source_timestamp": msg.source_timestamp.to_rfc3339(),
            });

            // Economic data uses ref:econ:{indicator} key, no per-ticker expansion
            let key = format!("ref:econ:{}", econ.indicator.as_str());
            let source_name = format!("tds:{:?}", msg.source);
            return vec![CacheRow {
                key,
                category: "reference_symbol".to_string(),
                value_json: serde_json::to_string(&value).unwrap_or_default(),
                source: source_name,
                symbol: None,
                created_at: now_str.clone(),
                expires_at,
                updated_at: now_str,
            }];
        }
        StreamPayload::CorporateEvent(event) => {
            let value = serde_json::json!({
                "event_type": format!("{:?}", event.event_type),
                "company": event.company.as_str(),
                "description": event.description,
                "date": event.date.map(|d| d.to_rfc3339()),
                "source_timestamp": msg.source_timestamp.to_rfc3339(),
            });
            ("event", "subscription", value)
        }
        StreamPayload::Raw(raw) => {
            let value = serde_json::json!({
                "content": raw.content,
                "content_type": raw.content_type.as_ref().map(|c| c.as_str()),
                "source_timestamp": msg.source_timestamp.to_rfc3339(),
            });
            ("raw", "sentiment", value)
        }
    };

    let source_name = format!("tds:{:?}", msg.source);
    let value_str = serde_json::to_string(&value_json).unwrap_or_default();

    // Create one row per ticker mentioned in the message
    if msg.metadata.tickers.is_empty() {
        // No tickers - still write with a generic key
        return vec![CacheRow {
            key: format!("sentiment:{key_prefix}:_general_{}", msg.id),
            category: category.to_string(),
            value_json: value_str,
            source: source_name,
            symbol: None,
            created_at: now_str.clone(),
            expires_at,
            updated_at: now_str,
        }];
    }

    msg.metadata
        .tickers
        .iter()
        .map(|ticker| {
            let symbol = ticker.symbol.as_str();
            CacheRow {
                key: key_patterns::sentiment(key_prefix, symbol),
                category: category.to_string(),
                value_json: value_str.clone(),
                source: source_name.clone(),
                symbol: Some(symbol.to_string()),
                created_at: now_str.clone(),
                expires_at: expires_at.clone(),
                updated_at: now_str.clone(),
            }
        })
        .collect()
}

/// Run the real-time stream loop.
pub async fn stream_loop(
    _config: StreamConfig,
    writer: Arc<Mutex<SqliteWriter>>,
    mut rx: broadcast::Receiver<Arc<StreamMessage>>,
    ttl_seconds: u64,
    cancel: CancellationToken,
) {
    tracing::info!("Stream loop started");

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                tracing::info!("Stream loop shutting down");
                break;
            }
            result = rx.recv() => {
                match result {
                    Ok(msg) => {
                        let rows = stream_message_to_cache_rows(&msg, ttl_seconds);
                        if !rows.is_empty() {
                            match writer.lock() {
                                Ok(mut w) => {
                                    if let Err(e) = w.upsert_batch(&rows) {
                                        tracing::error!(error = %e, "Failed to write stream data");
                                    } else {
                                        tracing::debug!(count = rows.len(), "Wrote stream entries");
                                    }
                                }
                                Err(e) => {
                                    tracing::error!(error = %e, "Writer lock poisoned");
                                }
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(skipped = n, "Stream receiver lagged");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        tracing::info!("Stream broadcast channel closed");
                        break;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tds::types::message::StreamMessage;
    use tds::types::metadata::MessageMetadata;
    use tds::types::ticker::Ticker;

    fn make_news_message(tickers: Vec<&str>) -> StreamMessage {
        StreamMessage::new(
            SourceId::Finnhub,
            Utc::now(),
            StreamPayload::News(NewsPayload {
                headline: "AAPL beats earnings".into(),
                summary: Some("Apple reported strong Q4 results".to_string()),
                url: Some("https://example.com/news".into()),
                author: Some("Analyst".into()),
                category: Some("earnings".into()),
            }),
            MessageMetadata::default()
                .with_tickers(tickers.into_iter().map(Ticker::equity).collect()),
        )
    }

    #[test]
    fn news_message_produces_sentiment_rows() {
        let msg = make_news_message(vec!["AAPL"]);
        let rows = stream_message_to_cache_rows(&msg, 1800);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].key, "sentiment:news:AAPL");
        assert_eq!(rows[0].category, "sentiment");
        assert!(rows[0].value_json.contains("AAPL beats earnings"));
        assert_eq!(rows[0].symbol, Some("AAPL".to_string()));
    }

    #[test]
    fn multi_ticker_message_produces_multiple_rows() {
        let msg = make_news_message(vec!["AAPL", "MSFT"]);
        let rows = stream_message_to_cache_rows(&msg, 1800);

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].key, "sentiment:news:AAPL");
        assert_eq!(rows[1].key, "sentiment:news:MSFT");
    }

    #[test]
    fn no_ticker_message_produces_general_row() {
        let msg = StreamMessage::new(
            SourceId::Finnhub,
            Utc::now(),
            StreamPayload::News(NewsPayload {
                headline: "Market update".into(),
                summary: None,
                url: None,
                author: None,
                category: None,
            }),
            MessageMetadata::default(),
        );
        let rows = stream_message_to_cache_rows(&msg, 1800);

        assert_eq!(rows.len(), 1);
        assert!(rows[0].key.starts_with("sentiment:news:_general_"));
        assert!(rows[0].symbol.is_none());
    }

    #[test]
    fn economic_data_uses_ref_key() {
        let msg = StreamMessage::new(
            SourceId::FredApi,
            Utc::now(),
            StreamPayload::EconomicData(EconomicDataPayload {
                indicator: "UNRATE".into(),
                title: "Unemployment Rate".into(),
                value: Some(3.7),
                previous_value: Some(3.8),
                unit: Some("percent".into()),
                frequency: Some("monthly".into()),
            }),
            MessageMetadata::default(),
        );
        let rows = stream_message_to_cache_rows(&msg, 1800);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].key, "ref:econ:UNRATE");
        assert_eq!(rows[0].category, "reference_symbol");
        assert!(rows[0].value_json.contains("3.7"));
    }

    #[test]
    fn filing_message_uses_subscription_category() {
        let msg = StreamMessage::new(
            SourceId::SecEdgarApi,
            Utc::now(),
            StreamPayload::Filing(FilingPayload {
                filing_type: "10-K".into(),
                filer_name: "Apple Inc".into(),
                filer_cik: Some("0000320193".into()),
                description: Some("Annual report".to_string()),
                url: "https://sec.gov/filing".into(),
                filing_date: None,
            }),
            MessageMetadata::default().with_tickers(vec![Ticker::equity("AAPL")]),
        );
        let rows = stream_message_to_cache_rows(&msg, 1800);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].key, "sentiment:filing:AAPL");
        assert_eq!(rows[0].category, "subscription");
    }
}
