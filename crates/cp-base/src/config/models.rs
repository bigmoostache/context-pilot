//! Per-provider model enums with [`ModelInfo`](super::llm_types::ModelInfo) impls.
//!
//! Each enum represents the available models for one LLM provider,
//! carrying API name, display name, context window, and pricing info.

use super::llm_types::ModelInfo;

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
        200_000
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
            Self::Grok41Fast | Self::Grok4Fast => 0.20,
        }
    }

    fn output_price_per_mtok(&self) -> f32 {
        match self {
            Self::Grok41Fast | Self::Grok4Fast => 0.50,
        }
    }

    fn max_output_tokens(&self) -> u32 {
        128_000
    }

    fn cache_hit_price_per_mtok(&self) -> f32 {
        self.input_price_per_mtok()
    }

    fn cache_miss_price_per_mtok(&self) -> f32 {
        self.input_price_per_mtok()
    }
}

/// Groq inference platform models.
///
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
        0x0002_0000
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
        self.input_price_per_mtok()
    }

    fn cache_miss_price_per_mtok(&self) -> f32 {
        self.input_price_per_mtok()
    }

    fn max_output_tokens(&self) -> u32 {
        128_000
    }
}

/// `DeepSeek` V4 model variants (OpenAI + Anthropic-compatible API).
///
/// Both variants support thinking and non-thinking modes via the API.
/// The legacy `deepseek-chat` / `deepseek-reasoner` names are deprecated
/// aliases for V4 Flash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeepSeekModel {
    /// `DeepSeek` V4 Flash — fast and cheap, 1M context.
    #[default]
    V4Flash,
    /// `DeepSeek` V4 Pro — higher capability, 1M context (75% launch discount until 2026-05-31).
    V4Pro,
}

impl ModelInfo for DeepSeekModel {
    fn api_name(&self) -> &'static str {
        match self {
            Self::V4Flash => "deepseek-v4-flash",
            Self::V4Pro => "deepseek-v4-pro",
        }
    }

    fn display_name(&self) -> &'static str {
        match self {
            Self::V4Flash => "V4 Flash",
            Self::V4Pro => "V4 Pro",
        }
    }

    fn context_window(&self) -> usize {
        1_000_000
    }

    fn input_price_per_mtok(&self) -> f32 {
        match self {
            Self::V4Flash => 0.14,
            Self::V4Pro => 0.435,
        }
    }

    fn output_price_per_mtok(&self) -> f32 {
        match self {
            Self::V4Flash => 0.28,
            Self::V4Pro => 0.87,
        }
    }

    fn cache_hit_price_per_mtok(&self) -> f32 {
        match self {
            Self::V4Flash => 0.0028,
            Self::V4Pro => 0.003_625,
        }
    }

    fn cache_miss_price_per_mtok(&self) -> f32 {
        match self {
            Self::V4Flash => 0.14,
            Self::V4Pro => 0.435,
        }
    }

    fn max_output_tokens(&self) -> u32 {
        384_000
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

/// Claude Code V2 model variants (OAuth, updated request format).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClaudeCodeV2Model {
    /// Claude Opus 4.8 — latest flagship model.
    #[default]
    ClaudeOpus48,
    /// Claude Fable 5 — premium tier, 1M context.
    ClaudeFable5,
    /// Claude Sonnet 4.6 — balanced cost / capability, 1M context.
    ClaudeSonnet46,
}

impl ModelInfo for ClaudeCodeV2Model {
    fn api_name(&self) -> &'static str {
        match self {
            Self::ClaudeOpus48 => "claude-opus-4-8",
            Self::ClaudeFable5 => "claude-fable-5",
            Self::ClaudeSonnet46 => "claude-sonnet-4-6",
        }
    }

    fn display_name(&self) -> &'static str {
        match self {
            Self::ClaudeOpus48 => "Opus 4.8",
            Self::ClaudeFable5 => "Fable 5",
            Self::ClaudeSonnet46 => "Sonnet 4.6",
        }
    }

    fn context_window(&self) -> usize {
        match self {
            Self::ClaudeOpus48 => 200_000,
            Self::ClaudeFable5 => 400_000,
            Self::ClaudeSonnet46 => 1_000_000,
        }
    }

    fn input_price_per_mtok(&self) -> f32 {
        match self {
            Self::ClaudeOpus48 => 5.0,
            Self::ClaudeFable5 => 10.0,
            Self::ClaudeSonnet46 => 3.0,
        }
    }

    fn output_price_per_mtok(&self) -> f32 {
        match self {
            Self::ClaudeOpus48 => 25.0,
            Self::ClaudeFable5 => 50.0,
            Self::ClaudeSonnet46 => 15.0,
        }
    }

    fn cache_hit_price_per_mtok(&self) -> f32 {
        match self {
            Self::ClaudeOpus48 => 0.50,
            Self::ClaudeFable5 => 1.0,
            Self::ClaudeSonnet46 => 0.30,
        }
    }

    fn cache_miss_price_per_mtok(&self) -> f32 {
        match self {
            Self::ClaudeOpus48 => 6.25,
            Self::ClaudeFable5 => 12.50,
            Self::ClaudeSonnet46 => 3.75,
        }
    }

    fn max_output_tokens(&self) -> u32 {
        match self {
            Self::ClaudeOpus48 | Self::ClaudeFable5 | Self::ClaudeSonnet46 => 64_000,
        }
    }
}
