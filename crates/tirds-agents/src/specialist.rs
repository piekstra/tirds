use async_trait::async_trait;
use tirds_models::agent_message::{AgentRequest, AgentResponse};

use crate::claude_cli::{invoke_claude, ClaudeCliConfig};
use crate::error::AgentError;
use crate::parser::parse_agent_response;
use crate::prompts::get_specialist_prompt;

/// Trait for specialist agents. Mockable for testing.
#[async_trait]
pub trait SpecialistAgent: Send + Sync {
    fn name(&self) -> &str;
    fn domain(&self) -> &str;

    async fn evaluate(&self, request: &AgentRequest) -> Result<AgentResponse, AgentError>;
}

/// A specialist agent that invokes the Claude CLI.
pub struct ClaudeSpecialist {
    pub name: String,
    pub domain: String,
    pub cli_config: ClaudeCliConfig,
}

impl ClaudeSpecialist {
    pub fn new(name: String, domain: String, model: String, timeout: std::time::Duration) -> Self {
        Self {
            name,
            domain,
            cli_config: ClaudeCliConfig { model, timeout },
        }
    }
}

#[async_trait]
impl SpecialistAgent for ClaudeSpecialist {
    fn name(&self) -> &str {
        &self.name
    }

    fn domain(&self) -> &str {
        &self.domain
    }

    async fn evaluate(&self, request: &AgentRequest) -> Result<AgentResponse, AgentError> {
        let system_prompt = get_specialist_prompt(&self.domain).ok_or_else(|| {
            AgentError::Cli(format!("No system prompt for domain: {}", self.domain))
        })?;

        let user_prompt = serde_json::to_string(request)?;
        let raw_output = invoke_claude(&system_prompt, &user_prompt, &self.cli_config).await?;
        parse_agent_response(&raw_output)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use uuid::Uuid;

    /// Mock specialist for testing the orchestrator without Claude CLI.
    pub struct MockSpecialist {
        pub name: String,
        pub domain: String,
        pub response: Arc<Mutex<AgentResponse>>,
        pub should_fail: bool,
    }

    impl MockSpecialist {
        pub fn new(name: &str, domain: &str, confidence: rust_decimal::Decimal) -> Self {
            Self {
                name: name.to_string(),
                domain: domain.to_string(),
                response: Arc::new(Mutex::new(AgentResponse {
                    request_id: Uuid::nil(),
                    agent_name: name.to_string(),
                    domain: domain.to_string(),
                    confidence,
                    reasoning: format!("Mock {name} analysis"),
                    analysis: serde_json::json!({"mock": true}),
                    data_sources_consulted: vec![format!("mock_{domain}_data")],
                })),
                should_fail: false,
            }
        }

        pub fn failing(name: &str, domain: &str) -> Self {
            let mut mock = Self::new(name, domain, dec!(0.0));
            mock.should_fail = true;
            mock
        }
    }

    #[async_trait]
    impl SpecialistAgent for MockSpecialist {
        fn name(&self) -> &str {
            &self.name
        }

        fn domain(&self) -> &str {
            &self.domain
        }

        async fn evaluate(&self, request: &AgentRequest) -> Result<AgentResponse, AgentError> {
            if self.should_fail {
                return Err(AgentError::Cli("Mock failure".to_string()));
            }

            let mut response = self.response.lock().await;
            response.request_id = request.request_id;
            Ok(response.clone())
        }
    }

    #[tokio::test]
    async fn mock_specialist_returns_response() {
        let mock = MockSpecialist::new("technical", "technical", dec!(0.80));
        let request = AgentRequest {
            request_id: Uuid::new_v4(),
            proposal: tirds_models::TradeProposal {
                id: Uuid::new_v4(),
                schema_version: 1,
                symbol: "AAPL".to_string(),
                legs: vec![],
                proposed_at: chrono::Utc::now(),
                context: None,
            },
            domain_data: serde_json::json!({}),
            domain: "technical".to_string(),
        };

        let result = mock.evaluate(&request).await.unwrap();
        assert_eq!(result.agent_name, "technical");
        assert_eq!(result.confidence, dec!(0.80));
        assert_eq!(result.request_id, request.request_id);
    }

    #[tokio::test]
    async fn mock_specialist_failure() {
        let mock = MockSpecialist::failing("technical", "technical");
        let request = AgentRequest {
            request_id: Uuid::new_v4(),
            proposal: tirds_models::TradeProposal {
                id: Uuid::new_v4(),
                schema_version: 1,
                symbol: "AAPL".to_string(),
                legs: vec![],
                proposed_at: chrono::Utc::now(),
                context: None,
            },
            domain_data: serde_json::json!({}),
            domain: "technical".to_string(),
        };

        let result = mock.evaluate(&request).await;
        assert!(result.is_err());
    }
}
