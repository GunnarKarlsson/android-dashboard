use std::time::Duration;

use serde_json::{json, Value};

use crate::config::InsightConfig;
use crate::reduce::InsightSnapshot;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(45);
const TEMPERATURE: f64 = 0.2;
const MAX_TOKENS: u32 = 350;
const MAX_NEXT_CHECKS: u32 = 3;
const MAX_REPLY_WORDS: u32 = 120;

#[derive(Debug, thiserror::Error)]
pub enum InsightError {
    #[error("AI_PROVIDER_API_KEY is not set")]
    MissingApiKey,
    #[error("HTTP {status}: {body}")]
    Http { status: u16, body: String },
    #[error("{0}")]
    Transport(String),
    #[error("empty model reply")]
    EmptyReply,
}

/// POSTs the snapshot to the configured Chat Completions endpoint and returns the assistant text.
///
/// Logs host, model, snapshot JSON, and the response body. Does not log the API key.
pub fn complete(
    config: &InsightConfig,
    snapshot: &InsightSnapshot,
) -> Result<String, InsightError> {
    if !config.has_api_key() {
        tracing::warn!("insight request skipped: AI_PROVIDER_API_KEY is not set");
        return Err(InsightError::MissingApiKey);
    }

    let snapshot_json = snapshot
        .to_pretty_json()
        .map_err(|err| InsightError::Transport(err.to_string()))?;
    tracing::info!(
        host = %config.host_for_log(),
        model = %config.model,
        snapshot = %snapshot_json,
        "sending insight request"
    );

    let url = config.completions_url();
    let system = format!(
        "You are an Android runtime analyst inside a terminal HUD.\n\
Comment only on the supplied snapshot. Do not invent stack frames.\n\
Output:\n\
1) Verdict in one line: HEALTHY | DEGRADING | FAILING\n\
2) Top issues: what / evidence count / likely cause\n\
3) Next checks, max {MAX_NEXT_CHECKS} bullets\n\
Max {MAX_REPLY_WORDS} words. No preamble."
    );
    let body = json!({
        "model": config.model,
        "temperature": TEMPERATURE,
        "max_tokens": MAX_TOKENS,
        "stream": false,
        "thinking": { "type": "disabled" },
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": snapshot_json },
        ],
    });

    let agent = ureq::AgentBuilder::new().timeout(REQUEST_TIMEOUT).build();
    let result = agent
        .post(&url)
        .set(
            "Authorization",
            &format!("Bearer {}", config.api_key.trim()),
        )
        .set("Content-Type", "application/json")
        .send_json(body);

    match result {
        Ok(response) => {
            let status = response.status();
            let text = response
                .into_string()
                .map_err(|err| InsightError::Transport(err.to_string()))?;
            tracing::info!(status, body = %text, "received insight response");
            extract_assistant_text(&text)
        }
        Err(ureq::Error::Status(status, response)) => {
            let text = response.into_string().unwrap_or_default();
            tracing::error!(status, body = %text, "insight response error");
            Err(InsightError::Http { status, body: text })
        }
        Err(err) => {
            tracing::error!(error = %err, "insight request failed");
            Err(InsightError::Transport(err.to_string()))
        }
    }
}

fn extract_assistant_text(body: &str) -> Result<String, InsightError> {
    let value: Value =
        serde_json::from_str(body).map_err(|err| InsightError::Transport(err.to_string()))?;
    let content = value
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty());
    match content {
        Some(text) => Ok(text.to_string()),
        None => Err(InsightError::EmptyReply),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_assistant_text_from_openai_shape() {
        let body = r#"{
            "choices": [{ "message": { "content": "FAILING  disk full" } }]
        }"#;
        assert_eq!(extract_assistant_text(body).unwrap(), "FAILING  disk full");
    }
}
