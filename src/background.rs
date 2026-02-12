use std::env;
use std::sync::mpsc::Sender;
use std::thread;

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

use crate::constants::{MODEL_TLDR, MAX_TLDR_TOKENS, API_ENDPOINT, API_VERSION, prompts};
use crate::state::estimate_tokens;

/// Simple debug logging to file
fn log(msg: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("tldr_debug.log")
    {
        let _ = writeln!(f, "{}", msg);
    }
}

/// Event for TL;DR completion
#[derive(Debug)]
pub struct TlDrResult {
    pub message_id: String,
    pub tl_dr: String,
    pub token_count: usize,
}

/// Generate TL;DR for a message in the background
/// If content is less than 25 tokens, use it directly; otherwise, summarize via LLM
pub fn generate_tldr(message_id: String, content: String, tx: Sender<TlDrResult>) {
    thread::spawn(move || {
        let token_count = estimate_tokens(&content);
        log(&format!("Token count: {}", token_count));

        // If short enough, use content directly
        if token_count < prompts::tldr_min_tokens() {
            log("Using content directly (short message)");
            let result = tx.send(TlDrResult {
                message_id,
                tl_dr: content,
                token_count,
            });
            log(&format!("Send result: {:?}", result.is_ok()));
            return;
        }

        // Otherwise, ask LLM to summarize
        log("Calling LLM for summary...");
        match summarize_content(&content) {
            Ok(summary) => {
                log(&format!("Got summary: {}", &summary[..summary.len().min(50)]));
                let summary_tokens = estimate_tokens(&summary);
                let result = tx.send(TlDrResult {
                    message_id,
                    tl_dr: summary,
                    token_count: summary_tokens,
                });
                log(&format!("Send result: {:?}", result.is_ok()));
            }
            Err(e) => {
                log(&format!("LLM error: {}", e));
                // On error, truncate content as fallback
                let truncated: String = content.chars().take(100).collect();
                let truncated = if content.len() > 100 {
                    format!("{}...", truncated)
                } else {
                    truncated
                };
                let token_count = estimate_tokens(&truncated);
                let result = tx.send(TlDrResult {
                    message_id,
                    tl_dr: truncated,
                    token_count,
                });
                log(&format!("Fallback send result: {:?}", result.is_ok()));
            }
        }
    });
}

/// Call LLM to summarize content
fn summarize_content(content: &str) -> Result<String, String> {
    dotenvy::dotenv().ok();
    let api_key = env::var("ANTHROPIC_API_KEY")
        .map_err(|_| "ANTHROPIC_API_KEY not set".to_string())?;

    let client = Client::new();

    #[derive(Serialize)]
    struct SummaryRequest {
        model: String,
        max_tokens: u32,
        messages: Vec<SummaryMessage>,
    }

    #[derive(Serialize)]
    struct SummaryMessage {
        role: String,
        content: String,
    }

    #[derive(Deserialize)]
    struct SummaryResponse {
        content: Vec<SummaryContent>,
    }

    #[derive(Deserialize)]
    struct SummaryContent {
        #[serde(rename = "type")]
        _content_type: Option<String>,
        text: Option<String>,
    }

    let request = SummaryRequest {
        model: MODEL_TLDR.to_string(),
        max_tokens: MAX_TLDR_TOKENS,
        messages: vec![SummaryMessage {
            role: "user".to_string(),
            content: format!("{}{}", prompts::tldr_prompt(), content),
        }],
    };

    let response = client
        .post(API_ENDPOINT)
        .header("x-api-key", &api_key)
        .header("anthropic-version", API_VERSION)
        .header("content-type", "application/json")
        .json(&request)
        .send()
        .map_err(|e| format!("Request failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("API error: {}", response.status()));
    }

    let result: SummaryResponse = response
        .json()
        .map_err(|e| format!("Parse error: {}", e))?;

    result
        .content
        .first()
        .and_then(|c| c.text.clone())
        .ok_or_else(|| "No content in response".to_string())
}
