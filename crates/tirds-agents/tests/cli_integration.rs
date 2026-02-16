//! Integration tests that invoke the real Claude CLI.
//!
//! These tests are `#[ignore]` by default — they require:
//! - The `claude` CLI installed and on PATH
//! - Valid Anthropic credentials configured
//!
//! Run explicitly with:
//! ```bash
//! cargo test -p tirds-agents --test cli_integration -- --ignored
//! ```

use tirds_agents::claude_cli::{check_cli_available, invoke_claude, ClaudeCliConfig};
use tirds_agents::parser::extract_json;

use std::time::Duration;

/// Verify the Claude CLI is installed and responds to --version.
#[tokio::test]
#[ignore]
async fn cli_is_available() {
    assert!(
        check_cli_available().await,
        "claude CLI not found on PATH — install it from https://docs.anthropic.com/en/docs/claude-code"
    );
}

/// Invoke the Claude CLI with a trivial prompt and verify we get parseable JSON back.
///
/// This catches breaking changes in the CLI's output format (new wrapping,
/// changed response structure, etc.) that would otherwise only surface in production.
#[tokio::test]
#[ignore]
async fn cli_output_is_parseable_json() {
    if !check_cli_available().await {
        eprintln!("Skipping: claude CLI not available");
        return;
    }

    let config = ClaudeCliConfig {
        model: "claude-3-5-haiku-latest".to_string(),
        timeout: Duration::from_secs(30),
    };

    let system_prompt = concat!(
        "You are a test agent. Respond ONLY with a JSON object, no other text.\n",
        "The JSON must have exactly these fields:\n",
        "- \"status\": the string \"ok\"\n",
        "- \"echo\": repeat back the user's message exactly\n",
    );

    let user_prompt = "ping";

    let raw = invoke_claude(system_prompt, user_prompt, &config)
        .await
        .expect("Claude CLI invocation failed");

    // The parser should be able to extract valid JSON from whatever format the CLI returns
    let json_str = extract_json(&raw).expect(&format!(
        "Failed to extract JSON from CLI output.\n\
         This likely means the CLI output format has changed.\n\
         Raw output:\n---\n{raw}\n---"
    ));

    let parsed: serde_json::Value =
        serde_json::from_str(&json_str).expect("Extracted JSON is not valid");

    assert_eq!(
        parsed["status"], "ok",
        "Unexpected response structure: {parsed}"
    );
}

/// Verify that the CLI returns a non-zero exit code for an invalid model,
/// and that our error handling captures it correctly.
#[tokio::test]
#[ignore]
async fn cli_reports_errors_for_invalid_model() {
    if !check_cli_available().await {
        eprintln!("Skipping: claude CLI not available");
        return;
    }

    let config = ClaudeCliConfig {
        model: "nonexistent-model-12345".to_string(),
        timeout: Duration::from_secs(15),
    };

    let result = invoke_claude("You are a test.", "hello", &config).await;

    assert!(
        result.is_err(),
        "Expected error for invalid model, got: {:?}",
        result.unwrap()
    );
}
