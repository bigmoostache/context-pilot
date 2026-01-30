use std::env;
use std::io::{BufRead, BufReader};
use std::sync::mpsc::Sender;
use std::thread;

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::constants::{MODEL_MAIN, MAX_RESPONSE_TOKENS, API_ENDPOINT, API_VERSION};
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
    _input_tokens: Option<usize>,
    output_tokens: Option<usize>,
}

/// Converts our messages to API format, including file context and directory tree
/// If include_last_tool_uses is true, tool_use blocks from the last assistant message are included
fn messages_to_api(
    messages: &[Message],
    file_context: &[(String, String)],
    glob_context: &[(String, String)],
    tmux_context: &[(String, String)],
    todo_context: &str,
    memory_context: &str,
    overview_context: &str,
    directory_tree: &str,
    include_last_tool_uses: bool,
) -> Vec<ApiMessage> {
    let mut api_messages: Vec<ApiMessage> = Vec::new();

    // Build system context with tree, files, globs, tmux, and todos
    let mut context_parts: Vec<String> = Vec::new();

    // Add directory tree first
    if !directory_tree.is_empty() {
        context_parts.push(format!("=== Directory Tree ===\n{}\n=== End of Directory Tree ===", directory_tree));
    }

    // Add todo list
    if !todo_context.is_empty() {
        context_parts.push(format!("=== Todo List ===\n{}\n=== End of Todo List ===", todo_context));
    }

    // Add memories
    if !memory_context.is_empty() && memory_context != "No memories" {
        context_parts.push(format!("=== Memories ===\n{}\n=== End of Memories ===", memory_context));
    }

    // Add context overview (self-awareness of context usage)
    if !overview_context.is_empty() {
        context_parts.push(format!("=== Context Overview ===\n{}\n=== End of Context Overview ===", overview_context));
    }

    // Add open files
    for (path, content) in file_context {
        context_parts.push(format!("=== File: {} ===\n{}\n=== End of {} ===", path, content, path));
    }

    // Add glob results
    for (name, results) in glob_context {
        context_parts.push(format!("=== {} ===\n{}\n=== End of {} ===", name, results, name));
    }

    // Add tmux pane outputs
    for (header, content) in tmux_context {
        context_parts.push(format!("=== {} ===\n{}\n=== End of {} ===", header, content, header));
    }

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

        // Handle tool call messages - include if there's a ToolResult message after them
        if msg.message_type == MessageType::ToolCall {
            // Check if there's a ToolResult message after this tool call
            let has_tool_result_after = messages[idx + 1..].iter()
                .any(|m| m.message_type == MessageType::ToolResult);

            if has_tool_result_after {
                for tool_use in &msg.tool_uses {
                    content_blocks.push(ContentBlock::ToolUse {
                        id: tool_use.id.clone(),
                        name: tool_use.name.clone(),
                        input: tool_use.input.clone(),
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
                // Skip tool call messages without results - they can't be included
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
                    content_blocks.push(ContentBlock::ToolUse {
                        id: tool_use.id.clone(),
                        name: tool_use.name.clone(),
                        input: tool_use.input.clone(),
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
    file_context: Vec<(String, String)>,
    glob_context: Vec<(String, String)>,
    tmux_context: Vec<(String, String)>,
    todo_context: String,
    memory_context: String,
    overview_context: String,
    directory_tree: String,
    tools: Vec<ToolDefinition>,
    tool_results: Option<Vec<ToolResult>>,
    tx: Sender<StreamEvent>,
) {
    thread::spawn(move || {
        if let Err(e) = stream_response(&messages, &file_context, &glob_context, &tmux_context, &todo_context, &memory_context, &overview_context, &directory_tree, &tools, None, tool_results.as_deref(), &tx) {
            let _ = tx.send(StreamEvent::Error(e));
        }
    });
}

/// Start context cleaning with specialized prompt and limited tools
pub fn start_cleaning(
    messages: Vec<Message>,
    file_context: Vec<(String, String)>,
    glob_context: Vec<(String, String)>,
    tmux_context: Vec<(String, String)>,
    todo_context: String,
    memory_context: String,
    overview_context: String,
    directory_tree: String,
    tools: Vec<ToolDefinition>,
    state: &crate::state::State,
    tx: Sender<StreamEvent>,
) {
    let cleaner_context = crate::context_cleaner::build_cleaner_context(state);
    let system_prompt = crate::context_cleaner::get_cleaner_system_prompt().to_string();

    thread::spawn(move || {
        if let Err(e) = stream_response(&messages, &file_context, &glob_context, &tmux_context, &todo_context, &memory_context, &overview_context, &directory_tree, &tools, Some((&system_prompt, &cleaner_context)), None, &tx) {
            let _ = tx.send(StreamEvent::Error(e));
        }
    });
}

fn stream_response(
    messages: &[Message],
    file_context: &[(String, String)],
    glob_context: &[(String, String)],
    tmux_context: &[(String, String)],
    todo_context: &str,
    memory_context: &str,
    overview_context: &str,
    directory_tree: &str,
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
    let mut api_messages = messages_to_api(messages, file_context, glob_context, tmux_context, todo_context, memory_context, overview_context, directory_tree, include_tool_uses);

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
        String::new()
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
                        let input: Value = serde_json::from_str(&input_json).unwrap_or(Value::Null);
                        let _ = tx.send(StreamEvent::ToolUse(ToolUse { id, name, input }));
                    }
                }
                "message_delta" => {
                    if let Some(usage) = event.usage {
                        output_tokens = usage.output_tokens.unwrap_or(0);
                    }
                }
                "message_stop" => break,
                _ => {}
            }
        }
    }

    let _ = tx.send(StreamEvent::Done { input_tokens: 0, output_tokens });
    Ok(())
}
