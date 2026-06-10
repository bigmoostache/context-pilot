//! Model selection, pricing, and cleaning-threshold helpers for [`State`].
//!
//! Extracted from `runtime.rs` to keep it under the 500-line structure limit.
//! Uses a trait (`ModelPricing`) so the `impl` lives here without triggering
//! `clippy::multiple_inherent_impl`.

use super::super::runtime::State;
use crate::cast::Safe as _;
use crate::config::llm_types::{LlmProvider, ModelInfo as _};

/// Model-selection, pricing, and context-budget helpers for [`State`].
pub trait ModelPricing {
    /// API model string for the active provider/model selection.
    fn current_model(&self) -> String;
    /// Max output tokens for the active provider/model.
    fn current_max_output_tokens(&self) -> u32;
    /// Max output tokens for the secondary provider/model.
    fn secondary_max_output_tokens(&self) -> u32;
    /// Context window size (tokens) for the active model.
    fn model_context_window(&self) -> usize;
    /// Effective context budget: custom override or full context window.
    fn effective_context_budget(&self) -> usize;
    /// Cache-hit input price per million tokens.
    fn cache_hit_price_per_mtok(&self) -> f32;
    /// Cache-miss input price per million tokens.
    fn cache_miss_price_per_mtok(&self) -> f32;
    /// Output price per million tokens.
    fn output_price_per_mtok(&self) -> f32;
    /// Cleaning target as absolute proportion (threshold × target ratio).
    fn cleaning_target(&self) -> f32;
    /// Cleaning threshold in tokens.
    fn cleaning_threshold_tokens(&self) -> usize;
    /// Cleaning target in tokens.
    fn cleaning_target_tokens(&self) -> usize;
}

impl ModelPricing for State {
    fn current_model(&self) -> String {
        match self.llm_provider {
            LlmProvider::Anthropic | LlmProvider::ClaudeCode | LlmProvider::ClaudeCodeApiKey => {
                self.anthropic_model.api_name().to_string()
            }
            LlmProvider::Grok => self.grok_model.api_name().to_string(),
            LlmProvider::Groq => self.groq_model.api_name().to_string(),
            LlmProvider::DeepSeek => self.deepseek_model.api_name().to_string(),
            LlmProvider::MiniMax => self.minimax_model.api_name().to_string(),
            LlmProvider::ClaudeCodeV2 => self.claude_code_v2_model.api_name().to_string(),
        }
    }

    fn current_max_output_tokens(&self) -> u32 {
        match self.llm_provider {
            LlmProvider::Anthropic | LlmProvider::ClaudeCode | LlmProvider::ClaudeCodeApiKey => {
                self.anthropic_model.max_output_tokens()
            }
            LlmProvider::Grok => self.grok_model.max_output_tokens(),
            LlmProvider::Groq => self.groq_model.max_output_tokens(),
            LlmProvider::DeepSeek => self.deepseek_model.max_output_tokens(),
            LlmProvider::MiniMax => self.minimax_model.max_output_tokens(),
            LlmProvider::ClaudeCodeV2 => self.claude_code_v2_model.max_output_tokens(),
        }
    }

    fn secondary_max_output_tokens(&self) -> u32 {
        match self.secondary_provider {
            LlmProvider::Anthropic | LlmProvider::ClaudeCode | LlmProvider::ClaudeCodeApiKey => {
                self.secondary_anthropic_model.max_output_tokens()
            }
            LlmProvider::Grok => self.secondary_grok_model.max_output_tokens(),
            LlmProvider::Groq => self.secondary_groq_model.max_output_tokens(),
            LlmProvider::DeepSeek => self.secondary_deepseek_model.max_output_tokens(),
            LlmProvider::MiniMax => self.secondary_minimax_model.max_output_tokens(),
            LlmProvider::ClaudeCodeV2 => self.secondary_claude_code_v2_model.max_output_tokens(),
        }
    }

    fn model_context_window(&self) -> usize {
        match self.llm_provider {
            LlmProvider::Anthropic | LlmProvider::ClaudeCode | LlmProvider::ClaudeCodeApiKey => {
                self.anthropic_model.context_window()
            }
            LlmProvider::Grok => self.grok_model.context_window(),
            LlmProvider::Groq => self.groq_model.context_window(),
            LlmProvider::DeepSeek => self.deepseek_model.context_window(),
            LlmProvider::MiniMax => self.minimax_model.context_window(),
            LlmProvider::ClaudeCodeV2 => self.claude_code_v2_model.context_window(),
        }
    }

    fn effective_context_budget(&self) -> usize {
        self.context_budget.unwrap_or_else(|| self.model_context_window())
    }

    fn cache_hit_price_per_mtok(&self) -> f32 {
        match self.llm_provider {
            LlmProvider::Anthropic | LlmProvider::ClaudeCode | LlmProvider::ClaudeCodeApiKey => {
                self.anthropic_model.cache_hit_price_per_mtok()
            }
            LlmProvider::Grok => self.grok_model.cache_hit_price_per_mtok(),
            LlmProvider::Groq => self.groq_model.cache_hit_price_per_mtok(),
            LlmProvider::DeepSeek => self.deepseek_model.cache_hit_price_per_mtok(),
            LlmProvider::MiniMax => self.minimax_model.cache_hit_price_per_mtok(),
            LlmProvider::ClaudeCodeV2 => self.claude_code_v2_model.cache_hit_price_per_mtok(),
        }
    }

    fn cache_miss_price_per_mtok(&self) -> f32 {
        match self.llm_provider {
            LlmProvider::Anthropic | LlmProvider::ClaudeCode | LlmProvider::ClaudeCodeApiKey => {
                self.anthropic_model.cache_miss_price_per_mtok()
            }
            LlmProvider::Grok => self.grok_model.cache_miss_price_per_mtok(),
            LlmProvider::Groq => self.groq_model.cache_miss_price_per_mtok(),
            LlmProvider::DeepSeek => self.deepseek_model.cache_miss_price_per_mtok(),
            LlmProvider::MiniMax => self.minimax_model.cache_miss_price_per_mtok(),
            LlmProvider::ClaudeCodeV2 => self.claude_code_v2_model.cache_miss_price_per_mtok(),
        }
    }

    fn output_price_per_mtok(&self) -> f32 {
        match self.llm_provider {
            LlmProvider::Anthropic | LlmProvider::ClaudeCode | LlmProvider::ClaudeCodeApiKey => {
                self.anthropic_model.output_price_per_mtok()
            }
            LlmProvider::Grok => self.grok_model.output_price_per_mtok(),
            LlmProvider::Groq => self.groq_model.output_price_per_mtok(),
            LlmProvider::DeepSeek => self.deepseek_model.output_price_per_mtok(),
            LlmProvider::MiniMax => self.minimax_model.output_price_per_mtok(),
            LlmProvider::ClaudeCodeV2 => self.claude_code_v2_model.output_price_per_mtok(),
        }
    }

    fn cleaning_target(&self) -> f32 {
        self.cleaning_threshold * self.cleaning_target_proportion
    }

    fn cleaning_threshold_tokens(&self) -> usize {
        (self.effective_context_budget().to_f32() * self.cleaning_threshold).to_usize()
    }

    fn cleaning_target_tokens(&self) -> usize {
        (self.effective_context_budget().to_f32() * self.cleaning_target()).to_usize()
    }
}

/// Cost in USD for a given token count and price per million tokens.
#[must_use]
pub fn token_cost(tokens: usize, price_per_mtok: f32) -> f64 {
    tokens.to_f64() * price_per_mtok.to_f64() / 1_000_000.0
}
