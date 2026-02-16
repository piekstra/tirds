use std::time::Duration;
use tokio::process::Command;
use tracing::{debug, warn};

use crate::error::AgentError;

/// Configuration for a Claude CLI invocation.
#[derive(Debug, Clone)]
pub struct ClaudeCliConfig {
    pub model: String,
    pub timeout: Duration,
}

impl Default for ClaudeCliConfig {
    fn default() -> Self {
        Self {
            model: "claude-3-5-haiku-latest".to_string(),
            timeout: Duration::from_secs(45),
        }
    }
}

/// Invoke the `claude` CLI with a system prompt and user prompt.
/// Returns the raw stdout text.
pub async fn invoke_claude(
    system_prompt: &str,
    user_prompt: &str,
    config: &ClaudeCliConfig,
) -> Result<String, AgentError> {
    debug!(model = %config.model, "Invoking claude CLI");

    let result = tokio::time::timeout(config.timeout, async {
        Command::new("claude")
            .args([
                "-p",
                user_prompt,
                "--system-prompt",
                system_prompt,
                "--model",
                &config.model,
                "--output-format",
                "text",
            ])
            .output()
            .await
    })
    .await
    .map_err(|_| AgentError::Timeout(config.timeout.as_secs()))?
    .map_err(|e| AgentError::Cli(format!("Failed to spawn claude: {e}")))?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        warn!(status = %result.status, stderr = %stderr, "Claude CLI failed");
        return Err(AgentError::Cli(format!(
            "claude exited {}: {}",
            result.status, stderr
        )));
    }

    let stdout = String::from_utf8_lossy(&result.stdout).to_string();
    if stdout.trim().is_empty() {
        return Err(AgentError::Cli(
            "Claude returned empty response".to_string(),
        ));
    }

    Ok(stdout)
}

/// Check if the `claude` CLI is available on the system.
pub async fn check_cli_available() -> bool {
    match Command::new("claude").arg("--version").output().await {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = ClaudeCliConfig::default();
        assert_eq!(config.model, "claude-3-5-haiku-latest");
        assert_eq!(config.timeout, Duration::from_secs(45));
    }
}
