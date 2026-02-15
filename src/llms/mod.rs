//! LLM provider abstraction layer.
//!
//! Provides a unified interface for different LLM providers (Anthropic, Grok, Groq, Claude Code OAuth)

pub mod anthropic;
pub mod claude_code;
pub mod deepseek;
pub mod error;
pub mod grok;
pub mod groq;
pub mod openai_compat;

use std::sync::mpsc::Sender;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::core::panels::ContextItem;
use crate::state::Message;
use crate::tool_defs::ToolDefinition;
use crate::tools::ToolResult;

// Re-export LLM types from cp-base so that `crate::llms::LlmProvider` etc. work
pub use cp_base::llm_types::{
    AnthropicModel, ApiCheckResult, DeepSeekModel, GrokModel, GroqModel, LlmProvider, ModelInfo, StreamEvent,
};

/// Configuration for an LLM request
#[derive(Debug, Clone)]
pub struct LlmRequest {
    pub model: String,
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
}

/// Trait for LLM providers
pub trait LlmClient: Send + Sync {
    /// Start a streaming response
    fn stream(&self, request: LlmRequest, tx: Sender<StreamEvent>) -> Result<(), error::LlmError>;

    /// Check API connectivity: auth, streaming, and tool calling
    fn check_api(&self, model: &str) -> ApiCheckResult;
}

/// Get the appropriate LLM client for the given provider
pub fn get_client(provider: LlmProvider) -> Box<dyn LlmClient> {
    match provider {
        LlmProvider::Anthropic => Box::new(anthropic::AnthropicClient::new()),
        LlmProvider::ClaudeCode => Box::new(claude_code::ClaudeCodeClient::new()),
        LlmProvider::Grok => Box::new(grok::GrokClient::new()),
        LlmProvider::Groq => Box::new(groq::GroqClient::new()),
        LlmProvider::DeepSeek => Box::new(deepseek::DeepSeekClient::new()),
    }
}

/// Start API check in background
pub fn start_api_check(provider: LlmProvider, model: String, tx: Sender<ApiCheckResult>) {
    let client = get_client(provider);
    std::thread::spawn(move || {
        let result = client.check_api(&model);
        let _ = tx.send(result);
    });
}

/// Parameters for starting a streaming LLM request
pub struct StreamParams {
    pub provider: LlmProvider,
    pub model: String,
    pub messages: Vec<Message>,
    pub context_items: Vec<ContextItem>,
    pub tools: Vec<ToolDefinition>,
    pub system_prompt: String,
    pub seed_content: Option<String>,
    pub worker_id: String,
}

/// Start streaming with the specified provider and model
pub fn start_streaming(params: StreamParams, tx: Sender<StreamEvent>) {
    let client = get_client(params.provider);

    std::thread::spawn(move || {
        let request = LlmRequest {
            model: params.model,
            messages: params.messages,
            context_items: params.context_items,
            tools: params.tools,
            tool_results: None,
            system_prompt: Some(params.system_prompt),
            extra_context: None,
            seed_content: params.seed_content,
            worker_id: params.worker_id,
        };

        if let Err(e) = client.stream(request, tx.clone()) {
            let _ = tx.send(StreamEvent::Error(e.to_string()));
        }
    });
}

// Re-export common types used by providers
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
pub struct ApiMessage {
    pub role: String,
    pub content: Vec<ContentBlock>,
}

/// Prepared panel data for injection as fake tool call/result pairs
#[derive(Debug, Clone)]
pub struct FakePanelMessage {
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
    if let Ok(since_epoch) = datetime.duration_since(UNIX_EPOCH) {
        let secs = since_epoch.as_secs();
        // Calculate components
        let days_since_epoch = secs / 86400;
        let time_of_day = secs % 86400;
        let hours = time_of_day / 3600;
        let minutes = (time_of_day % 3600) / 60;
        let seconds = time_of_day % 60;

        // Calculate year/month/day from days since 1970-01-01
        let mut year = 1970i32;
        let mut remaining_days = days_since_epoch as i32;

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
        for days in days_in_months.iter() {
            if remaining_days < *days {
                break;
            }
            remaining_days -= days;
            month += 1;
        }
        let day = remaining_days + 1;

        format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", year, month, day, hours, minutes, seconds)
    } else {
        "1970-01-01T00:00:00Z".to_string()
    }
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// Format a time delta in a human-readable way
fn format_time_delta(delta_ms: u64) -> String {
    let seconds = delta_ms / 1000;
    if seconds < 60 {
        format!("{} seconds ago", seconds)
    } else if seconds < 3600 {
        let minutes = seconds / 60;
        if minutes == 1 { "1 minute ago".to_string() } else { format!("{} minutes ago", minutes) }
    } else {
        let hours = seconds / 3600;
        if hours == 1 { "1 hour ago".to_string() } else { format!("{} hours ago", hours) }
    }
}

/// Generate the header text for dynamic panel display
pub fn panel_header_text() -> &'static str {
    crate::constants::prompts::panel_header()
}

/// Generate the timestamp text for an individual panel
/// Handles zero/unknown timestamps gracefully
pub fn panel_timestamp_text(timestamp_ms: u64) -> String {
    use crate::constants::prompts;

    // Check for zero/invalid timestamp (1970-01-01 or very old)
    // Consider anything before year 2020 as invalid (timestamp < ~1577836800000)
    if timestamp_ms < 1577836800000 {
        return prompts::panel_timestamp_unknown().to_string();
    }

    let iso_time = ms_to_iso8601(timestamp_ms);

    prompts::panel_timestamp().replace("{iso_time}", &iso_time)
}

/// Generate the footer text for dynamic panel display, including message timestamps
pub fn panel_footer_text(messages: &[Message], current_ms: u64) -> String {
    use crate::constants::prompts;

    // Get last 25 messages with non-zero timestamps
    let recent_messages: Vec<&Message> = messages.iter().filter(|m| m.timestamp_ms > 0).rev().take(25).collect();

    // Build message timestamps section
    let message_timestamps = if !recent_messages.is_empty() {
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
    } else {
        String::new()
    };

    prompts::panel_footer()
        .replace("{message_timestamps}", &message_timestamps)
        .replace("{current_datetime}", &ms_to_iso8601(current_ms))
}

/// Prepare context items for injection as fake tool call/result pairs.
/// - Filters out Conversation (id="chat") -- it's sent as actual messages, not a panel
/// - Items are assumed to be pre-sorted by last_refresh_ms (done in prepare_stream_context)
/// - Returns FakePanelMessage structs that providers can convert to their format
pub fn prepare_panel_messages(context_items: &[ContextItem]) -> Vec<FakePanelMessage> {
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
