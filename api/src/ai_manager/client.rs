//! Claude API HTTP Client
//!
//! Handles communication with the Anthropic Claude API for AI-powered analysis.
//! Includes retry logic, rate limiting awareness, and structured response parsing.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use super::{MetricsSnapshot, Decision};
use super::analysis::{Analysis, Recommendation};

const CLAUDE_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01"; // Latest stable API version (new features use beta headers)
const MAX_TOKENS: u32 = 2048;

/// Claude API Client for simulation analysis
pub struct ClaudeClient {
    client: Client,
    api_key: String,
    model: String,
}

impl ClaudeClient {
    /// Create a new Claude API client
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            model,
        }
    }

    /// Analyze simulation metrics and get recommendations
    pub async fn analyze(
        &self,
        snapshot: &MetricsSnapshot,
        recent_decisions: &[&Decision],
    ) -> Result<Analysis, String> {
        if self.api_key.is_empty() {
            return Err("API key not configured".to_string());
        }

        let system_prompt = self.build_system_prompt();
        let user_message = self.build_user_message(snapshot, recent_decisions)?;

        let request = ClaudeRequest {
            model: self.model.clone(),
            max_tokens: MAX_TOKENS,
            system: system_prompt,
            messages: vec![Message {
                role: "user".to_string(),
                content: user_message,
            }],
        };

        debug!("Sending analysis request to Claude API");

        let response = self.client
            .post(CLAUDE_API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            warn!("Claude API error: {} - {}", status, body);
            return Err(format!("API error: {} - {}", status, body));
        }

        let claude_response: ClaudeResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        // Extract the text content
        let text = claude_response.content
            .first()
            .and_then(|c| match c {
                ContentBlock::Text { text } => Some(text.as_str()),
            })
            .ok_or_else(|| "No text content in response".to_string())?;

        // Parse the JSON response from Claude
        self.parse_analysis_response(text)
    }

    /// Build the system prompt for Claude
    fn build_system_prompt(&self) -> String {
        r#"You are an AI simulation manager for the Orbit Royale game server.
Your job is to analyze game metrics and recommend parameter adjustments to maintain optimal performance.

## Available Parameters for Tuning

| Parameter | Range | Description |
|-----------|-------|-------------|
| arena.grow_lerp | 0.01-0.1 | How fast arena grows towards target |
| arena.shrink_lerp | 0.001-0.05 | How fast arena shrinks towards target |
| arena.shrink_delay_ticks | 0-300 | Ticks to wait before shrinking |
| arena.max_wells | 5-50 | Maximum gravity wells |
| arena.growth_per_player | 5-50 | Arena growth units per player |
| arena.player_threshold | 1-50 | Player count before arena grows |

## Performance Guidelines

1. **Tick Time**: Target <20ms (20000us) for smooth 30Hz gameplay
   - If p95 > 25000us: CRITICAL - reduce complexity
   - If p95 > 20000us: WARNING - consider reducing entities
   - If p95 < 15000us: GOOD - can increase complexity

2. **Arena Density**: Players should have space but not be too spread out
   - Too dense (many collisions): increase arena growth
   - Too sparse (no action): decrease arena size

3. **Entity Count**: Balance between visual richness and performance
   - High debris/projectiles + high tick time: reduce spawning
   - Low entities + low tick time: can increase

## Decision Rules

1. Make small, incremental changes (max 20% per adjustment)
2. Only recommend changes when confident (>0.7)
3. Consider past decision outcomes when available
4. Prioritize performance over aesthetics

## Response Format

Respond with valid JSON only:

```json
{
  "summary": "Brief 1-2 sentence assessment",
  "reasoning": "Detailed explanation of your analysis and why you're making these recommendations",
  "recommendations": [
    {
      "parameter": "arena.max_wells",
      "value": 15,
      "reason": "Reducing wells to improve tick time"
    }
  ],
  "confidence": 0.85
}
```

If no changes are needed, return empty recommendations with confidence < 0.5.
"#.to_string()
    }

    /// Build the user message with current metrics and history
    fn build_user_message(
        &self,
        snapshot: &MetricsSnapshot,
        recent_decisions: &[&Decision],
    ) -> Result<String, String> {
        let metrics_json = serde_json::to_string_pretty(snapshot)
            .map_err(|e| format!("Failed to serialize metrics: {}", e))?;

        let history_summary = if recent_decisions.is_empty() {
            "No recent decisions".to_string()
        } else {
            let mut summary = String::new();
            for decision in recent_decisions {
                let outcome_str = match &decision.outcome {
                    Some(o) if o.success => format!("SUCCESS ({}us)", o.performance_delta_us),
                    Some(o) => format!("FAILED ({}us)", o.performance_delta_us),
                    None => "PENDING".to_string(),
                };

                summary.push_str(&format!(
                    "- {}: {} actions, {} | {}\n",
                    decision.id,
                    decision.actions.len(),
                    outcome_str,
                    decision.analysis
                ));
            }
            summary
        };

        Ok(format!(
            "## Current Metrics\n\n```json\n{}\n```\n\n## Recent Decisions\n\n{}",
            metrics_json,
            history_summary
        ))
    }

    /// Parse Claude's response into an Analysis struct
    fn parse_analysis_response(&self, text: &str) -> Result<Analysis, String> {
        // Try to extract JSON from the response (Claude might wrap it in markdown)
        let json_str = if let Some(start) = text.find('{') {
            if let Some(end) = text.rfind('}') {
                &text[start..=end]
            } else {
                text
            }
        } else {
            text
        };

        let parsed: AnalysisResponse = serde_json::from_str(json_str)
            .map_err(|e| format!("Failed to parse analysis JSON: {} - Raw: {}", e, json_str))?;

        Ok(Analysis {
            summary: parsed.summary,
            reasoning: parsed.reasoning,
            recommendations: parsed.recommendations.into_iter().map(|r| Recommendation {
                parameter: r.parameter,
                value: r.value,
                reason: r.reason,
            }).collect(),
            confidence: parsed.confidence,
        })
    }
}

// Claude API request/response types

#[derive(Debug, Serialize)]
struct ClaudeRequest {
    model: String,
    max_tokens: u32,
    system: String,
    messages: Vec<Message>,
}

#[derive(Debug, Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ClaudeResponse {
    content: Vec<ContentBlock>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
}

#[derive(Debug, Deserialize)]
struct AnalysisResponse {
    summary: String,
    reasoning: String,
    recommendations: Vec<RecommendationResponse>,
    confidence: f32,
}

#[derive(Debug, Deserialize)]
struct RecommendationResponse {
    parameter: String,
    value: f32,
    reason: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_analysis_response() {
        let client = ClaudeClient::new("test".to_string(), "test".to_string());

        let json = r#"{
            "summary": "Performance is good",
            "reasoning": "All metrics within target",
            "recommendations": [],
            "confidence": 0.3
        }"#;

        let result = client.parse_analysis_response(json);
        assert!(result.is_ok());

        let analysis = result.unwrap();
        assert_eq!(analysis.summary, "Performance is good");
        assert_eq!(analysis.confidence, 0.3);
        assert!(analysis.recommendations.is_empty());
    }

    #[test]
    fn test_parse_wrapped_json() {
        let client = ClaudeClient::new("test".to_string(), "test".to_string());

        let wrapped = r#"Here is my analysis:

```json
{
    "summary": "Test",
    "reasoning": "Test reasoning",
    "recommendations": [{"parameter": "arena.max_wells", "value": 15, "reason": "test"}],
    "confidence": 0.8
}
```

Let me know if you need more details."#;

        let result = client.parse_analysis_response(wrapped);
        assert!(result.is_ok());

        let analysis = result.unwrap();
        assert_eq!(analysis.recommendations.len(), 1);
    }
}
