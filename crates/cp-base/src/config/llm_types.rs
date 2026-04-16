//! LLM provider type definitions and model metadata.
//!
//! Contains enums, traits, and structs shared across the crate boundary.
//! Does NOT include client implementations or streaming logic.

use crate::tools::ToolUse;

/// Events emitted by the LLM during streaming.
#[derive(Debug)]
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
    },
    /// Unrecoverable error during streaming.
    Error(String),
}

/// Result of an LLM provider API connectivity check.
#[derive(Debug, Clone)]
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
        self.input_price_per_mtok() * 0.1
    }
    /// Cache write/miss price per million tokens in USD (default: 1.25x input)
    fn cache_miss_price_per_mtok(&self) -> f32 {
        self.input_price_per_mtok() * 1.25
    }
    /// Maximum output tokens the model can produce in a single response
    fn max_output_tokens(&self) -> u32;
}

/// Supported LLM provider backends. Each variant maps to a distinct
/// API client, auth flow, and model roster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
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
}

/// Anthropic model variants with per-model pricing and context limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AnthropicModel {
    /// Claude Opus 4.5 — highest capability, largest output window.
    #[default]
    ClaudeOpus45,
    /// Claude Sonnet 4.5 — balanced cost / capability.
    ClaudeSonnet45,
    /// Claude Haiku 4.5 — fast and cheap.
    ClaudeHaiku45,
}

impl ModelInfo for AnthropicModel {
    fn api_name(&self) -> &'static str {
        match self {
            Self::ClaudeOpus45 => "claude-opus-4-6",
            Self::ClaudeSonnet45 => "claude-sonnet-4-5-20250929",
            Self::ClaudeHaiku45 => "claude-haiku-4-5-20251001",
        }
    }

    fn display_name(&self) -> &'static str {
        match self {
            Self::ClaudeOpus45 => "Opus 4.6",
            Self::ClaudeSonnet45 => "Sonnet 4.5",
            Self::ClaudeHaiku45 => "Haiku 4.5",
        }
    }

    fn context_window(&self) -> usize {
        200_000 // All current Anthropic models: 200K context window
    }

    fn input_price_per_mtok(&self) -> f32 {
        match self {
            Self::ClaudeOpus45 => 5.0,
            Self::ClaudeSonnet45 => 3.0,
            Self::ClaudeHaiku45 => 1.0,
        }
    }

    fn output_price_per_mtok(&self) -> f32 {
        match self {
            Self::ClaudeOpus45 => 25.0,
            Self::ClaudeSonnet45 => 15.0,
            Self::ClaudeHaiku45 => 5.0,
        }
    }

    fn cache_hit_price_per_mtok(&self) -> f32 {
        match self {
            Self::ClaudeOpus45 => 0.50,
            Self::ClaudeSonnet45 => 0.30,
            Self::ClaudeHaiku45 => 0.10,
        }
    }

    fn cache_miss_price_per_mtok(&self) -> f32 {
        match self {
            Self::ClaudeOpus45 => 6.25,
            Self::ClaudeSonnet45 => 3.75,
            Self::ClaudeHaiku45 => 1.25,
        }
    }

    fn max_output_tokens(&self) -> u32 {
        match self {
            Self::ClaudeOpus45 => 128_000,
            Self::ClaudeSonnet45 | Self::ClaudeHaiku45 => 64_000,
        }
    }
}

/// xAI Grok model variants (fast models optimized for tool calling).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GrokModel {
    /// Grok 4.1 Fast — latest iteration, 2M context.
    #[default]
    Grok41Fast,
    /// Grok 4 Fast — previous generation, same 2M context.
    Grok4Fast,
}

impl ModelInfo for GrokModel {
    fn api_name(&self) -> &'static str {
        match self {
            Self::Grok41Fast => "grok-4-1-fast",
            Self::Grok4Fast => "grok-4-fast",
        }
    }

    fn display_name(&self) -> &'static str {
        match self {
            Self::Grok41Fast => "Grok 4.1 Fast",
            Self::Grok4Fast => "Grok 4 Fast",
        }
    }

    fn context_window(&self) -> usize {
        match self {
            Self::Grok41Fast | Self::Grok4Fast => 2_000_000,
        }
    }

    fn input_price_per_mtok(&self) -> f32 {
        match self {
            Self::Grok41Fast | Self::Grok4Fast => 0.20, // $0.20/1M input
        }
    }

    fn output_price_per_mtok(&self) -> f32 {
        match self {
            Self::Grok41Fast | Self::Grok4Fast => 0.50, // $0.50/1M output
        }
    }

    fn max_output_tokens(&self) -> u32 {
        128_000
    }

    fn cache_hit_price_per_mtok(&self) -> f32 {
        // Grok has no prompt caching — fall back to input price
        self.input_price_per_mtok()
    }

    fn cache_miss_price_per_mtok(&self) -> f32 {
        // Grok has no prompt caching — fall back to input price
        self.input_price_per_mtok()
    }
}
/// - GPT-OSS models: Support BOTH custom tools AND built-in tools (browser search, code exec)
/// - Llama models: Custom tools only
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GroqModel {
    /// GPT-OSS 120B — large, with built-in web search.
    #[default]
    GptOss120b,
    /// GPT-OSS 20B — small, with built-in web search.
    GptOss20b,
    /// Llama 3.3 70B Versatile — open-source, custom tools only.
    Llama33_70b,
    /// Llama 3.1 8B Instant — fastest, custom tools only.
    Llama31_8b,
}

impl ModelInfo for GroqModel {
    fn api_name(&self) -> &'static str {
        match self {
            Self::GptOss120b => "openai/gpt-oss-120b",
            Self::GptOss20b => "openai/gpt-oss-20b",
            Self::Llama33_70b => "llama-3.3-70b-versatile",
            Self::Llama31_8b => "llama-3.1-8b-instant",
        }
    }

    fn display_name(&self) -> &'static str {
        match self {
            Self::GptOss120b => "GPT-OSS 120B (+web)",
            Self::GptOss20b => "GPT-OSS 20B (+web)",
            Self::Llama33_70b => "Llama 3.3 70B",
            Self::Llama31_8b => "Llama 3.1 8B",
        }
    }

    fn context_window(&self) -> usize {
        0x0002_0000 // All models have 131K context
    }

    fn input_price_per_mtok(&self) -> f32 {
        match self {
            Self::GptOss120b => 1.20,
            Self::GptOss20b => 0.20,
            Self::Llama33_70b => 0.59,
            Self::Llama31_8b => 0.05,
        }
    }

    fn output_price_per_mtok(&self) -> f32 {
        match self {
            Self::GptOss120b => 1.20,
            Self::GptOss20b => 0.20,
            Self::Llama33_70b => 0.79,
            Self::Llama31_8b => 0.08,
        }
    }

    fn cache_hit_price_per_mtok(&self) -> f32 {
        // Groq has no prompt caching — fall back to input price
        self.input_price_per_mtok()
    }

    fn cache_miss_price_per_mtok(&self) -> f32 {
        // Groq has no prompt caching — fall back to input price
        self.input_price_per_mtok()
    }

    fn max_output_tokens(&self) -> u32 {
        128_000
    }
}

/// `DeepSeek` model variants (OpenAI-compatible API, budget-friendly).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeepSeekModel {
    /// `DeepSeek` Chat — general-purpose, 128K context.
    #[default]
    DeepseekChat,
    /// `DeepSeek` Reasoner — chain-of-thought, larger output.
    DeepseekReasoner,
}

impl ModelInfo for DeepSeekModel {
    fn api_name(&self) -> &'static str {
        match self {
            Self::DeepseekChat => "deepseek-chat",
            Self::DeepseekReasoner => "deepseek-reasoner",
        }
    }

    fn display_name(&self) -> &'static str {
        match self {
            Self::DeepseekChat => "DeepSeek Chat",
            Self::DeepseekReasoner => "DeepSeek Reasoner",
        }
    }

    fn context_window(&self) -> usize {
        128_000
    }

    fn input_price_per_mtok(&self) -> f32 {
        0.28
    }

    fn output_price_per_mtok(&self) -> f32 {
        0.42
    }

    fn cache_hit_price_per_mtok(&self) -> f32 {
        0.028
    }

    fn cache_miss_price_per_mtok(&self) -> f32 {
        0.28
    }

    fn max_output_tokens(&self) -> u32 {
        match self {
            Self::DeepseekChat => 8_192,
            Self::DeepseekReasoner => 0x4000,
        }
    }
}

/// `MiniMax` model variants (Anthropic-compatible API via Token Plan).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MiniMaxModel {
    /// `MiniMax` M2.7 — flagship model, 204K context.
    #[default]
    M27,
    /// `MiniMax` M2.7 Highspeed — faster variant, same context window.
    M27Highspeed,
}

impl ModelInfo for MiniMaxModel {
    fn api_name(&self) -> &'static str {
        match self {
            Self::M27 | Self::M27Highspeed => "MiniMax-M2.7",
        }
    }

    fn display_name(&self) -> &'static str {
        match self {
            Self::M27 => "M2.7",
            Self::M27Highspeed => "M2.7 HS",
        }
    }

    fn context_window(&self) -> usize {
        match self {
            Self::M27 => 204_800,
            Self::M27Highspeed => 0x2_0000,
        }
    }

    fn input_price_per_mtok(&self) -> f32 {
        match self {
            Self::M27 => 2.0,
            Self::M27Highspeed => 4.0,
        }
    }

    fn output_price_per_mtok(&self) -> f32 {
        match self {
            Self::M27 => 8.0,
            Self::M27Highspeed => 16.0,
        }
    }

    fn cache_hit_price_per_mtok(&self) -> f32 {
        match self {
            Self::M27 => 0.2,
            Self::M27Highspeed => 0.4,
        }
    }

    fn cache_miss_price_per_mtok(&self) -> f32 {
        match self {
            Self::M27 => 2.5,
            Self::M27Highspeed => 5.0,
        }
    }

    fn max_output_tokens(&self) -> u32 {
        match self {
            Self::M27 | Self::M27Highspeed => 128_000,
        }
    }
}
