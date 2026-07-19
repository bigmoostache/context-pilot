//! LLM provider abstraction layer.
//!
//! Provides a unified interface for different LLM providers (Anthropic, Grok, Groq, Claude Code OAuth)

pub(crate) mod anthropic;
/// Prompt caching engine and optimizer (breakpoint placement, density models).
pub(crate) mod cache;
pub(crate) mod claude_code;
pub(crate) mod claude_code_api_key;
/// Claude Code V2 provider (OAuth, updated request format with Opus 4.8).
pub(crate) mod claude_code_v2;
/// MiniMax provider (Anthropic-compatible API via Token Plan).
pub(crate) mod minimax;
/// OpenAI-compatible provider implementations (Grok, Groq, DeepSeek).
pub(crate) mod oai_providers;

use std::sync::mpsc::Sender;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::app::panels::ContextItem;
use crate::infra::tools::ToolDefinition;
use crate::infra::tools::ToolResult;
use crate::state::Message;

// Re-export LLM types from cp-base so that `crate::llms::LlmProvider` etc. work
pub(crate) use cp_base::config::llm_types::{ApiCheckResult, LlmProvider, ModelInfo, StreamEvent};
pub(crate) use cp_base::config::models::{
    AnthropicModel, ClaudeCodeV2Model, DeepSeekModel, GrokModel, GroqModel, MiniMaxModel,
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
    /// Serialized cache optimization engine state (JSON) for breakpoint placement.
    /// Passed from `State.cache_engine_json` to the streaming thread.
    pub cache_engine_json: Option<String>,
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
        LlmProvider::ClaudeCodeV2 => Box::new(claude_code_v2::ClaudeCodeV2Client::new()),
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
    /// Serialized cache optimization engine state for breakpoint placement.
    pub cache_engine_json: Option<String>,
}

/// Start streaming with the specified provider and model
pub(crate) fn start_streaming(params: StreamParams, tx: Sender<StreamEvent>) {
    let client = get_client(params.provider);

    let _r = std::thread::spawn(move || {
        // Assemble the prompt (panels + seed + conversation → api_messages)
        let include_tool_uses = false; // No pending tool results on first stream
        let api_messages = crate::app::prompt::assemble_prompt(
            &params.messages,
            &params.context_items,
            include_tool_uses,
            params.seed_content.as_deref(),
        );

        // Dump prompt tick CSV for debugging cache behavior
        cache::prompt_tick_csv::dump_prompt_tick_csv(&api_messages);

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
            cache_engine_json: params.cache_engine_json,
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
    let ms_i64 = i64::try_from(ms).unwrap_or(i64::MAX);
    cp_mod_utilities::time::epoch_ms_to_rfc3339(ms_i64).unwrap_or_else(|| "1970-01-01T00:00:00Z".to_owned())
}

/// Convert milliseconds since UNIX epoch to date-only format (YYYY-MM-DD).
fn ms_to_date(ms: u64) -> String {
    let ms_i64 = i64::try_from(ms).unwrap_or(i64::MAX);
    cp_mod_utilities::time::epoch_ms_to_utc_date(ms_i64).unwrap_or_else(|| "1970-01-01".to_owned())
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
        return prompts::panel_timestamp_unknown().to_owned();
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

/// Result of converting `ApiMessage`s to Claude Code JSON format.
///
/// Bundles the JSON messages with cache engine metadata for post-request bookkeeping.
pub(crate) struct CcJsonResult {
    /// Raw JSON messages ready for the Claude Code API.
    pub json_messages: Vec<Value>,
    /// Accumulated hashes at the 4 breakpoint positions (for `record_breakpoints`).
    pub bp_hashes: Vec<String>,
    /// Panel IDs each breakpoint landed on, in prompt order — the BP→panel
    /// mapping the freeze pass reads next turn to widen the free-to-update region.
    pub bp_panel_ids: Vec<String>,
    /// How many stored BPs matched the current request's hash chain.
    pub alive_count: usize,
    /// Per-mille positions (0–1000) of alive BPs within the prompt.
    pub alive_positions_permille: Vec<u16>,
}

/// Recover the panel ID a breakpoint block belongs to, or `None` for a
/// non-panel block (system / tools / conversation / footer).
///
/// Panels are injected (see `inject_panel_messages`) as an assistant `tool_use`
/// plus a user `tool_result` pair, both keyed `panel_{id}`. A breakpoint may tag
/// the header `Text` block or the `tool_use`/`tool_result` block of a panel
/// message, so the whole message is scanned for the `panel_`-prefixed id instead
/// of only the tagged block. The `panel_footer` sentinel is excluded.
fn message_panel_id(msg: &ApiMessage) -> Option<String> {
    msg.content.iter().find_map(|block| {
        let raw = match block {
            ContentBlock::ToolUse { id, .. } => id,
            ContentBlock::ToolResult { tool_use_id, .. } => tool_use_id,
            ContentBlock::Text { .. } => return None,
        };
        raw.strip_prefix("panel_").filter(|id| *id != "footer").map(str::to_owned)
    })
}

/// Convert pre-assembled `Vec<ApiMessage>` into Claude Code's raw JSON format.
///
/// Claude Code requires raw `serde_json::Value` messages (not typed structs).
/// This injects up to 4 `cache_control` breakpoints using the **cache optimization engine**:
/// 1. Compute accumulated hashes for every content block.
/// 2. Find the cache frontier (deepest matching stored breakpoint within 5-min TTL).
/// 3. Place beacon BP at frontier + 20 blocks (extends the cached prefix).
/// 4. Place 3 remaining BPs via greedy weighted coverage to minimize expected cost.
///
/// When `engine_json` is `None`, uses a fresh engine (no stored breakpoints).
///
/// Shared between `claude_code` and `claude_code_api_key` providers.
pub(crate) fn api_messages_to_cc_json(api_messages: &[ApiMessage], engine_json: Option<&str>) -> CcJsonResult {
    // ── Phase 1: Load and prune cache engine ──
    let mut engine =
        engine_json.map_or_else(cache::cache_engine::CacheEngine::default, cache::cache_engine::CacheEngine::from_json);
    engine.prune(cp_base::panels::now_ms());

    // ── Phase 2: Compute optimal breakpoint positions ──
    let plan = engine.compute_breakpoints(api_messages);

    // ── Phase 3: Convert to JSON, tagging breakpoint blocks with cache_control ──
    let mut json_messages: Vec<Value> = Vec::new();

    for (msg_idx, msg) in api_messages.iter().enumerate() {
        let content_blocks: Vec<Value> = msg
            .content
            .iter()
            .enumerate()
            .map(|(blk_idx, block)| {
                let should_tag = plan.positions.contains(&(msg_idx, blk_idx));
                match block {
                    ContentBlock::Text { text } => {
                        let mut obj = serde_json::json!({"type": "text", "text": text});
                        if should_tag && let Some(o) = obj.as_object_mut() {
                            let _prev = o.insert("cache_control".to_owned(), serde_json::json!({"type": "ephemeral"}));
                        }
                        obj
                    }
                    ContentBlock::ToolUse { id, name, input } => {
                        let mut obj = serde_json::json!({"type": "tool_use", "id": id, "name": name, "input": input});
                        if should_tag && let Some(o) = obj.as_object_mut() {
                            let _prev = o.insert("cache_control".to_owned(), serde_json::json!({"type": "ephemeral"}));
                        }
                        obj
                    }
                    ContentBlock::ToolResult { tool_use_id, content } => {
                        let mut obj =
                            serde_json::json!({"type": "tool_result", "tool_use_id": tool_use_id, "content": content});
                        if should_tag && let Some(o) = obj.as_object_mut() {
                            let _prev = o.insert("cache_control".to_owned(), serde_json::json!({"type": "ephemeral"}));
                        }
                        obj
                    }
                }
            })
            .collect();

        json_messages.push(serde_json::json!({
            "role": msg.role,
            "content": content_blocks
        }));
    }

    // Map each breakpoint block back to the panel it landed on (in prompt order,
    // deduped) — persisted next turn for the freeze pass's free-region widening.
    let mut bp_panel_ids: Vec<String> = Vec::new();
    for (msg_idx, _blk_idx) in &plan.positions {
        if let Some(id) = api_messages.get(*msg_idx).and_then(message_panel_id)
            && !bp_panel_ids.contains(&id)
        {
            bp_panel_ids.push(id);
        }
    }

    CcJsonResult {
        json_messages,
        bp_hashes: plan.bp_hashes,
        bp_panel_ids,
        alive_count: plan.alive_count,
        alive_positions_permille: plan.alive_positions_permille,
    }
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
    let recent = if ctx.last_lines.is_empty() { "(none)".to_owned() } else { ctx.last_lines.join("\n") };
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
