//! DeepSeek API implementation.
//!
//! DeepSeek uses an OpenAI-compatible API format.
//! Message building is delegated to the shared `openai_compat` module,
//! with a thin wrapper to add `reasoning_content` for deepseek-reasoner.

use std::env;
use std::io::{BufRead, BufReader};
use std::sync::mpsc::Sender;

use reqwest::blocking::Client;
use serde::Serialize;

use super::openai_compat::{self, OaiMessage, BuildOptions, ToolCallAccumulator};
use super::{LlmClient, LlmRequest, StreamEvent};

const DEEPSEEK_API_ENDPOINT: &str = "https://api.deepseek.com/chat/completions";

/// DeepSeek client
pub struct DeepSeekClient {
    api_key: Option<String>,
}

impl DeepSeekClient {
    pub fn new() -> Self {
        dotenvy::dotenv().ok();
        Self {
            api_key: env::var("DEEPSEEK_API_KEY").ok(),
        }
    }
}

impl Default for DeepSeekClient {
    fn default() -> Self {
        Self::new()
    }
}

// ───────────────────────────────────────────────────────────────────
// DeepSeek-specific message type (adds reasoning_content field)
// ───────────────────────────────────────────────────────────────────

/// DeepSeek message — wraps the shared OaiMessage but adds `reasoning_content`
/// which is required for deepseek-reasoner model on assistant messages.
#[derive(Debug, Serialize)]
struct DsMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<openai_compat::OaiToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

impl DsMessage {
    /// Convert from shared OaiMessage, adding reasoning_content for assistant messages.
    fn from_oai(msg: OaiMessage, is_reasoner: bool) -> Self {
        let reasoning_content = if is_reasoner && msg.role == "assistant" {
            Some(String::new())
        } else {
            None
        };
        Self {
            role: msg.role,
            content: msg.content,
            reasoning_content,
            tool_calls: msg.tool_calls,
            tool_call_id: msg.tool_call_id,
        }
    }
}

#[derive(Debug, Serialize)]
struct DsRequest {
    model: String,
    messages: Vec<DsMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<openai_compat::OaiTool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>,
    max_tokens: u32,
    stream: bool,
}

impl LlmClient for DeepSeekClient {
    fn stream(&self, request: LlmRequest, tx: Sender<StreamEvent>) -> Result<(), String> {
        let api_key = self
            .api_key
            .clone()
            .ok_or_else(|| "DEEPSEEK_API_KEY not set".to_string())?;

        let client = Client::new();
        let is_reasoner = request.model == "deepseek-reasoner";

        // Collect pending tool result IDs
        let pending_tool_ids: Vec<String> = request.tool_results.as_ref()
            .map(|results| results.iter().map(|r| r.tool_use_id.clone()).collect())
            .unwrap_or_default();

        // Build messages using shared builder
        let oai_messages = openai_compat::build_messages(
            &request.messages,
            &request.context_items,
            &BuildOptions {
                system_prompt: request.system_prompt.clone(),
                system_suffix: None,
                extra_context: request.extra_context.clone(),
                pending_tool_result_ids: pending_tool_ids,
            },
        );

        // Convert to DeepSeek format (adds reasoning_content for assistant messages)
        let mut ds_messages: Vec<DsMessage> = oai_messages
            .into_iter()
            .map(|m| DsMessage::from_oai(m, is_reasoner))
            .collect();

        // Add tool results if present
        if let Some(results) = &request.tool_results {
            for result in results {
                ds_messages.push(DsMessage {
                    role: "tool".to_string(),
                    content: Some(result.content.clone()),
                    reasoning_content: None,
                    tool_calls: None,
                    tool_call_id: Some(result.tool_use_id.clone()),
                });
            }
        }

        let tools = openai_compat::tools_to_oai(&request.tools);
        let tool_choice = if tools.is_empty() { None } else { Some("auto".to_string()) };

        let api_request = DsRequest {
            model: request.model.clone(),
            messages: ds_messages,
            tools,
            tool_choice,
            max_tokens: if is_reasoner { 16384 } else { 8192 },
            stream: true,
        };

        openai_compat::dump_request(&request.worker_id, "deepseek", &api_request);

        let response = client
            .post(DEEPSEEK_API_ENDPOINT)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&api_request)
            .send()
            .map_err(|e| format!("Request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            return Err(format!("API error {}: {}", status, body));
        }

        // Stream SSE using shared helpers
        let reader = BufReader::new(response);
        let mut input_tokens = 0;
        let mut output_tokens = 0;
        let mut cache_hit_tokens = 0;
        let mut cache_miss_tokens = 0;
        let mut stop_reason: Option<String> = None;
        let mut tool_acc = ToolCallAccumulator::new();

        for line in reader.lines() {
            let line = line.map_err(|e| format!("Read error: {}", e))?;

            if let Some(resp) = openai_compat::parse_sse_line(&line) {
                if let Some(usage) = resp.usage {
                    if let Some(inp) = usage.prompt_tokens { input_tokens = inp; }
                    if let Some(out) = usage.completion_tokens { output_tokens = out; }
                    // DeepSeek-specific cache fields
                    if let Some(hit) = usage.prompt_cache_hit_tokens { cache_hit_tokens = hit; }
                    if let Some(miss) = usage.prompt_cache_miss_tokens { cache_miss_tokens = miss; }
                }

                for choice in resp.choices {
                    if let Some(delta) = choice.delta {
                        if let Some(content) = delta.content {
                            if !content.is_empty() {
                                let _ = tx.send(StreamEvent::Chunk(content));
                            }
                        }
                        if let Some(calls) = delta.tool_calls {
                            for call in &calls {
                                tool_acc.feed(call);
                            }
                        }
                    }
                    if let Some(ref reason) = choice.finish_reason {
                        stop_reason = Some(openai_compat::normalize_stop_reason(reason));
                        for tool_use in tool_acc.drain() {
                            let _ = tx.send(StreamEvent::ToolUse(tool_use));
                        }
                    }
                }
            }
        }

        let _ = tx.send(StreamEvent::Done {
            input_tokens,
            output_tokens,
            cache_hit_tokens,
            cache_miss_tokens,
            stop_reason,
        });
        Ok(())
    }

    fn check_api(&self, model: &str) -> super::ApiCheckResult {
        let api_key = match &self.api_key {
            Some(k) => k.clone(),
            None => {
                return super::ApiCheckResult {
                    auth_ok: false,
                    streaming_ok: false,
                    tools_ok: false,
                    error: Some("DEEPSEEK_API_KEY not set".to_string()),
                }
            }
        };

        let client = Client::new();

        // Test 1: Basic auth
        let auth_result = client
            .post(DEEPSEEK_API_ENDPOINT)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "model": model,
                "max_tokens": 10,
                "messages": [{"role": "user", "content": "Hi"}]
            }))
            .send();

        let auth_ok = auth_result.as_ref().map(|r| r.status().is_success()).unwrap_or(false);

        if !auth_ok {
            let error = auth_result
                .err()
                .map(|e| e.to_string())
                .or_else(|| Some("Auth failed".to_string()));
            return super::ApiCheckResult {
                auth_ok: false,
                streaming_ok: false,
                tools_ok: false,
                error,
            };
        }

        // Test 2: Streaming
        let stream_result = client
            .post(DEEPSEEK_API_ENDPOINT)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "model": model,
                "max_tokens": 10,
                "stream": true,
                "messages": [{"role": "user", "content": "Say ok"}]
            }))
            .send();

        let streaming_ok = stream_result.as_ref().map(|r| r.status().is_success()).unwrap_or(false);

        // Test 3: Tools
        let tools_result = client
            .post(DEEPSEEK_API_ENDPOINT)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "model": model,
                "max_tokens": 50,
                "tools": [{
                    "type": "function",
                    "function": {
                        "name": "test_tool",
                        "description": "A test tool",
                        "parameters": {
                            "type": "object",
                            "properties": {},
                            "required": []
                        }
                    }
                }],
                "messages": [{"role": "user", "content": "Hi"}]
            }))
            .send();

        let tools_ok = tools_result.as_ref().map(|r| r.status().is_success()).unwrap_or(false);

        super::ApiCheckResult {
            auth_ok,
            streaming_ok,
            tools_ok,
            error: None,
        }
    }
}
