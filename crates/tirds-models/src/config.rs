use serde::{Deserialize, Serialize};

/// Top-level configuration for TIRDS.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TirdsConfig {
    pub cache: CacheConfig,
    pub agents: AgentsConfig,
}

/// Configuration for the cache reader layer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CacheConfig {
    /// Path to the shared SQLite cache file (written by data pipeline, read by TIRDS).
    pub sqlite_path: String,
    /// Maximum number of entries in the in-memory moka cache.
    pub memory_max_capacity: u64,
    /// Default TTL in seconds for moka entries (how long to keep a read in memory).
    pub memory_ttl_seconds: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            sqlite_path: "data/tirds_cache.db".to_string(),
            memory_max_capacity: 10_000,
            memory_ttl_seconds: 60,
        }
    }
}

/// Configuration for the agent orchestration layer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentsConfig {
    /// Total timeout for the entire evaluation pipeline in seconds.
    pub total_timeout_seconds: u64,
    /// Per-specialist agent timeout in seconds.
    pub specialist_timeout_seconds: u64,
    /// Model to use for the synthesizer (final aggregation).
    pub synthesizer_model: String,
    /// Default model for specialist agents.
    pub specialist_model: String,
    /// List of specialist agent configurations.
    pub specialists: Vec<SpecialistConfig>,
}

impl Default for AgentsConfig {
    fn default() -> Self {
        Self {
            total_timeout_seconds: 120,
            specialist_timeout_seconds: 45,
            synthesizer_model: "claude-sonnet-4-5-20250929".to_string(),
            specialist_model: "claude-3-5-haiku-latest".to_string(),
            specialists: vec![
                SpecialistConfig {
                    name: "technical".to_string(),
                    domain: "technical".to_string(),
                    model: None,
                    enabled: true,
                },
                SpecialistConfig {
                    name: "macro".to_string(),
                    domain: "macro".to_string(),
                    model: None,
                    enabled: true,
                },
                SpecialistConfig {
                    name: "sentiment".to_string(),
                    domain: "sentiment".to_string(),
                    model: None,
                    enabled: true,
                },
                SpecialistConfig {
                    name: "sector".to_string(),
                    domain: "sector".to_string(),
                    model: None,
                    enabled: true,
                },
            ],
        }
    }
}

/// Configuration for a single specialist agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpecialistConfig {
    pub name: String,
    pub domain: String,
    /// Override model for this specialist. Falls back to `AgentsConfig::specialist_model`.
    pub model: Option<String>,
    pub enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_tirds_config() {
        let config = TirdsConfig {
            cache: CacheConfig::default(),
            agents: AgentsConfig::default(),
        };

        let json = serde_json::to_string(&config).unwrap();
        let deserialized: TirdsConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, deserialized);
    }

    #[test]
    fn default_config_has_four_specialists() {
        let agents = AgentsConfig::default();
        assert_eq!(agents.specialists.len(), 4);
        assert!(agents.specialists.iter().all(|s| s.enabled));
    }

    #[test]
    fn config_from_toml() {
        let toml_str = r#"
[cache]
sqlite_path = "/tmp/test_cache.db"
memory_max_capacity = 5000
memory_ttl_seconds = 30

[agents]
total_timeout_seconds = 60
specialist_timeout_seconds = 20
synthesizer_model = "claude-sonnet-4-5-20250929"
specialist_model = "claude-3-5-haiku-latest"

[[agents.specialists]]
name = "technical"
domain = "technical"
enabled = true

[[agents.specialists]]
name = "macro"
domain = "macro"
enabled = false
"#;

        let config: TirdsConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.cache.sqlite_path, "/tmp/test_cache.db");
        assert_eq!(config.agents.specialists.len(), 2);
        assert!(!config.agents.specialists[1].enabled);
    }
}
