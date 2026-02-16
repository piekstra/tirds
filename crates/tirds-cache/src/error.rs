use thiserror::Error;

#[derive(Error, Debug)]
pub enum CacheError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("JSON deserialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Cache entry expired: key={0}")]
    Expired(String),

    #[error("Cache not available: {0}")]
    Unavailable(String),
}
