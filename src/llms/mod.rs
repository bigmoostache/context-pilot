//! LLM provider abstraction layer.
//!
//! Provides a unified interface for different LLM providers (Anthropic, Grok, Groq, Claude Code OAuth)

pub(crate) mod anthropic;
pub(crate) mod claude_code;
pub(crate) mod claude_code_api_key;
/// MiniMax provider (Anthropic-compatible API via Token Plan).
pub(crate) mod minimax;
/// OpenAI-compatible provider implementations (Grok, Groq, DeepSeek).
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
use cp_base::cast::Safe as _;

// Re-export LLM types from cp-base so that `crate::llms::LlmProvider` etc. work
pub(crate) use cp_base::config::llm_types::{
    AnthropicModel, ApiCheckResult, DeepSeekModel, GrokModel, GroqModel, LlmProvider, MiniMaxModel, ModelInfo,
    StreamEvent,
};

// Re-export provider clients through the module path for get_client()
use oai_providers::deepseek;
use oai_providers::grok;
use oai_providers::groq;

/// Configuration for an LLM request
#[derive(Debug, Clone)]
pub(crate) struct LlmRequest {
    /// Model identifier string
    pub model: String,
    /// Maximum number of output tokens to generate
    pub max_output_tokens: u32,
    /// Conversation messages to send
    pub messages: Vec<Message>,
    /// Context items (panels) to inject
    pub context_items: Vec<ContextItem>,
    /// Tool definitions available for the model
    pub tools: Vec<ToolDefinition>,
    /// Pending tool results from a tool loop
    pub tool_results: Option<Vec<ToolResult>>,
    /// Custom system prompt (falls back to default if None)
    pub system_prompt: Option<String>,
    /// Extra context for cleaner mode
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
        LlmProvider::MiniMax => Box::new(minimax::MiniMaxClient::new()),
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
    /// Which LLM provider to use
    pub provider: LlmProvider,
    /// Model identifier string
    pub model: String,
    /// Maximum number of output tokens to generate
    pub max_output_tokens: u32,
    /// Conversation messages to send
    pub messages: Vec<Message>,
    /// Context items (panels) to inject
    pub context_items: Vec<ContextItem>,
    /// Tool definitions available for the model
    pub tools: Vec<ToolDefinition>,
    /// System prompt text
    pub system_prompt: String,
    /// Seed content to repeat after panels
    pub seed_content: Option<String>,
    /// Worker/reverie ID for debug logging
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

/// Content block types used in API messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub(crate) enum ContentBlock {
    /// Plain text content
    #[serde(rename = "text")]
    Text {
        /// The text content
        text: String,
    },
    /// Tool use request from the assistant
    #[serde(rename = "tool_use")]
    ToolUse {
        /// Tool invocation ID
        id: String,
        /// Tool name
        name: String,
        /// Tool input parameters
        input: Value,
    },
    /// Tool result response from the user
    #[serde(rename = "tool_result")]
    ToolResult {
        /// ID of the tool use this result responds to
        tool_use_id: String,
        /// Tool result content
        content: String,
    },
}

/// A single message in the API conversation format.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ApiMessage {
    /// Message role (e.g. "user", "assistant")
    pub role: String,
    /// Content blocks within this message
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

/// Convert milliseconds since UNIX epoch to ISO 8601 format.
fn ms_to_iso8601(ms: u64) -> String {
    use chrono::{DateTime, Utc};

    let secs = cp_base::panels::time_arith::ms_to_secs(ms);
    DateTime::<Utc>::from_timestamp(secs.to_i64(), 0)
        .map_or_else(|| "1970-01-01T00:00:00Z".to_string(), |dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
}

/// Convert milliseconds since UNIX epoch to date-only format (YYYY-MM-DD).
fn ms_to_date(ms: u64) -> String {
    use chrono::{DateTime, Utc};

    let secs = cp_base::panels::time_arith::ms_to_secs(ms);
    DateTime::<Utc>::from_timestamp(secs.to_i64(), 0)
        .map_or_else(|| "1970-01-01".to_string(), |dt| dt.format("%Y-%m-%d").to_string())
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

    prompts::panel_timestamp().replace(concat!("{", "iso_time", "}"), &iso_time)
}

/// Generate the footer text for dynamic panel display
pub(crate) fn panel_footer_text(current_ms: u64) -> String {
    use crate::infra::constants::prompts;

    let current_date = ms_to_date(current_ms);

    prompts::panel_footer().replace(concat!("{", "current_date", "}"), &current_date)
}

/// Prepare context items for injection as fake tool call/result pairs.
/// - Filters out Conversation (id="chat") -- it's sent as actual messages, not a panel
/// - Items are assumed to be pre-sorted by `last_refresh_ms` (done in `prepare_stream_context`)
/// - Returns `FakePanelMessage` structs that providers can convert to their format
pub(crate) fn prepare_panel_messages(context_items: &[ContextItem]) -> Vec<FakePanelMessage> {
    // Filter out Conversation panel (id="chat") -- it's the live message feed, not a context panel
    context_items
        .iter()
        .filter(|item| !item.content.is_empty())
        .filter(|item| item.id != "chat")
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
            let pos = (panel_count.saturating_mul(quarter)).div_ceil(4);
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
                        && let Some(obj) = result.as_object_mut()
                    {
                        let _prev = obj.insert("cache_control".to_string(), serde_json::json!({"type": "ephemeral"}));
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

/// Context for logging an SSE error event.
pub(crate) struct SseErrorContext<'ctx> {
    /// Name of the LLM provider that encountered the error
    pub provider: &'ctx str,
    /// Raw JSON string from the error event
    pub json_str: &'ctx str,
    /// Total bytes read from the stream so far
    pub total_bytes: usize,
    /// Total SSE lines read from the stream so far
    pub line_count: usize,
    /// Last few SSE lines for context
    pub last_lines: &'ctx [String],
}

/// Log an SSE error event to `.context-pilot/errors/sse_errors.log` for post-mortem debugging.
pub(crate) fn log_sse_error(ctx: &SseErrorContext<'_>) {
    use std::io::Write as _;

    let dir = std::path::Path::new(".context-pilot").join("errors");
    let _r = std::fs::create_dir_all(&dir);
    let path = dir.join("sse_errors.log");

    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let recent = if ctx.last_lines.is_empty() { "(none)".to_string() } else { ctx.last_lines.join("\n") };
    let entry = format!(
        "[{ts}] SSE error event ({})\n\
         Stream position: {} bytes, {} lines\n\
         Error data: {}\n\
         Last SSE lines:\n{recent}\n\
         ---\n",
        ctx.provider, ctx.total_bytes, ctx.line_count, ctx.json_str
    );

    let _rw = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(entry.as_bytes()));
}

/// LLM error types.
pub(crate) mod error {
    use std::fmt;

    /// Typed error for LLM streaming operations.
    #[derive(Debug)]
    pub(crate) enum LlmError {
        /// Authentication error (missing or invalid API key)
        Auth(String),
        /// Network-level error (connection failure, DNS, etc.)
        Network(String),
        /// API-level error with HTTP status code and response body
        Api {
            /// HTTP status code
            status: u16,
            /// Response body text
            body: String,
        },
        /// Error reading the SSE stream mid-response
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

    #[expect(clippy::missing_trait_methods, reason = "type_id/cause/provide are unstable or deprecated")]
    impl std::error::Error for LlmError {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            None
        }
    }

    impl From<reqwest::Error> for LlmError {
        fn from(e: reqwest::Error) -> Self {
            Self::Network(e.to_string())
        }
    }
}
