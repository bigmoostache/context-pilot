//! LLM provider abstraction layer.
//!
//! Provides a unified interface for different LLM providers (Anthropic, Grok, Groq, Claude Code OAuth)

pub(crate) mod anthropic;
pub(crate) mod claude_code;
pub(crate) mod claude_code_api_key;
pub(crate) mod oai_providers;
pub(crate) mod openai_compat;
pub(crate) mod openai_streaming;

use std::sync::mpsc::Sender;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::app::panels::ContextItem;
use crate::infra::tools::ToolDefinition;
use crate::infra::tools::ToolResult;
use crate::state::Message;
use cp_base::cast::SafeCast;

// Re-export LLM types from cp-base so that `crate::llms::LlmProvider` etc. work
pub(crate) use cp_base::llm_types::{
    AnthropicModel, ApiCheckResult, DeepSeekModel, GrokModel, GroqModel, LlmProvider, ModelInfo, StreamEvent,
};

// Re-export provider clients through the module path for get_client()
use oai_providers::deepseek;
use oai_providers::grok;
use oai_providers::groq;

/// Configuration for an LLM request
#[derive(Debug, Clone)]
pub(crate) struct LlmRequest {
    pub model: String,
    pub max_output_tokens: u32,
    pub messages: Vec<Message>,
    pub context_items: Vec<ContextItem>,
    pub tools: Vec<ToolDefinition>,
    pub tool_results: Option<Vec<ToolResult>>,
    pub system_prompt: Option<String>,
    pub extra_context: Option<String>,
    /// Seed/system prompt content to repeat after panels
    pub seed_content: Option<String>,
    /// Worker/reverie ID for debug logging
    pub worker_id: String,
    /// Pre-assembled API messages (panels + seed + conversation).
    /// When non-empty, providers should use this instead of doing their own assembly.
    pub api_messages: Vec<ApiMessage>,
}

/// Trait for LLM providers
pub(crate) trait LlmClient: Send + Sync {
    /// Start a streaming response
    fn stream(&self, request: LlmRequest, tx: Sender<StreamEvent>) -> Result<(), error::LlmError>;

    /// Check API connectivity: auth, streaming, and tool calling
    fn check_api(&self, model: &str) -> ApiCheckResult;
}

/// Get the appropriate LLM client for the given provider
pub(crate) fn get_client(provider: LlmProvider) -> Box<dyn LlmClient> {
    match provider {
        LlmProvider::Anthropic => Box::new(anthropic::AnthropicClient::new()),
        LlmProvider::ClaudeCode => Box::new(claude_code::ClaudeCodeClient::new()),
        LlmProvider::ClaudeCodeApiKey => Box::new(claude_code_api_key::ClaudeCodeApiKeyClient::new()),
        LlmProvider::Grok => Box::new(grok::GrokClient::new()),
        LlmProvider::Groq => Box::new(groq::GroqClient::new()),
        LlmProvider::DeepSeek => Box::new(deepseek::DeepSeekClient::new()),
    }
}

/// Start API check in background
pub(crate) fn start_api_check(provider: LlmProvider, model: String, tx: Sender<ApiCheckResult>) {
    let client = get_client(provider);
    let _r = std::thread::spawn(move || {
        let result = client.check_api(&model);
        let _r = tx.send(result);
    });
}

/// Parameters for starting a streaming LLM request
pub(crate) struct StreamParams {
    pub provider: LlmProvider,
    pub model: String,
    pub max_output_tokens: u32,
    pub messages: Vec<Message>,
    pub context_items: Vec<ContextItem>,
    pub tools: Vec<ToolDefinition>,
    pub system_prompt: String,
    pub seed_content: Option<String>,
    pub worker_id: String,
}

/// Start streaming with the specified provider and model
pub(crate) fn start_streaming(params: StreamParams, tx: Sender<StreamEvent>) {
    let client = get_client(params.provider);

    let _r = std::thread::spawn(move || {
        // Assemble the prompt (panels + seed + conversation → api_messages)
        let include_tool_uses = false; // No pending tool results on first stream
        let api_messages = crate::app::prompt_builder::assemble_prompt(
            &params.messages,
            &params.context_items,
            include_tool_uses,
            params.seed_content.as_deref(),
        );

        let request = LlmRequest {
            model: params.model,
            max_output_tokens: params.max_output_tokens,
            messages: params.messages,
            context_items: params.context_items,
            tools: params.tools,
            tool_results: None,
            system_prompt: Some(params.system_prompt),
            extra_context: None,
            seed_content: params.seed_content,
            worker_id: params.worker_id,
            api_messages,
        };

        if let Err(e) = client.stream(request, tx.clone()) {
            let _r = tx.send(StreamEvent::Error(e.to_string()));
        }
    });
}

// Re-export common types used by providers
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse { id: String, name: String, input: Value },
    #[serde(rename = "tool_result")]
    ToolResult { tool_use_id: String, content: String },
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ApiMessage {
    pub role: String,
    pub content: Vec<ContentBlock>,
}

/// Prepared panel data for injection as fake tool call/result pairs
#[derive(Debug, Clone)]
pub(crate) struct FakePanelMessage {
    /// Panel ID (e.g., "P2", "P7")
    pub panel_id: String,
    /// Timestamp in milliseconds since UNIX epoch
    pub timestamp_ms: u64,
    /// Panel content with header
    pub content: String,
}

/// Convert milliseconds since UNIX epoch to ISO 8601 format
fn ms_to_iso8601(ms: u64) -> String {
    use std::time::{Duration, UNIX_EPOCH};
    let duration = Duration::from_millis(ms);
    let datetime = UNIX_EPOCH + duration;

    // Manual formatting since we don't have chrono
    let since_epoch = datetime.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = since_epoch.as_secs();
    // Calculate components
    let days_since_epoch = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Calculate year/month/day from days since 1970-01-01
    let mut year = 1970i32;
    let mut remaining_days = days_since_epoch.to_i32();

    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        year += 1;
    }

    let days_in_months: [i32; 12] = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month = 1;
    for days in &days_in_months {
        if remaining_days < *days {
            break;
        }
        remaining_days -= days;
        month += 1;
    }
    let day = remaining_days + 1;

    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

const fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// Format a time delta in a human-readable way
fn format_time_delta(delta_ms: u64) -> String {
    let seconds = delta_ms / 1000;
    if seconds < 60 {
        format!("{seconds} seconds ago")
    } else if seconds < 3600 {
        let minutes = seconds / 60;
        if minutes == 1 { "1 minute ago".to_string() } else { format!("{minutes} minutes ago") }
    } else {
        let hours = seconds / 3600;
        if hours == 1 { "1 hour ago".to_string() } else { format!("{hours} hours ago") }
    }
}

/// Generate the header text for dynamic panel display
pub(crate) fn panel_header_text() -> &'static str {
    crate::infra::constants::prompts::panel_header()
}

/// Generate the timestamp text for an individual panel
/// Handles zero/unknown timestamps gracefully
pub(crate) fn panel_timestamp_text(timestamp_ms: u64) -> String {
    use crate::infra::constants::prompts;

    // Check for zero/invalid timestamp (1970-01-01 or very old)
    // Consider anything before year 2020 as invalid (timestamp < ~1_577_836_800_000)
    if timestamp_ms < 1_577_836_800_000 {
        return prompts::panel_timestamp_unknown().to_string();
    }

    let iso_time = ms_to_iso8601(timestamp_ms);

    prompts::panel_timestamp().replace("{iso_time}", &iso_time)
}

/// Generate the footer text for dynamic panel display, including message timestamps
pub(crate) fn panel_footer_text(messages: &[Message], current_ms: u64) -> String {
    use crate::infra::constants::prompts;

    // Get last 25 messages with non-zero timestamps
    let recent_messages: Vec<&Message> = messages.iter().filter(|m| m.timestamp_ms > 0).rev().take(25).collect();

    // Build message timestamps section
    let message_timestamps = if recent_messages.is_empty() {
        String::new()
    } else {
        let mut lines = String::from(prompts::panel_footer_msg_header());
        lines.push('\n');
        for msg in recent_messages.iter().rev() {
            let iso_time = ms_to_iso8601(msg.timestamp_ms);
            let time_delta = if current_ms > msg.timestamp_ms {
                format_time_delta(current_ms - msg.timestamp_ms)
            } else {
                "just now".to_string()
            };
            let line = prompts::panel_footer_msg_line()
                .replace("{role}", &msg.role)
                .replace("{iso_time}", &iso_time)
                .replace("{time_delta}", &time_delta);
            lines.push_str(&line);
            lines.push('\n');
        }
        lines
    };

    prompts::panel_footer()
        .replace("{message_timestamps}", &message_timestamps)
        .replace("{current_datetime}", &ms_to_iso8601(current_ms))
}

/// Prepare context items for injection as fake tool call/result pairs.
/// - Filters out Conversation (id="chat") -- it's sent as actual messages, not a panel
/// - Items are assumed to be pre-sorted by `last_refresh_ms` (done in `prepare_stream_context`)
/// - Returns `FakePanelMessage` structs that providers can convert to their format
pub(crate) fn prepare_panel_messages(context_items: &[ContextItem]) -> Vec<FakePanelMessage> {
    // Filter out Conversation panel (id="chat") -- it's the live message feed, not a context panel
    let filtered: Vec<&ContextItem> =
        context_items.iter().filter(|item| !item.content.is_empty()).filter(|item| item.id != "chat").collect();

    filtered
        .into_iter()
        .map(|item| FakePanelMessage {
            panel_id: item.id.clone(),
            timestamp_ms: item.last_refresh_ms,
            content: format!("======= [{}] {} =======\n{}", item.id, item.header, item.content),
        })
        .collect()
}

/// Convert pre-assembled `Vec<ApiMessage>` into Claude Code's raw JSON format.
///
/// Claude Code requires raw `serde_json::Value` messages (not typed structs).
/// This also injects `cache_control` breakpoints at 25/50/75/100% of panel
/// `tool_result` positions for prefix-based cache optimization.
///
/// Shared between `claude_code` and `claude_code_api_key` providers.
pub(crate) fn api_messages_to_cc_json(api_messages: &[ApiMessage]) -> Vec<Value> {
    // Find all panel tool_result indices for cache breakpoints
    let panel_result_indices: Vec<usize> = api_messages
        .iter()
        .enumerate()
        .filter(|(_, m)| {
            m.role == "user"
                && m.content.iter().any(
                    |b| matches!(b, ContentBlock::ToolResult { tool_use_id, .. } if tool_use_id.starts_with("panel_")),
                )
        })
        .map(|(i, _)| i)
        .collect();

    let panel_count = panel_result_indices.len();
    let mut cache_breakpoints = std::collections::BTreeSet::new();
    if panel_count > 0 {
        for quarter in 1..=4usize {
            let pos = (panel_count * quarter).div_ceil(4);
            let _r = cache_breakpoints.insert(pos.saturating_sub(1));
        }
    }

    let mut json_messages: Vec<Value> = Vec::new();

    for (msg_idx, msg) in api_messages.iter().enumerate() {
        let content_blocks: Vec<Value> = msg
            .content
            .iter()
            .map(|block| match block {
                ContentBlock::Text { text } => serde_json::json!({"type": "text", "text": text}),
                ContentBlock::ToolUse { id, name, input } => {
                    serde_json::json!({"type": "tool_use", "id": id, "name": name, "input": input})
                }
                ContentBlock::ToolResult { tool_use_id, content } => {
                    let mut result =
                        serde_json::json!({"type": "tool_result", "tool_use_id": tool_use_id, "content": content});
                    // Add cache_control at breakpoint positions
                    if let Some(panel_pos) = panel_result_indices.iter().position(|&i| i == msg_idx)
                        && cache_breakpoints.contains(&panel_pos)
                    {
                        result["cache_control"] = serde_json::json!({"type": "ephemeral"});
                    }
                    result
                }
            })
            .collect();

        json_messages.push(serde_json::json!({
            "role": msg.role,
            "content": content_blocks
        }));
    }

    json_messages
}

/// Log an SSE error event to `.context-pilot/errors/sse_errors.log` for post-mortem debugging.
pub(crate) fn log_sse_error(
    provider: &str,
    json_str: &str,
    total_bytes: usize,
    line_count: usize,
    last_lines: &[String],
) {
    use std::io::Write;

    let dir = std::path::Path::new(".context-pilot").join("errors");
    let _r = std::fs::create_dir_all(&dir);
    let path = dir.join("sse_errors.log");

    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let recent = if last_lines.is_empty() { "(none)".to_string() } else { last_lines.join("\n") };
    let entry = format!(
        "[{ts}] SSE error event ({provider})\n\
         Stream position: {total_bytes} bytes, {line_count} lines\n\
         Error data: {json_str}\n\
         Last SSE lines:\n{recent}\n\
         ---\n"
    );

    let _r = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(entry.as_bytes()));
}

pub(crate) mod error {
    use std::fmt;

    /// Typed error for LLM streaming operations.
    #[derive(Debug)]
    pub(crate) enum LlmError {
        Auth(String),
        Network(String),
        Api { status: u16, body: String },
        StreamRead(String),
    }

    impl fmt::Display for LlmError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::Auth(msg) => write!(f, "Auth error: {msg}"),
                Self::Network(msg) => write!(f, "Network error: {msg}"),
                Self::Api { status, body } => write!(f, "API error {status}: {body}"),
                Self::StreamRead(msg) => write!(f, "Stream read error: {msg}"),
            }
        }
    }

    impl std::error::Error for LlmError {}

    impl From<reqwest::Error> for LlmError {
        fn from(e: reqwest::Error) -> Self {
            Self::Network(e.to_string())
        }
    }
}
