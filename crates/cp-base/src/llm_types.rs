//! LLM provider type definitions and model metadata.
//!
//! Contains enums, traits, and structs shared across the crate boundary.
//! Does NOT include client implementations or streaming logic.

use crate::tools::ToolUse;

/// Events emitted during streaming
#[derive(Debug)]
pub enum StreamEvent {
    /// Text chunk from the response
    Chunk(String),
    /// Tool use request from the LLM
    ToolUse(ToolUse),
    /// Stream completed with token usage
    Done {
        input_tokens: usize,
        output_tokens: usize,
        cache_hit_tokens: usize,
        cache_miss_tokens: usize,
        stop_reason: Option<String>,
    },
    /// Error occurred
    Error(String),
}

/// Result of API check
#[derive(Debug, Clone)]
pub struct ApiCheckResult {
    pub auth_ok: bool,
    pub streaming_ok: bool,
    pub tools_ok: bool,
    pub error: Option<String>,
}

impl ApiCheckResult {
    pub fn all_ok(&self) -> bool {
        self.auth_ok && self.streaming_ok && self.tools_ok
    }
}

/// Model metadata trait for context window and pricing info
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
}

/// Available LLM providers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LlmProvider {
    #[default]
    Anthropic,
    #[serde(alias = "claudecode")]
    ClaudeCode,
    Grok,
    Groq,
    DeepSeek,
}

/// Available models for Anthropic
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AnthropicModel {
    #[default]
    ClaudeOpus45,
    ClaudeSonnet45,
    ClaudeHaiku45,
}

impl ModelInfo for AnthropicModel {
    fn api_name(&self) -> &'static str {
        match self {
            AnthropicModel::ClaudeOpus45 => "claude-opus-4-5",
            AnthropicModel::ClaudeSonnet45 => "claude-sonnet-4-5",
            AnthropicModel::ClaudeHaiku45 => "claude-haiku-4-5",
        }
    }

    fn display_name(&self) -> &'static str {
        match self {
            AnthropicModel::ClaudeOpus45 => "Opus 4.5",
            AnthropicModel::ClaudeSonnet45 => "Sonnet 4.5",
            AnthropicModel::ClaudeHaiku45 => "Haiku 4.5",
        }
    }

    fn context_window(&self) -> usize {
        200_000
    }

    fn input_price_per_mtok(&self) -> f32 {
        match self {
            AnthropicModel::ClaudeOpus45 => 5.0,
            AnthropicModel::ClaudeSonnet45 => 3.0,
            AnthropicModel::ClaudeHaiku45 => 1.0,
        }
    }

    fn output_price_per_mtok(&self) -> f32 {
        match self {
            AnthropicModel::ClaudeOpus45 => 25.0,
            AnthropicModel::ClaudeSonnet45 => 15.0,
            AnthropicModel::ClaudeHaiku45 => 5.0,
        }
    }

    fn cache_hit_price_per_mtok(&self) -> f32 {
        match self {
            AnthropicModel::ClaudeOpus45 => 0.50,
            AnthropicModel::ClaudeSonnet45 => 0.30,
            AnthropicModel::ClaudeHaiku45 => 0.10,
        }
    }

    fn cache_miss_price_per_mtok(&self) -> f32 {
        match self {
            AnthropicModel::ClaudeOpus45 => 6.25,
            AnthropicModel::ClaudeSonnet45 => 3.75,
            AnthropicModel::ClaudeHaiku45 => 1.25,
        }
    }
}

/// Available models for Grok (fast models optimized for tool calling)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GrokModel {
    #[default]
    Grok41Fast,
    Grok4Fast,
}

impl ModelInfo for GrokModel {
    fn api_name(&self) -> &'static str {
        match self {
            GrokModel::Grok41Fast => "grok-4-1-fast",
            GrokModel::Grok4Fast => "grok-4-fast",
        }
    }

    fn display_name(&self) -> &'static str {
        match self {
            GrokModel::Grok41Fast => "Grok 4.1 Fast",
            GrokModel::Grok4Fast => "Grok 4 Fast",
        }
    }

    fn context_window(&self) -> usize {
        match self {
            GrokModel::Grok41Fast => 2_000_000,
            GrokModel::Grok4Fast => 2_000_000,
        }
    }

    fn input_price_per_mtok(&self) -> f32 {
        match self {
            GrokModel::Grok41Fast => 0.20, // $0.20/1M input
            GrokModel::Grok4Fast => 0.20,
        }
    }

    fn output_price_per_mtok(&self) -> f32 {
        match self {
            GrokModel::Grok41Fast => 0.50, // $0.50/1M output
            GrokModel::Grok4Fast => 0.50,
        }
    }
}

/// Available models for Groq
/// - GPT-OSS models: Support BOTH custom tools AND built-in tools (browser search, code exec)
/// - Llama models: Custom tools only
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GroqModel {
    #[default]
    GptOss120b,
    GptOss20b,
    Llama33_70b,
    Llama31_8b,
}

impl ModelInfo for GroqModel {
    fn api_name(&self) -> &'static str {
        match self {
            GroqModel::GptOss120b => "openai/gpt-oss-120b",
            GroqModel::GptOss20b => "openai/gpt-oss-20b",
            GroqModel::Llama33_70b => "llama-3.3-70b-versatile",
            GroqModel::Llama31_8b => "llama-3.1-8b-instant",
        }
    }

    fn display_name(&self) -> &'static str {
        match self {
            GroqModel::GptOss120b => "GPT-OSS 120B (+web)",
            GroqModel::GptOss20b => "GPT-OSS 20B (+web)",
            GroqModel::Llama33_70b => "Llama 3.3 70B",
            GroqModel::Llama31_8b => "Llama 3.1 8B",
        }
    }

    fn context_window(&self) -> usize {
        131_072 // All models have 131K context
    }

    fn input_price_per_mtok(&self) -> f32 {
        match self {
            GroqModel::GptOss120b => 1.20,
            GroqModel::GptOss20b => 0.20,
            GroqModel::Llama33_70b => 0.59,
            GroqModel::Llama31_8b => 0.05,
        }
    }

    fn output_price_per_mtok(&self) -> f32 {
        match self {
            GroqModel::GptOss120b => 1.20,
            GroqModel::GptOss20b => 0.20,
            GroqModel::Llama33_70b => 0.79,
            GroqModel::Llama31_8b => 0.08,
        }
    }
}

/// Available models for DeepSeek
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeepSeekModel {
    #[default]
    DeepseekChat,
    DeepseekReasoner,
}

impl ModelInfo for DeepSeekModel {
    fn api_name(&self) -> &'static str {
        match self {
            DeepSeekModel::DeepseekChat => "deepseek-chat",
            DeepSeekModel::DeepseekReasoner => "deepseek-reasoner",
        }
    }

    fn display_name(&self) -> &'static str {
        match self {
            DeepSeekModel::DeepseekChat => "DeepSeek Chat",
            DeepSeekModel::DeepseekReasoner => "DeepSeek Reasoner",
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
}
