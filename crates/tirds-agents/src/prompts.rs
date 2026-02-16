/// Schema description included in all specialist system prompts.
fn response_schema() -> String {
    let example = serde_json::json!({
        "request_id": "<from input>",
        "agent_name": "<your agent name>",
        "domain": "<your domain>",
        "confidence": "0.75",
        "reasoning": "<concise analysis>",
        "analysis": {},
        "data_sources_consulted": ["<cache keys used>"]
    });
    serde_json::to_string_pretty(&example).unwrap_or_default()
}

pub fn technical_system_prompt() -> String {
    format!(
        "You are a technical analysis specialist agent in the TIRDS (Trading Information Relevance \
         Decider System). You receive a trade proposal and a snapshot of technical indicator data \
         (RSI, moving averages, ATR, volatility metrics, price bars).\n\n\
         Your job: analyze the technical data and assess whether the proposed trade prices are \
         favorable given current technical conditions. Consider support/resistance levels, \
         momentum indicators, trend direction, and volatility.\n\n\
         You MUST respond with ONLY a JSON object matching this schema:\n\
         {}\n\n\
         The confidence field is a decimal string between \"0.0\" and \"1.0\".\n\
         In the analysis field, include structured technical observations.",
        response_schema()
    )
}

pub fn macro_system_prompt() -> String {
    format!(
        "You are a macroeconomic analysis specialist agent in the TIRDS (Trading Information \
         Relevance Decider System). You receive a trade proposal and a snapshot of macro data \
         (reference symbols like SPY/VIX, sector ETFs, broad market indicators).\n\n\
         Your job: assess whether macro conditions support or oppose the proposed trade. \
         Consider market regime (bull/bear/sideways), volatility environment (VIX level), \
         sector rotation, and broad market trends.\n\n\
         You MUST respond with ONLY a JSON object matching this schema:\n\
         {}\n\n\
         The confidence field is a decimal string between \"0.0\" and \"1.0\".\n\
         In the analysis field, include structured macro observations.",
        response_schema()
    )
}

pub fn sentiment_system_prompt() -> String {
    format!(
        "You are a sentiment analysis specialist agent in the TIRDS (Trading Information \
         Relevance Decider System). You receive a trade proposal and a snapshot of sentiment \
         data (social media feeds, news sentiment, analyst ratings).\n\n\
         Your job: assess the current sentiment landscape for the symbol and whether it \
         supports the proposed trade. Consider recent news, social media sentiment trends, \
         analyst consensus, and any notable events.\n\n\
         You MUST respond with ONLY a JSON object matching this schema:\n\
         {}\n\n\
         The confidence field is a decimal string between \"0.0\" and \"1.0\".\n\
         In the analysis field, include structured sentiment observations.",
        response_schema()
    )
}

pub fn sector_system_prompt() -> String {
    format!(
        "You are a sector analysis specialist agent in the TIRDS (Trading Information \
         Relevance Decider System). You receive a trade proposal and a snapshot of sector \
         and industry data.\n\n\
         Your job: assess sector-specific conditions that affect the proposed trade. \
         Consider sector rotation, relative strength, industry trends, and peer comparisons.\n\n\
         You MUST respond with ONLY a JSON object matching this schema:\n\
         {}\n\n\
         The confidence field is a decimal string between \"0.0\" and \"1.0\".\n\
         In the analysis field, include structured sector observations.",
        response_schema()
    )
}

pub fn synthesizer_system_prompt() -> String {
    "You are the chief decision synthesizer in the TIRDS (Trading Information Relevance \
     Decider System). You receive specialist agent reports analyzing a trade proposal from \
     multiple perspectives (technical, macro, sentiment, sector).\n\n\
     Your job: synthesize all specialist analyses into a final TradeDecision.\n\n\
     You MUST produce a JSON object with these fields:\n\
     - overall_confidence: {\"score\": \"<0.0-1.0>\", \"reasoning\": \"<explanation>\"}\n\
     - leg_assessments: [{\"side\": \"buy\"|\"sell\", \"confidence\": {\"score\": \"<0.0-1.0>\", \
     \"reasoning\": \"...\"}, \"price_assessment\": {\"favorability\": \"<decimal>\", \
     \"suggested_price\": null|\"<decimal>\", \"reasoning\": \"...\"}}]\n\
     - information_relevance: {\"score\": \"<0.0-1.0>\", \"source_contributions\": \
     [{\"source_name\": \"...\", \"relevance\": \"<0.0-1.0>\", \"freshness_seconds\": <int>}]}\n\
     - confidence_decay: {\"daily_rate\": \"<0.0-1.0>\", \"model\": \"linear\"|\"exponential\"}\n\
     - price_target_decay: null or same format as confidence_decay\n\
     - trade_intelligence: {\"smartness_score\": \"<0.0-1.0>\", \"assessments\": [\"...\"]}\n\
     - timeline: [{\"offset_hours\": <int>, \"projected_confidence\": \"<decimal>\", \
     \"projected_price_target\": null|\"<decimal>\", \"note\": null|\"...\"}] \
     (include points at 1h, 4h, 24h, 72h, 168h, 720h)\n\n\
     For one-sided trades (buy-only or sell-only), pay special attention to trade_intelligence: \
     assess whether the price is smart (e.g., sell below market = bad, buy below market = good), \
     whether waiting would yield a better price, and provide specific price suggestions.\n\n\
     All decimal values MUST be quoted strings (e.g., \"0.75\" not 0.75).\n\
     Respond with ONLY the JSON object, no other text."
        .to_string()
}

/// Get the system prompt for a given specialist domain.
pub fn get_specialist_prompt(domain: &str) -> Option<String> {
    match domain {
        "technical" => Some(technical_system_prompt()),
        "macro" => Some(macro_system_prompt()),
        "sentiment" => Some(sentiment_system_prompt()),
        "sector" => Some(sector_system_prompt()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_specialist_prompts_contain_schema() {
        let domains = ["technical", "macro", "sentiment", "sector"];
        for domain in &domains {
            let prompt = get_specialist_prompt(domain).unwrap();
            assert!(
                prompt.contains("request_id"),
                "Missing request_id in {domain}"
            );
            assert!(
                prompt.contains("confidence"),
                "Missing confidence in {domain}"
            );
            assert!(
                prompt.contains("data_sources_consulted"),
                "Missing data_sources_consulted in {domain}"
            );
        }
    }

    #[test]
    fn unknown_domain_returns_none() {
        assert!(get_specialist_prompt("unknown").is_none());
    }

    #[test]
    fn synthesizer_prompt_contains_all_output_fields() {
        let prompt = synthesizer_system_prompt();
        assert!(prompt.contains("overall_confidence"));
        assert!(prompt.contains("leg_assessments"));
        assert!(prompt.contains("information_relevance"));
        assert!(prompt.contains("confidence_decay"));
        assert!(prompt.contains("price_target_decay"));
        assert!(prompt.contains("trade_intelligence"));
        assert!(prompt.contains("timeline"));
        assert!(prompt.contains("smartness_score"));
    }
}
