//! LLM provider type definitions and model metadata.
//!
//! Contains enums, traits, and structs shared across the crate boundary.
//! Does NOT include client implementations or streaming logic.

use crate::tools::ToolUse;

/// Events emitted by the LLM during streaming.
#[derive(Debug)]
#[expect(
    clippy::exhaustive_enums,
    reason = "stream-event contract: StreamEvent is constructed by every LLM provider and matched exhaustively by the streaming pipeline; the variant set is closed and #[non_exhaustive] would forbid that cross-crate construction"
)]
pub enum StreamEvent {
    /// Text chunk from the response.
    Chunk(String),
    /// Advisory: a tool call is being streamed (name + partial JSON input so far).
    ///
    /// Pure UI hint — has no effect on execution. Cleared when the final
    /// [`ToolUse`](Self::ToolUse) arrives.
    ToolProgress {
        /// Tool name (available from `content_block_start`).
        name: String,
        /// Accumulated partial JSON input (grows with each `input_json_delta`).
        input_so_far: String,
    },
    /// Tool use request from the LLM.
    ToolUse(ToolUse),
    /// Stream completed with token usage.
    Done {
        /// Tokens consumed by the input prompt.
        input_tokens: usize,
        /// Tokens generated in the response.
        output_tokens: usize,
        /// Input tokens served from provider cache.
        cache_hit_tokens: usize,
        /// Input tokens that missed the cache (written on this call).
        cache_miss_tokens: usize,
        /// Provider stop reason (e.g., `"end_turn"`, `"tool_use"`).
        stop_reason: Option<String>,
        /// Accumulated hashes at breakpoint positions (for cache engine update).
        /// Only populated by providers that use the cache optimization engine.
        bp_hashes: Vec<String>,
        /// Panel IDs each breakpoint landed on, in prompt order (the BP→panel
        /// mapping consumed next turn by the freeze pass's free-region widening).
        /// Empty for non-caching providers.
        bp_panel_ids: Vec<String>,
        /// How many stored breakpoints were both non-expired AND matched in the
        /// current request's accumulated hash chain. Zero for non-caching providers.
        alive_count: usize,
        /// Per-mille positions (0–1000) of alive BPs within the prompt, sorted.
        /// Each value = `bp_cumulative_tokens * 1000 / total_tokens`.
        alive_positions_permille: Vec<u16>,
    },
    /// Unrecoverable error during streaming.
    Error(String),
}

/// Result of an LLM provider API connectivity check.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct ApiCheckResult {
    /// Whether authentication (API key / OAuth) succeeded.
    pub auth_ok: bool,
    /// Whether streaming responses work.
    pub streaming_ok: bool,
    /// Whether tool-use / function-calling works.
    pub tools_ok: bool,
    /// Human-readable error message, if any check failed.
    pub error: Option<String>,
}

impl ApiCheckResult {
    /// Total failure: no check passed, carrying the given error message.
    #[must_use]
    pub const fn failure(error: Option<String>) -> Self {
        Self { auth_ok: false, streaming_ok: false, tools_ok: false, error }
    }

    /// Build a result from the three probe outcomes (`[auth, streaming, tools]`),
    /// no error message. The shared tail of every provider's `check_api`. Takes
    /// the outcomes as an array — three separate `bool` params trip
    /// `fn_params_excessive_bools`.
    #[must_use]
    pub const fn checks([auth_ok, streaming_ok, tools_ok]: [bool; 3]) -> Self {
        Self { auth_ok, streaming_ok, tools_ok, error: None }
    }

    /// `true` only when auth, streaming, and tool-use all passed.
    #[must_use]
    pub const fn all_ok(&self) -> bool {
        self.auth_ok && self.streaming_ok && self.tools_ok
    }
}

/// Model metadata trait for context window and pricing info.
pub trait ModelInfo {
    /// API model identifier
    fn api_name(&self) -> &'static str;
    /// Human-readable display name
    fn display_name(&self) -> &'static str;
    /// Maximum context window in tokens
    fn context_window(&self) -> usize;
    /// Input price per million tokens in USD (used for cache miss / uncached input)
    fn input_price_per_mtok(&self) -> f32;
    /// Output price per million tokens in USD
    fn output_price_per_mtok(&self) -> f32;
    /// Cache hit price per million tokens in USD (default: same as input)
    fn cache_hit_price_per_mtok(&self) -> f32 {
        crate::cast::float_math::mul_f32(self.input_price_per_mtok(), 0.1)
    }
    /// Cache write/miss price per million tokens in USD (default: 1.25x input)
    fn cache_miss_price_per_mtok(&self) -> f32 {
        crate::cast::float_math::mul_f32(self.input_price_per_mtok(), 1.25)
    }
    /// Maximum output tokens the model can produce in a single response
    fn max_output_tokens(&self) -> u32;
}

/// Supported LLM provider backends. Each variant maps to a distinct
/// API client, auth flow, and model roster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
#[expect(
    clippy::exhaustive_enums,
    reason = "provider-roster contract: LlmProvider is a closed set constructed cross-crate by config/dispatch and matched exhaustively across the client factory and config UI; #[non_exhaustive] would forbid that construction"
)]
pub enum LlmProvider {
    /// Direct Anthropic Messages API (API-key auth).
    #[default]
    Anthropic,
    /// Claude Code CLI backend (OAuth-based, pipes through `cc` process).
    #[serde(alias = "claudecode")]
    ClaudeCode,
    /// Claude Code with explicit API key (bypasses OAuth).
    #[serde(alias = "claudecodeapikey")]
    ClaudeCodeApiKey,
    /// xAI Grok models (OpenAI-compatible API).
    Grok,
    /// Groq inference platform (OpenAI-compatible, very fast).
    Groq,
    /// `DeepSeek` models (OpenAI-compatible API).
    DeepSeek,
    /// `MiniMax` models (Anthropic-compatible API via Token Plan).
    MiniMax,
    /// Claude Code V2 (OAuth, updated request format with Opus 4.8).
    #[serde(alias = "claudecodev2")]
    ClaudeCodeV2,
}
