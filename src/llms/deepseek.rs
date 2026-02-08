//! DeepSeek API implementation.
//!
//! DeepSeek uses an OpenAI-compatible API format.

use std::env;
use std::io::{BufRead, BufReader};
use std::sync::mpsc::Sender;

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{LlmClient, LlmRequest, StreamEvent, prepare_panel_messages, panel_header_text, panel_footer_text, panel_timestamp_text};
use crate::constants::{prompts, MAX_RESPONSE_TOKENS};
use crate::core::panels::{ContextItem, now_ms};
use crate::state::{Message, MessageStatus, MessageType};
use crate::tool_defs::ToolDefinition;
use crate::tools::ToolUse;

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

// OpenAI-compatible message format
#[derive(Debug, Serialize)]
struct DsMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    /// Required on all assistant messages when using deepseek-reasoner.
    /// Set to empty string for historical messages where we don't have the original reasoning.
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<DsToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct DsToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: DsFunction,
}

#[derive(Debug, Serialize, Deserialize)]
struct DsFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Serialize)]
struct DsTool {
    #[serde(rename = "type")]
    tool_type: String,
    function: DsFunctionDef,
}

#[derive(Debug, Serialize)]
struct DsFunctionDef {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Debug, Serialize)]
struct DsRequest {
    model: String,
    messages: Vec<DsMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<DsTool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<String>,
    max_tokens: u32,
    stream: bool,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: Option<StreamDelta>,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamDelta {
    content: Option<String>,
    tool_calls: Option<Vec<StreamToolCall>>,
}

#[derive(Debug, Deserialize)]
struct StreamToolCall {
    index: Option<usize>,
    id: Option<String>,
    function: Option<StreamFunction>,
}

#[derive(Debug, Deserialize)]
struct StreamFunction {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamResponse {
    choices: Vec<StreamChoice>,
    usage: Option<StreamUsage>,
}

#[derive(Debug, Deserialize)]
struct StreamUsage {
    prompt_tokens: Option<usize>,
    completion_tokens: Option<usize>,
}

impl LlmClient for DeepSeekClient {
    fn stream(&self, request: LlmRequest, tx: Sender<StreamEvent>) -> Result<(), String> {
        let api_key = self
            .api_key
            .clone()
            .ok_or_else(|| "DEEPSEEK_API_KEY not set".to_string())?;

        let client = Client::new();

        // Collect pending tool result IDs so the message builder includes their tool calls
        let pending_tool_ids: Vec<String> = request.tool_results.as_ref()
            .map(|results| results.iter().map(|r| r.tool_use_id.clone()).collect())
            .unwrap_or_default();

        // Build messages in OpenAI format
        let mut ds_messages = messages_to_ds(
            &request.messages,
            &request.context_items,
            &request.system_prompt,
            &request.extra_context,
            &pending_tool_ids,
            &request.model,
        );

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

        // Convert tools to OpenAI format
        let ds_tools = tools_to_ds(&request.tools);

        // Set tool_choice to "auto" when tools are available
        let tool_choice = if ds_tools.is_empty() {
            None
        } else {
            Some("auto".to_string())
        };

        let api_request = DsRequest {
            model: request.model.clone(),
            messages: ds_messages,
            tools: ds_tools,
            tool_choice,
            max_tokens: MAX_RESPONSE_TOKENS,
            stream: true,
        };

        // Dump last request for debugging
        dump_last_request(&request.worker_id, &api_request);

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

        let reader = BufReader::new(response);
        let mut input_tokens = 0;
        let mut output_tokens = 0;
        let mut stop_reason: Option<String> = None;

        // Track tool calls being built (index -> (id, name, arguments))
        let mut tool_calls: std::collections::HashMap<usize, (String, String, String)> =
            std::collections::HashMap::new();

        for line in reader.lines() {
            let line = line.map_err(|e| format!("Read error: {}", e))?;

            if !line.starts_with("data: ") {
                continue;
            }

            let json_str = &line[6..];
            if json_str == "[DONE]" {
                break;
            }

            if let Ok(resp) = serde_json::from_str::<StreamResponse>(json_str) {
                // Handle usage info
                if let Some(usage) = resp.usage {
                    if let Some(inp) = usage.prompt_tokens {
                        input_tokens = inp;
                    }
                    if let Some(out) = usage.completion_tokens {
                        output_tokens = out;
                    }
                }

                for choice in resp.choices {
                    if let Some(delta) = choice.delta {
                        // Handle text content
                        if let Some(content) = delta.content {
                            if !content.is_empty() {
                                let _ = tx.send(StreamEvent::Chunk(content));
                            }
                        }

                        // Handle tool calls
                        if let Some(calls) = delta.tool_calls {
                            for call in calls {
                                let idx = call.index.unwrap_or(0);

                                // Initialize or update tool call
                                let entry = tool_calls.entry(idx).or_insert_with(|| {
                                    (String::new(), String::new(), String::new())
                                });

                                if let Some(id) = call.id {
                                    entry.0 = id;
                                }
                                if let Some(func) = call.function {
                                    if let Some(name) = func.name {
                                        entry.1 = name;
                                    }
                                    if let Some(args) = func.arguments {
                                        entry.2.push_str(&args);
                                    }
                                }
                            }
                        }
                    }

                    // Check for finish reason
                    if let Some(ref reason) = choice.finish_reason {
                        stop_reason = Some(match reason.as_str() {
                            "length" => "max_tokens".to_string(),
                            "stop" => "end_turn".to_string(),
                            "tool_calls" => "tool_use".to_string(),
                            other => other.to_string(),
                        });
                        // Emit any completed tool calls
                        for (_, (id, name, arguments)) in tool_calls.drain() {
                            if !id.is_empty() && !name.is_empty() {
                                let input: Value = serde_json::from_str(&arguments)
                                    .unwrap_or_else(|_| Value::Object(serde_json::Map::new()));
                                let _ = tx.send(StreamEvent::ToolUse(ToolUse { id, name, input }));
                            }
                        }
                    }
                }
            }
        }

        let _ = tx.send(StreamEvent::Done {
            input_tokens,
            output_tokens,
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

/// Convert internal messages to DeepSeek/OpenAI format
fn messages_to_ds(
    messages: &[Message],
    context_items: &[ContextItem],
    system_prompt: &Option<String>,
    extra_context: &Option<String>,
    pending_tool_result_ids: &[String],
    model: &str,
) -> Vec<DsMessage> {
    let mut ds_messages: Vec<DsMessage> = Vec::new();
    let is_reasoner = model == "deepseek-reasoner";

    // Helper: reasoning_content for assistant messages (required for deepseek-reasoner)
    let rc = || -> Option<String> {
        if is_reasoner { Some(String::new()) } else { None }
    };

    // Add system message
    let system_content = system_prompt
        .clone()
        .unwrap_or_else(|| prompts::main_system().to_string());
    ds_messages.push(DsMessage {
        role: "system".to_string(),
        content: Some(system_content),
        reasoning_content: None,
        tool_calls: None,
        tool_call_id: None,
    });

    // Inject context panels as fake tool call/result pairs (P2+ only, sorted by timestamp)
    let fake_panels = prepare_panel_messages(context_items);
    let current_ms = now_ms();

    if !fake_panels.is_empty() {
        for (idx, panel) in fake_panels.iter().enumerate() {
            let timestamp_text = panel_timestamp_text(panel.timestamp_ms, current_ms);
            let text = if idx == 0 {
                format!("{}\n\n{}", panel_header_text(), timestamp_text)
            } else {
                timestamp_text
            };

            // Assistant message with tool_call
            ds_messages.push(DsMessage {
                role: "assistant".to_string(),
                content: Some(text),
                reasoning_content: rc(),
                tool_calls: Some(vec![DsToolCall {
                    id: format!("panel_{}", panel.panel_id),
                    call_type: "function".to_string(),
                    function: DsFunction {
                        name: "dynamic_panel".to_string(),
                        arguments: format!(r#"{{"id":"{}"}}"#, panel.panel_id),
                    },
                }]),
                tool_call_id: None,
            });

            // Tool result message
            ds_messages.push(DsMessage {
                role: "tool".to_string(),
                content: Some(panel.content.clone()),
                reasoning_content: None,
                tool_calls: None,
                tool_call_id: Some(format!("panel_{}", panel.panel_id)),
            });
        }

        // Add footer after all panels
        let footer = panel_footer_text(messages, current_ms);
        ds_messages.push(DsMessage {
            role: "assistant".to_string(),
            content: Some(footer),
            reasoning_content: rc(),
            tool_calls: Some(vec![DsToolCall {
                id: "panel_footer".to_string(),
                call_type: "function".to_string(),
                function: DsFunction {
                    name: "dynamic_panel".to_string(),
                    arguments: r#"{"action":"end_panels"}"#.to_string(),
                },
            }]),
            tool_call_id: None,
        });
        ds_messages.push(DsMessage {
            role: "tool".to_string(),
            content: Some(crate::constants::prompts::panel_footer_ack().to_string()),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: Some("panel_footer".to_string()),
        });
    }

    // Add extra context if present (for cleaner mode)
    if let Some(ctx) = extra_context {
        ds_messages.push(DsMessage {
            role: "user".to_string(),
            content: Some(format!(
                "Please clean up the context to reduce token usage:\n\n{}",
                ctx
            )),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        });
    }

    // First pass: collect tool_use IDs that have matching results.
    // Tool calls without results (e.g. truncated by max_tokens) are excluded
    // to avoid the "insufficient tool messages" API error.
    // Seed with pending tool result IDs (from current tool loop, not yet in messages).
    let mut included_tool_use_ids: std::collections::HashSet<String> =
        pending_tool_result_ids.iter().cloned().collect();
    for (idx, msg) in messages.iter().enumerate() {
        if msg.status == MessageStatus::Deleted || msg.message_type != MessageType::ToolCall {
            continue;
        }
        let tool_use_ids: Vec<&str> = msg.tool_uses.iter().map(|t| t.id.as_str()).collect();
        let has_result = messages[idx + 1..]
            .iter()
            .filter(|m| m.status != MessageStatus::Deleted && m.message_type == MessageType::ToolResult)
            .any(|m| m.tool_results.iter().any(|r| tool_use_ids.contains(&r.tool_use_id.as_str())));
        if has_result {
            for id in tool_use_ids {
                included_tool_use_ids.insert(id.to_string());
            }
        }
    }

    for msg in messages.iter() {
        if msg.status == MessageStatus::Deleted {
            continue;
        }

        if msg.content.is_empty() && msg.tool_uses.is_empty() && msg.tool_results.is_empty() {
            continue;
        }

        // Handle tool results — only include if the tool_use was included
        if msg.message_type == MessageType::ToolResult {
            for result in &msg.tool_results {
                if included_tool_use_ids.contains(&result.tool_use_id) {
                    ds_messages.push(DsMessage {
                        role: "tool".to_string(),
                        content: Some(format!("[{}]:\n{}", msg.id, result.content)),
                        reasoning_content: None,
                        tool_calls: None,
                        tool_call_id: Some(result.tool_use_id.clone()),
                    });
                }
            }
            continue;
        }

        // Handle tool calls — only include if they have matching results
        if msg.message_type == MessageType::ToolCall {
            let tool_calls: Vec<DsToolCall> = msg
                .tool_uses
                .iter()
                .filter(|tu| included_tool_use_ids.contains(&tu.id))
                .map(|tu| DsToolCall {
                    id: tu.id.clone(),
                    call_type: "function".to_string(),
                    function: DsFunction {
                        name: tu.name.clone(),
                        arguments: serde_json::to_string(&tu.input).unwrap_or_default(),
                    },
                })
                .collect();

            if !tool_calls.is_empty() {
                ds_messages.push(DsMessage {
                    role: "assistant".to_string(),
                    content: None,
                    reasoning_content: rc(),
                    tool_calls: Some(tool_calls),
                    tool_call_id: None,
                });
            }
            continue;
        }

        // Regular text message
        let message_content = match msg.status {
            MessageStatus::Summarized => msg.tl_dr.as_ref().unwrap_or(&msg.content).clone(),
            _ => msg.content.clone(),
        };

        if !message_content.is_empty() {
            let prefixed_content = format!("[{}]:\n{}", msg.id, message_content);
            // reasoning_content is only needed on assistant messages for deepseek-reasoner
            let msg_rc = if msg.role == "assistant" { rc() } else { None };

            ds_messages.push(DsMessage {
                role: msg.role.clone(),
                content: Some(prefixed_content),
                reasoning_content: msg_rc,
                tool_calls: None,
                tool_call_id: None,
            });
        }
    }

    ds_messages
}

/// Dump the outgoing API request to disk for debugging.
fn dump_last_request(worker_id: &str, api_request: &DsRequest) {
    let dir = ".context-pilot/last_requests";
    let _ = std::fs::create_dir_all(dir);
    let path = format!("{}/{}_deepseek_last_request.json", dir, worker_id);
    let _ = std::fs::write(path, serde_json::to_string_pretty(api_request).unwrap_or_default());
}

/// Convert tool definitions to DeepSeek/OpenAI format
fn tools_to_ds(tools: &[ToolDefinition]) -> Vec<DsTool> {
    tools
        .iter()
        .filter(|t| t.enabled)
        .map(|t| DsTool {
            tool_type: "function".to_string(),
            function: DsFunctionDef {
                name: t.id.clone(),
                description: t.description.clone(),
                parameters: t.to_json_schema(),
            },
        })
        .collect()
}
