//! LLM provider abstraction layer.
//!
//! Provides a unified interface for different LLM providers (Anthropic, Grok, Groq, Claude Code OAuth)

pub(crate) mod anthropic;
pub(crate) mod cache_engine;
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
        let api_messages = crate::app::prompt_builder::assemble_prompt(
            &params.messages,
            &params.context_items,
            include_tool_uses,
            params.seed_content.as_deref(),
        );

        // Dump prompt tick CSV for debugging cache behavior
        dump_prompt_tick_csv(&api_messages);

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
    let mut engine = engine_json.map_or_else(cache_engine::CacheEngine::default, cache_engine::CacheEngine::from_json);
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

// ─── Prompt Tick CSV Dumper ─────────────────────────────────────────────────

/// Dump every message in the assembled prompt to a CSV file for debugging.
///
/// Each tick writes a new file to `.context-pilot/prompt_ticks/` named by
/// datetime (second precision). Rolling deletion keeps only the 20 most recent.
fn dump_prompt_tick_csv(api_messages: &[ApiMessage]) {
    struct CsvRow {
        hash: String,
        role: String,
        block_type: &'static str,
        context: String,
        preview: String,
        tokens: usize,
    }

    let mut row_data: Vec<CsvRow> = Vec::new();

    let dir = std::path::Path::new(".context-pilot").join("prompt_ticks");
    let _mkdir = std::fs::create_dir_all(&dir);

    // Rolling cleanup: keep only 20 most recent CSVs
    if let Ok(mut entries) = std::fs::read_dir(&dir) {
        let mut files: Vec<std::path::PathBuf> = entries
            .by_ref()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("csv"))
            .collect();
        if files.len() >= 20 {
            files.sort();
            for old in files.iter().take(files.len().saturating_sub(19)) {
                let _del = std::fs::remove_file(old);
            }
        }
    }

    // Filename: datetime with second precision
    let ts = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
    let path = dir.join(format!("{ts}.csv"));

    for msg in api_messages {
        for block in &msg.content {
            let (block_type, context, raw_text) = match block {
                ContentBlock::Text { text } => {
                    let ctx = classify_text_context(text, &msg.role);
                    ("text", ctx, text.as_str())
                }
                ContentBlock::ToolUse { id, name, .. } => {
                    let ctx =
                        if name == "dynamic_panel" { format!("panel_call:{id}") } else { format!("tool_use:{name}") };
                    ("tool_use", ctx, name.as_str())
                }
                ContentBlock::ToolResult { tool_use_id, content } => {
                    let ctx = if tool_use_id.starts_with("panel_") {
                        let panel_info = content
                            .lines()
                            .next()
                            .unwrap_or("")
                            .trim_start_matches("======= [")
                            .split(']')
                            .next()
                            .unwrap_or(tool_use_id);
                        format!("panel_result:{panel_info}")
                    } else {
                        format!("tool_result:{tool_use_id}")
                    };
                    ("tool_result", ctx, content.as_str())
                }
            };

            let full_hash = crate::state::cache::hash_content(raw_text);
            let short_hash = full_hash.get(..16).unwrap_or(&full_hash).to_string();
            let tokens = cp_base::state::context::estimate_tokens(raw_text);

            let preview: String = raw_text
                .chars()
                .take(60)
                .map(|c| if c == ',' || c == '\n' || c == '\r' || c == '"' { ' ' } else { c })
                .collect();

            row_data.push(CsvRow { hash: short_hash, role: msg.role.clone(), block_type, context, preview, tokens });
        }
    }

    // Second pass: compute accumulated and reverse-accumulated token counts
    let total_tokens: usize = row_data.iter().map(|r| r.tokens).sum();
    let mut acc: usize = 0;
    let mut rows: Vec<String> = vec!["hash,role,type,context,tokens,acc_tokens,rev_acc_tokens,preview".to_string()];

    for row in &row_data {
        acc = acc.saturating_add(row.tokens);
        let rev_acc = total_tokens.saturating_sub(acc);
        rows.push(format!(
            "{},{},{},{},{},{},{},{}",
            row.hash, row.role, row.block_type, row.context, row.tokens, acc, rev_acc, row.preview
        ));
    }

    let csv_content = rows.join("\n");
    let _write = std::fs::write(&path, csv_content.as_bytes());
}

/// Classify a text block's context based on content and role.
fn classify_text_context(text: &str, role: &str) -> String {
    // Panel header (first text in the panel injection sequence)
    if text.contains("Beginning of dynamic panel display") {
        return "panel_header".to_string();
    }
    // Panel timestamp lines
    if text.starts_with("Panel automatically generated at") {
        return "panel_timestamp".to_string();
    }
    // Panel footer
    if text.contains("End of dynamic panel display") {
        return "panel_footer".to_string();
    }
    // Seed re-injection header
    if text.contains("System instructions") {
        return "seed_reinjection".to_string();
    }
    // Seed re-injection ack
    if role == "assistant" && text.contains("Understood") && text.len() < 100 {
        return "seed_ack".to_string();
    }
    // Footer ack
    if role == "user" && text.contains("Proceeding with conversation") {
        return "footer_ack".to_string();
    }
    // Conversation messages
    if role == "user" { "conversation:user".to_string() } else { "conversation:assistant".to_string() }
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
