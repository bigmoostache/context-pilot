//! xAI Grok API implementation.
//!
//! Grok uses an OpenAI-compatible API format.

use std::env;
use std::io::{BufRead, BufReader};
use std::sync::mpsc::Sender;

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{LlmClient, LlmRequest, StreamEvent};
use crate::constants::{prompts, MAX_RESPONSE_TOKENS};
use crate::panels::ContextItem;
use crate::state::{Message, MessageStatus, MessageType};
use crate::tool_defs::ToolDefinition;
use crate::tools::ToolUse;

const GROK_API_ENDPOINT: &str = "https://api.x.ai/v1/chat/completions";

/// xAI Grok client
pub struct GrokClient {
    api_key: Option<String>,
}

impl GrokClient {
    pub fn new() -> Self {
        dotenvy::dotenv().ok();
        Self {
            api_key: env::var("XAI_API_KEY").ok(),
        }
    }
}

impl Default for GrokClient {
    fn default() -> Self {
        Self::new()
    }
}

// OpenAI-compatible message format
#[derive(Debug, Serialize)]
struct GrokMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<GrokToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GrokToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: GrokFunction,
}

#[derive(Debug, Serialize, Deserialize)]
struct GrokFunction {
    name: String,
    arguments: String,
}

#[derive(Debug, Serialize)]
struct GrokTool {
    #[serde(rename = "type")]
    tool_type: String,
    function: GrokFunctionDef,
}

#[derive(Debug, Serialize)]
struct GrokFunctionDef {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Debug, Serialize)]
struct GrokRequest {
    model: String,
    messages: Vec<GrokMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<GrokTool>,
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

impl LlmClient for GrokClient {
    fn stream(&self, request: LlmRequest, tx: Sender<StreamEvent>) -> Result<(), String> {
        let api_key = self
            .api_key
            .clone()
            .ok_or_else(|| "XAI_API_KEY not set".to_string())?;

        let client = Client::new();

        // Build messages in OpenAI format
        let mut grok_messages = messages_to_grok(
            &request.messages,
            &request.context_items,
            &request.system_prompt,
            &request.extra_context,
        );

        // Add tool results if present
        if let Some(results) = &request.tool_results {
            for result in results {
                grok_messages.push(GrokMessage {
                    role: "tool".to_string(),
                    content: Some(result.content.clone()),
                    tool_calls: None,
                    tool_call_id: Some(result.tool_use_id.clone()),
                });
            }
        }

        // Convert tools to OpenAI format
        let grok_tools = tools_to_grok(&request.tools);

        let api_request = GrokRequest {
            model: request.model.clone(),
            messages: grok_messages,
            tools: grok_tools,
            max_tokens: MAX_RESPONSE_TOKENS,
            stream: true,
        };

        let response = client
            .post(GROK_API_ENDPOINT)
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
                    if choice.finish_reason.is_some() {
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
        });
        Ok(())
    }
}

/// Convert internal messages to Grok/OpenAI format
fn messages_to_grok(
    messages: &[Message],
    context_items: &[ContextItem],
    system_prompt: &Option<String>,
    extra_context: &Option<String>,
) -> Vec<GrokMessage> {
    let mut grok_messages: Vec<GrokMessage> = Vec::new();

    // Add system message
    let system_content = system_prompt
        .clone()
        .unwrap_or_else(|| prompts::MAIN_SYSTEM.to_string());
    grok_messages.push(GrokMessage {
        role: "system".to_string(),
        content: Some(system_content),
        tool_calls: None,
        tool_call_id: None,
    });

    // Format context items
    let context_parts: Vec<String> = context_items
        .iter()
        .filter(|item| !item.content.is_empty())
        .map(|item| item.format())
        .collect();

    // Add extra context if present (for cleaner mode)
    if let Some(ctx) = extra_context {
        grok_messages.push(GrokMessage {
            role: "user".to_string(),
            content: Some(format!(
                "Please clean up the context to reduce token usage:\n\n{}",
                ctx
            )),
            tool_calls: None,
            tool_call_id: None,
        });
    }

    let mut first_user_message = true;

    for msg in messages.iter() {
        if msg.status == MessageStatus::Deleted {
            continue;
        }

        if msg.content.is_empty() && msg.tool_uses.is_empty() && msg.tool_results.is_empty() {
            continue;
        }

        // Handle tool results
        if msg.message_type == MessageType::ToolResult {
            for result in &msg.tool_results {
                grok_messages.push(GrokMessage {
                    role: "tool".to_string(),
                    content: Some(format!("[{}]: {}", msg.id, result.content)),
                    tool_calls: None,
                    tool_call_id: Some(result.tool_use_id.clone()),
                });
            }
            continue;
        }

        // Handle tool calls
        if msg.message_type == MessageType::ToolCall {
            let tool_calls: Vec<GrokToolCall> = msg
                .tool_uses
                .iter()
                .map(|tu| GrokToolCall {
                    id: tu.id.clone(),
                    call_type: "function".to_string(),
                    function: GrokFunction {
                        name: tu.name.clone(),
                        arguments: serde_json::to_string(&tu.input).unwrap_or_default(),
                    },
                })
                .collect();

            grok_messages.push(GrokMessage {
                role: "assistant".to_string(),
                content: None,
                tool_calls: Some(tool_calls),
                tool_call_id: None,
            });
            continue;
        }

        // Regular text message
        let message_content = match msg.status {
            MessageStatus::Summarized => msg.tl_dr.as_ref().unwrap_or(&msg.content).clone(),
            _ => msg.content.clone(),
        };

        if !message_content.is_empty() {
            let prefixed_content = format!("[{}]: {}", msg.id, message_content);

            let text = if msg.role == "user" && first_user_message && !context_parts.is_empty() {
                first_user_message = false;
                let context = context_parts.join("\n\n");
                format!("{}\n\n{}", context, prefixed_content)
            } else {
                if msg.role == "user" {
                    first_user_message = false;
                }
                prefixed_content
            };

            grok_messages.push(GrokMessage {
                role: msg.role.clone(),
                content: Some(text),
                tool_calls: None,
                tool_call_id: None,
            });
        }
    }

    grok_messages
}

/// Convert tool definitions to Grok/OpenAI format
fn tools_to_grok(tools: &[ToolDefinition]) -> Vec<GrokTool> {
    tools
        .iter()
        .filter(|t| t.enabled)
        .map(|t| GrokTool {
            tool_type: "function".to_string(),
            function: GrokFunctionDef {
                name: t.id.clone(),
                description: t.description.clone(),
                parameters: t.to_json_schema(),
            },
        })
        .collect()
}
