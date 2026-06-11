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
pub(crate) use cp_base::config::llm_types::{
    AnthropicModel, ApiCheckResult, ClaudeCodeV2Model, DeepSeekModel, GrokModel, GroqModel, LlmProvider, MiniMaxModel,
    ModelInfo, StreamEvent,
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

/// Maximum time a stream attempt may produce **zero** events before the
/// `start_streaming` watchdog forces a `StreamEvent::Error` (→ retry on a fresh
/// connection).
///
/// This is the **catch-all** backstop, deliberately placed OUTSIDE the provider's
/// `do_stream` so it fires regardless of *where* a silent hold occurs — the
/// `.send()` headers wait, the SSE body read, a TLS/DNS stall, an OAuth refresh,
/// or any unguarded blocking call the three in-provider watchdogs
/// (`send_with_header_timeout` 60s, `FIRST_CONTENT_TIMEOUT` 90s,
/// `IdleTimeoutReader` 120s) cannot see. Observed in the field: a cold-start
/// stream froze for minutes producing **no** events while every in-provider guard
/// silently failed to fire. A pure wall-clock timer on time-to-first-event cannot
/// be evaded by any internal block. A healthy stream emits its first event
/// (usage / first delta) within seconds even under model queueing, and a
/// legitimately long stream has already emitted events (disarming the watchdog),
/// so this never cuts a real response.
const STREAM_TTFB_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(100);

/// Start streaming with the specified provider and model
pub(crate) fn start_streaming(params: StreamParams, tx: Sender<StreamEvent>) {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let client = get_client(params.provider);

    // ── Catch-all time-to-first-event watchdog ──
    // The provider writes events into `inner_tx`; a relay forwards them to the
    // real `tx`, flipping `got_event` on the first one. A watchdog thread injects
    // a StreamError if no event arrives within STREAM_TTFB_TIMEOUT. This sits
    // entirely outside the provider, so it catches ANY internal silent hold.
    let (inner_tx, inner_rx) = std::sync::mpsc::channel::<StreamEvent>();
    let got_event = Arc::new(AtomicBool::new(false));
    let aborted = Arc::new(AtomicBool::new(false));

    // Watchdog tx clone made first so the relay can consume the original `tx`
    // by move (avoids a redundant final clone / needless-pass-by-value lint).
    let watchdog_tx = tx.clone();

    // Relay: inner_rx → tx, marking first event; drops events once aborted so a
    // late-waking stuck stream can't interleave with the post-retry stream.
    {
        let got_event = Arc::clone(&got_event);
        let aborted = Arc::clone(&aborted);
        let _relay = std::thread::spawn(move || {
            while let Ok(evt) = inner_rx.recv() {
                got_event.store(true, Ordering::Relaxed);
                if aborted.load(Ordering::Relaxed) {
                    continue; // watchdog already forced a retry; discard stragglers
                }
                if tx.send(evt).is_err() {
                    break; // consumer gone
                }
            }
        });
    }

    // Watchdog: force a retry if the stream produced nothing in time.
    {
        let got_event = Arc::clone(&got_event);
        let aborted = Arc::clone(&aborted);
        let _watchdog = std::thread::spawn(move || {
            std::thread::sleep(STREAM_TTFB_TIMEOUT);
            if !got_event.load(Ordering::Relaxed) {
                aborted.store(true, Ordering::Relaxed);
                let _r = watchdog_tx.send(StreamEvent::Error(format!(
                    "stream produced no event within {}s (start_streaming catch-all watchdog) — \
                     aborting to retry on a fresh connection (silent hold no in-provider guard caught)",
                    STREAM_TTFB_TIMEOUT.as_secs()
                )));
            }
        });
    }

    let _r = std::thread::spawn(move || {
        // Assemble the prompt (panels + seed + conversation → api_messages)
        let include_tool_uses = false; // No pending tool results on first stream
        let api_messages = crate::app::prompt_builder::assemble_prompt(
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

        if let Err(e) = client.stream(request, inner_tx.clone()) {
            let _r = inner_tx.send(StreamEvent::Error(e.to_string()));
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
    cp_mod_utilities::time::epoch_ms_to_rfc3339(ms_i64).unwrap_or_else(|| "1970-01-01T00:00:00Z".to_string())
}

/// Convert milliseconds since UNIX epoch to date-only format (YYYY-MM-DD).
fn ms_to_date(ms: u64) -> String {
    let ms_i64 = i64::try_from(ms).unwrap_or(i64::MAX);
    cp_mod_utilities::time::epoch_ms_to_utc_date(ms_i64).unwrap_or_else(|| "1970-01-01".to_string())
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

/// Result of converting `ApiMessage`s to Claude Code JSON format.
///
/// Bundles the JSON messages with cache engine metadata for post-request bookkeeping.
pub(crate) struct CcJsonResult {
    /// Raw JSON messages ready for the Claude Code API.
    pub json_messages: Vec<Value>,
    /// Accumulated hashes at the 4 breakpoint positions (for `record_breakpoints`).
    pub bp_hashes: Vec<String>,
    /// How many stored BPs matched the current request's hash chain.
    pub alive_count: usize,
    /// Per-mille positions (0–1000) of alive BPs within the prompt.
    pub alive_positions_permille: Vec<u16>,
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
                            let _prev = o.insert("cache_control".to_string(), serde_json::json!({"type": "ephemeral"}));
                        }
                        obj
                    }
                    ContentBlock::ToolUse { id, name, input } => {
                        let mut obj = serde_json::json!({"type": "tool_use", "id": id, "name": name, "input": input});
                        if should_tag && let Some(o) = obj.as_object_mut() {
                            let _prev = o.insert("cache_control".to_string(), serde_json::json!({"type": "ephemeral"}));
                        }
                        obj
                    }
                    ContentBlock::ToolResult { tool_use_id, content } => {
                        let mut obj =
                            serde_json::json!({"type": "tool_result", "tool_use_id": tool_use_id, "content": content});
                        if should_tag && let Some(o) = obj.as_object_mut() {
                            let _prev = o.insert("cache_control".to_string(), serde_json::json!({"type": "ephemeral"}));
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

    CcJsonResult {
        json_messages,
        bp_hashes: plan.bp_hashes,
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
