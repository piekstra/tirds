use crate::error::AgentError;

/// Extract the first JSON object from a string that may contain surrounding text.
///
/// Handles common Claude response formats:
/// - Clean JSON: `{"key": "value"}`
/// - Markdown-wrapped: ```json\n{"key": "value"}\n```
/// - Prefix text: `Here is the analysis:\n{"key": "value"}`
pub fn extract_json(text: &str) -> Result<String, AgentError> {
    let trimmed = text.trim();

    // Try parsing the whole thing as JSON first
    if trimmed.starts_with('{') && serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
        return Ok(trimmed.to_string());
    }

    // Try extracting from markdown code block
    if let Some(json_str) = extract_from_markdown_block(trimmed) {
        if serde_json::from_str::<serde_json::Value>(&json_str).is_ok() {
            return Ok(json_str);
        }
    }

    // Try finding the first { ... } pair using brace matching
    if let Some(json_str) = extract_first_object(trimmed) {
        if serde_json::from_str::<serde_json::Value>(&json_str).is_ok() {
            return Ok(json_str);
        }
    }

    Err(AgentError::Parse(format!(
        "No valid JSON object found in response (length={})",
        text.len()
    )))
}

/// Extract JSON from a markdown code block (```json ... ``` or ``` ... ```)
fn extract_from_markdown_block(text: &str) -> Option<String> {
    // Look for ```json or just ```
    let start_markers = ["```json\n", "```json\r\n", "```\n", "```\r\n"];

    for marker in &start_markers {
        if let Some(start) = text.find(marker) {
            let json_start = start + marker.len();
            if let Some(end) = text[json_start..].find("```") {
                let extracted = text[json_start..json_start + end].trim();
                return Some(extracted.to_string());
            }
        }
    }

    None
}

/// Find the first balanced { ... } in the text.
fn extract_first_object(text: &str) -> Option<String> {
    let mut depth = 0;
    let mut start = None;
    let mut in_string = false;
    let mut escape_next = false;

    for (i, ch) in text.char_indices() {
        if escape_next {
            escape_next = false;
            continue;
        }

        match ch {
            '\\' if in_string => {
                escape_next = true;
            }
            '"' => {
                in_string = !in_string;
            }
            '{' if !in_string => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            '}' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    if let Some(s) = start {
                        return Some(text[s..=i].to_string());
                    }
                }
            }
            _ => {}
        }
    }

    None
}

/// Parse an AgentResponse from raw Claude CLI output.
pub fn parse_agent_response(raw: &str) -> Result<tirds_models::AgentResponse, AgentError> {
    let json_str = extract_json(raw)?;
    serde_json::from_str(&json_str).map_err(|e| {
        AgentError::Parse(format!(
            "Failed to parse AgentResponse: {e}\nJSON: {json_str}"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_clean_json() {
        let input = r#"{"confidence": 0.75, "reasoning": "test"}"#;
        let result = extract_json(input).unwrap();
        assert_eq!(result, input);
    }

    #[test]
    fn extract_from_markdown() {
        let input = "Here is my analysis:\n```json\n{\"confidence\": 0.75}\n```\nDone.";
        let result = extract_json(input).unwrap();
        assert_eq!(result, r#"{"confidence": 0.75}"#);
    }

    #[test]
    fn extract_from_markdown_no_lang() {
        let input = "Result:\n```\n{\"confidence\": 0.75}\n```";
        let result = extract_json(input).unwrap();
        assert_eq!(result, r#"{"confidence": 0.75}"#);
    }

    #[test]
    fn extract_with_prefix_text() {
        let input = "Based on my analysis, here is the result:\n{\"confidence\": 0.75, \"reasoning\": \"bullish\"}";
        let result = extract_json(input).unwrap();
        assert!(result.contains("confidence"));
    }

    #[test]
    fn extract_nested_json() {
        let input = r#"{"outer": {"inner": "value"}, "list": [1, 2, 3]}"#;
        let result = extract_json(input).unwrap();
        assert_eq!(result, input);
    }

    #[test]
    fn extract_with_escaped_braces_in_strings() {
        let input = r#"{"reasoning": "price went from {low} to {high}", "confidence": 0.5}"#;
        let result = extract_json(input).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["confidence"], 0.5);
    }

    #[test]
    fn extract_no_json() {
        let input = "This is just plain text with no JSON at all.";
        let result = extract_json(input);
        assert!(result.is_err());
    }

    #[test]
    fn parse_full_agent_response() {
        let input = r#"```json
{
    "request_id": "550e8400-e29b-41d4-a716-446655440000",
    "agent_name": "technical",
    "domain": "technical",
    "confidence": "0.82",
    "reasoning": "RSI indicates oversold",
    "analysis": {"rsi": 32.5},
    "data_sources_consulted": ["rsi_14_AAPL"]
}
```"#;

        let response = parse_agent_response(input).unwrap();
        assert_eq!(response.agent_name, "technical");
        assert_eq!(response.domain, "technical");
    }
}
