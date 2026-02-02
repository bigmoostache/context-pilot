use std::env;
use std::io::{BufRead, BufReader};
use std::sync::mpsc::Sender;
use std::thread;

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::constants::{MODEL_MAIN, MAX_RESPONSE_TOKENS, API_ENDPOINT, API_VERSION, prompts};
use crate::panels::ContextItem;
use crate::state::{Message, MessageStatus, MessageType};
use crate::tool_defs::{ToolDefinition, build_api_tools};
use crate::tools::{ToolResult, ToolUse};

#[derive(Debug)]
pub enum StreamEvent {
    Chunk(String),
    ToolUse(ToolUse),
    Done { input_tokens: usize, output_tokens: usize },
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse { id: String, name: String, input: Value },
    #[serde(rename = "tool_result")]
    ToolResult { tool_use_id: String, content: String },
}

#[derive(Debug, Serialize)]
struct ApiMessage {
    role: String,
    content: Vec<ContentBlock>,
}

#[derive(Debug, Serialize)]
struct ApiRequest {
    model: String,
    max_tokens: u32,
    system: String,
    messages: Vec<ApiMessage>,
    tools: Value,
    stream: bool,
}

#[derive(Debug, Deserialize)]
struct StreamContentBlock {
    #[serde(rename = "type")]
    block_type: Option<String>,
    id: Option<String>,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamDelta {
    #[serde(rename = "type")]
    delta_type: Option<String>,
    text: Option<String>,
    partial_json: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamMessage {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    _index: Option<usize>,
    content_block: Option<StreamContentBlock>,
    delta: Option<StreamDelta>,
    usage: Option<StreamUsage>,
}

#[derive(Debug, Deserialize)]
struct StreamUsage {
    input_tokens: Option<usize>,
    output_tokens: Option<usize>,
}

/// Converts our messages to API format, prepending context items to first user message
/// If include_last_tool_uses is true, tool_use blocks from the last assistant message are included
fn messages_to_api(
    messages: &[Message],
    context_items: &[ContextItem],
    include_last_tool_uses: bool,
) -> Vec<ApiMessage> {
    let mut api_messages: Vec<ApiMessage> = Vec::new();

    // Format all context items (include empty todos/memories so LLM knows the tools exist)
    let context_parts: Vec<String> = context_items
        .iter()
        .filter(|item| !item.content.is_empty())
        .map(|item| item.format())
        .collect();

    for (idx, msg) in messages.iter().enumerate() {
        // Skip deleted messages entirely
        if msg.status == MessageStatus::Deleted {
            continue;
        }

        if msg.content.is_empty() && msg.tool_uses.is_empty() && msg.tool_results.is_empty() {
            continue;
        }

        let mut content_blocks: Vec<ContentBlock> = Vec::new();

        // Handle ToolResult messages - these go as user messages with tool_result blocks
        if msg.message_type == MessageType::ToolResult {
            for result in &msg.tool_results {
                // Include message ID in result content so AI can reference it
                let prefixed_content = format!("[{}]: {}", msg.id, result.content);
                content_blocks.push(ContentBlock::ToolResult {
                    tool_use_id: result.tool_use_id.clone(),
                    content: prefixed_content,
                });
            }

            if !content_blocks.is_empty() {
                api_messages.push(ApiMessage {
                    role: "user".to_string(),
                    content: content_blocks,
                });
            }
            continue;
        }

        // Handle tool call messages - include if there's a matching non-deleted ToolResult after them
        if msg.message_type == MessageType::ToolCall {
            // Get tool_use_ids from this tool call
            let tool_use_ids: Vec<&str> = msg.tool_uses.iter().map(|t| t.id.as_str()).collect();

            // Check if there's a non-deleted ToolResult message after this that matches these tool_use_ids
            let has_matching_tool_result = messages[idx + 1..].iter()
                .filter(|m| m.status != MessageStatus::Deleted)
                .filter(|m| m.message_type == MessageType::ToolResult)
                .any(|m| m.tool_results.iter().any(|r| tool_use_ids.contains(&r.tool_use_id.as_str())));

            if has_matching_tool_result {
                for tool_use in &msg.tool_uses {
                    // Ensure input is always an object, never null
                    let input = if tool_use.input.is_null() {
                        Value::Object(serde_json::Map::new())
                    } else {
                        tool_use.input.clone()
                    };
                    content_blocks.push(ContentBlock::ToolUse {
                        id: tool_use.id.clone(),
                        name: tool_use.name.clone(),
                        input,
                    });
                }

                // Append tool_use blocks to previous assistant message if it exists
                // (API expects text + tool_use in same assistant message)
                if let Some(last_api_msg) = api_messages.last_mut() {
                    if last_api_msg.role == "assistant" {
                        last_api_msg.content.extend(content_blocks);
                        continue;
                    }
                }
            } else {
                // Skip tool call messages without matching results - they can't be included
                continue;
            }
        } else {
            // Regular text message
            // Determine content based on status
            let message_content = match msg.status {
                MessageStatus::Summarized => {
                    // Use TL;DR if available, otherwise fall back to content
                    msg.tl_dr.as_ref().unwrap_or(&msg.content).clone()
                }
                _ => msg.content.clone(),
            };

            // Add text content if present, with message id prefix
            if !message_content.is_empty() {
                // Build the message text with id prefix
                let prefixed_content = format!("[{}]: {}", msg.id, message_content);

                let text = if msg.role == "user" && !context_parts.is_empty() && api_messages.is_empty() {
                    // Prepend file context to first user message
                    let context = context_parts.join("\n\n");
                    format!("{}\n\n{}", context, prefixed_content)
                } else {
                    prefixed_content
                };
                content_blocks.push(ContentBlock::Text { text });
            }

            // For the last assistant message before tool results, include any tool_uses
            // (this handles the transition when we're about to send tool_results)
            let is_last = idx == messages.len().saturating_sub(1);
            if msg.role == "assistant" && include_last_tool_uses && is_last && !msg.tool_uses.is_empty() {
                for tool_use in &msg.tool_uses {
                    // Ensure input is always an object, never null
                    let input = if tool_use.input.is_null() {
                        Value::Object(serde_json::Map::new())
                    } else {
                        tool_use.input.clone()
                    };
                    content_blocks.push(ContentBlock::ToolUse {
                        id: tool_use.id.clone(),
                        name: tool_use.name.clone(),
                        input,
                    });
                }
            }
        }

        if !content_blocks.is_empty() {
            api_messages.push(ApiMessage {
                role: msg.role.clone(),
                content: content_blocks,
            });
        }
    }

    api_messages
}

/// Start streaming with optional tool results to continue
pub fn start_streaming(
    messages: Vec<Message>,
    context_items: Vec<ContextItem>,
    tools: Vec<ToolDefinition>,
    tool_results: Option<Vec<ToolResult>>,
    tx: Sender<StreamEvent>,
) {
    thread::spawn(move || {
        if let Err(e) = stream_response(&messages, &context_items, &tools, None, tool_results.as_deref(), &tx) {
            let _ = tx.send(StreamEvent::Error(e));
        }
    });
}

/// Start context cleaning with specialized prompt and limited tools
pub fn start_cleaning(
    messages: Vec<Message>,
    context_items: Vec<ContextItem>,
    tools: Vec<ToolDefinition>,
    state: &crate::state::State,
    tx: Sender<StreamEvent>,
) {
    let cleaner_context = crate::context_cleaner::build_cleaner_context(state);
    let system_prompt = crate::context_cleaner::get_cleaner_system_prompt().to_string();

    thread::spawn(move || {
        if let Err(e) = stream_response(&messages, &context_items, &tools, Some((&system_prompt, &cleaner_context)), None, &tx) {
            let _ = tx.send(StreamEvent::Error(e));
        }
    });
}

fn stream_response(
    messages: &[Message],
    context_items: &[ContextItem],
    tools: &[ToolDefinition],
    cleaner_prompt: Option<(&str, &str)>, // (system_prompt, cleaner_context)
    tool_results: Option<&[ToolResult]>,
    tx: &Sender<StreamEvent>,
) -> Result<(), String> {
    dotenvy::dotenv().ok();
    let api_key = env::var("ANTHROPIC_API_KEY")
        .map_err(|_| "ANTHROPIC_API_KEY not set".to_string())?;

    let client = Client::new();

    // Include tool_uses in last assistant message only if we're sending tool_results
    let include_tool_uses = tool_results.is_some();
    let mut api_messages = messages_to_api(messages, context_items, include_tool_uses);

    // If we have tool results, add them
    if let Some(results) = tool_results {
        // Add tool results as a user message
        let tool_result_blocks: Vec<ContentBlock> = results
            .iter()
            .map(|r| ContentBlock::ToolResult {
                tool_use_id: r.tool_use_id.clone(),
                content: r.content.clone(),
            })
            .collect();

        api_messages.push(ApiMessage {
            role: "user".to_string(),
            content: tool_result_blocks,
        });
    }

    // Handle cleaner mode - add cleaner context as user message
    let system_prompt = if let Some((prompt, context)) = cleaner_prompt {
        api_messages.push(ApiMessage {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: format!("Please clean up the context to reduce token usage:\n\n{}", context),
            }],
        });
        prompt.to_string()
    } else {
        prompts::MAIN_SYSTEM.to_string()
    };

    let request = ApiRequest {
        model: MODEL_MAIN.to_string(),
        max_tokens: MAX_RESPONSE_TOKENS,
        system: system_prompt,
        messages: api_messages,
        tools: build_api_tools(tools),
        stream: true,
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
        let status = response.status();
        let body = response.text().unwrap_or_default();
        return Err(format!("API error {}: {}", status, body));
    }

    let reader = BufReader::new(response);
    let mut input_tokens = 0;
    let mut output_tokens = 0;

    // Track current tool use being built
    let mut current_tool: Option<(String, String, String)> = None; // (id, name, json)

    for line in reader.lines() {
        let line = line.map_err(|e| format!("Read error: {}", e))?;

        if !line.starts_with("data: ") {
            continue;
        }

        let json_str = &line[6..];
        if json_str == "[DONE]" {
            break;
        }

        if let Ok(event) = serde_json::from_str::<StreamMessage>(json_str) {
            match event.event_type.as_str() {
                "content_block_start" => {
                    if let Some(block) = event.content_block {
                        if block.block_type.as_deref() == Some("tool_use") {
                            current_tool = Some((
                                block.id.unwrap_or_default(),
                                block.name.unwrap_or_default(),
                                String::new(),
                            ));
                        }
                    }
                }
                "content_block_delta" => {
                    if let Some(delta) = event.delta {
                        match delta.delta_type.as_deref() {
                            Some("text_delta") => {
                                if let Some(text) = delta.text {
                                    let _ = tx.send(StreamEvent::Chunk(text));
                                }
                            }
                            Some("input_json_delta") => {
                                if let Some(json) = delta.partial_json {
                                    if let Some((_, _, ref mut input)) = current_tool {
                                        input.push_str(&json);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                "content_block_stop" => {
                    if let Some((id, name, input_json)) = current_tool.take() {
                        // Fallback to empty object {} (not null) - API requires input to be a valid dictionary
                        let input: Value = serde_json::from_str(&input_json)
                            .unwrap_or_else(|_| Value::Object(serde_json::Map::new()));
                        let _ = tx.send(StreamEvent::ToolUse(ToolUse { id, name, input }));
                    }
                }
                "message_delta" => {
                    if let Some(usage) = event.usage {
                        if let Some(inp) = usage.input_tokens {
                            input_tokens = inp;
                        }
                        if let Some(out) = usage.output_tokens {
                            output_tokens = out;
                        }
                    }
                }
                "message_stop" => break,
                _ => {}
            }
        }
    }

    let _ = tx.send(StreamEvent::Done { input_tokens, output_tokens });
    Ok(())
}
