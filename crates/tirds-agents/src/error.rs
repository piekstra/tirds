use thiserror::Error;

#[derive(Error, Debug)]
pub enum AgentError {
    #[error("Claude CLI error: {0}")]
    Cli(String),

    #[error("Agent response parse error: {0}")]
    Parse(String),

    #[error("Agent timed out after {0} seconds")]
    Timeout(u64),

    #[error("Agent disabled: {0}")]
    Disabled(String),

    #[error("Cache error: {0}")]
    Cache(#[from] tirds_cache::CacheError),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}
