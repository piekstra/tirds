use moka::future::Cache;
use std::time::Duration;

/// In-memory hot cache backed by moka.
///
/// Provides fast access to recently-read cache entries.
/// Entries are automatically evicted after TTL.
pub struct MemoryCache {
    inner: Cache<String, String>,
}

impl MemoryCache {
    pub fn new(max_capacity: u64, ttl: Duration) -> Self {
        Self {
            inner: Cache::builder()
                .max_capacity(max_capacity)
                .time_to_live(ttl)
                .build(),
        }
    }

    pub async fn get(&self, key: &str) -> Option<String> {
        self.inner.get(key).await
    }

    pub async fn insert(&self, key: String, value: String) {
        self.inner.insert(key, value).await;
    }

    pub async fn invalidate(&self, key: &str) {
        self.inner.invalidate(key).await;
    }

    pub fn entry_count(&self) -> u64 {
        self.inner.entry_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn insert_and_get() {
        let cache = MemoryCache::new(100, Duration::from_secs(60));
        cache.insert("key1".to_string(), "value1".to_string()).await;

        let result = cache.get("key1").await;
        assert_eq!(result, Some("value1".to_string()));
    }

    #[tokio::test]
    async fn get_missing() {
        let cache = MemoryCache::new(100, Duration::from_secs(60));
        let result = cache.get("nonexistent").await;
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn invalidate() {
        let cache = MemoryCache::new(100, Duration::from_secs(60));
        cache.insert("key1".to_string(), "value1".to_string()).await;
        cache.invalidate("key1").await;

        let result = cache.get("key1").await;
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn ttl_expiration() {
        let cache = MemoryCache::new(100, Duration::from_millis(50));
        cache.insert("key1".to_string(), "value1".to_string()).await;

        // Should exist immediately
        assert!(cache.get("key1").await.is_some());

        // Wait for TTL
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Should be expired
        assert!(cache.get("key1").await.is_none());
    }
}
