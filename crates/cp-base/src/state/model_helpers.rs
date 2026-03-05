//! Model/provider dispatch helpers for [`State`].
//!
//! Extracted from `runtime.rs` to keep file size manageable.
//! All methods here delegate to the active provider's model enum.

use super::runtime::State;
use crate::cast::SafeCast as _;
use crate::llm_types::{LlmProvider, ModelInfo as _};

// =============================================================================
// Model selection & context window
// =============================================================================

impl State {
    /// Get the API model string for the current provider/model selection
    #[must_use]
    pub fn current_model(&self) -> String {
        match self.llm_provider {
            LlmProvider::Anthropic | LlmProvider::ClaudeCode | LlmProvider::ClaudeCodeApiKey => {
                self.anthropic_model.api_name().to_string()
            }
            LlmProvider::Grok => self.grok_model.api_name().to_string(),
            LlmProvider::Groq => self.groq_model.api_name().to_string(),
            LlmProvider::DeepSeek => self.deepseek_model.api_name().to_string(),
        }
    }

    /// Get the max output tokens for the current provider/model selection
    #[must_use]
    pub fn current_max_output_tokens(&self) -> u32 {
        match self.llm_provider {
            LlmProvider::Anthropic | LlmProvider::ClaudeCode | LlmProvider::ClaudeCodeApiKey => {
                self.anthropic_model.max_output_tokens()
            }
            LlmProvider::Grok => self.grok_model.max_output_tokens(),
            LlmProvider::Groq => self.groq_model.max_output_tokens(),
            LlmProvider::DeepSeek => self.deepseek_model.max_output_tokens(),
        }
    }

    /// Get the max output tokens for the secondary provider/model selection
    #[must_use]
    pub fn secondary_max_output_tokens(&self) -> u32 {
        match self.secondary_provider {
            LlmProvider::Anthropic | LlmProvider::ClaudeCode | LlmProvider::ClaudeCodeApiKey => {
                self.secondary_anthropic_model.max_output_tokens()
            }
            LlmProvider::Grok => self.secondary_grok_model.max_output_tokens(),
            LlmProvider::Groq => self.secondary_groq_model.max_output_tokens(),
            LlmProvider::DeepSeek => self.secondary_deepseek_model.max_output_tokens(),
        }
    }

    /// Get the current model's context window
    #[must_use]
    pub fn model_context_window(&self) -> usize {
        match self.llm_provider {
            LlmProvider::Anthropic | LlmProvider::ClaudeCode | LlmProvider::ClaudeCodeApiKey => {
                self.anthropic_model.context_window()
            }
            LlmProvider::Grok => self.grok_model.context_window(),
            LlmProvider::Groq => self.groq_model.context_window(),
            LlmProvider::DeepSeek => self.deepseek_model.context_window(),
        }
    }

    /// Get effective context budget (custom or model's full context)
    #[must_use]
    pub fn effective_context_budget(&self) -> usize {
        self.context_budget.unwrap_or_else(|| self.model_context_window())
    }
}

// =============================================================================
// Cleaning thresholds
// =============================================================================

impl State {
    /// Get the cleaning target as absolute proportion (threshold × `target_proportion`)
    #[must_use]
    pub fn cleaning_target(&self) -> f32 {
        self.cleaning_threshold * self.cleaning_target_proportion
    }

    /// Get cleaning threshold in tokens
    #[must_use]
    pub fn cleaning_threshold_tokens(&self) -> usize {
        (self.effective_context_budget().to_f32() * self.cleaning_threshold).to_usize()
    }

    /// Get cleaning target in tokens
    #[must_use]
    pub fn cleaning_target_tokens(&self) -> usize {
        (self.effective_context_budget().to_f32() * self.cleaning_target()).to_usize()
    }
}

// =============================================================================
// Pricing
// =============================================================================

impl State {
    /// Get cache hit price per million tokens for the current model
    #[must_use]
    pub fn cache_hit_price_per_mtok(&self) -> f32 {
        match self.llm_provider {
            LlmProvider::Anthropic | LlmProvider::ClaudeCode | LlmProvider::ClaudeCodeApiKey => {
                self.anthropic_model.cache_hit_price_per_mtok()
            }
            LlmProvider::Grok => self.grok_model.cache_hit_price_per_mtok(),
            LlmProvider::Groq => self.groq_model.cache_hit_price_per_mtok(),
            LlmProvider::DeepSeek => self.deepseek_model.cache_hit_price_per_mtok(),
        }
    }

    /// Get cache miss price per million tokens for the current model
    #[must_use]
    pub fn cache_miss_price_per_mtok(&self) -> f32 {
        match self.llm_provider {
            LlmProvider::Anthropic | LlmProvider::ClaudeCode | LlmProvider::ClaudeCodeApiKey => {
                self.anthropic_model.cache_miss_price_per_mtok()
            }
            LlmProvider::Grok => self.grok_model.cache_miss_price_per_mtok(),
            LlmProvider::Groq => self.groq_model.cache_miss_price_per_mtok(),
            LlmProvider::DeepSeek => self.deepseek_model.cache_miss_price_per_mtok(),
        }
    }

    /// Get output price per million tokens for the current model
    #[must_use]
    pub fn output_price_per_mtok(&self) -> f32 {
        match self.llm_provider {
            LlmProvider::Anthropic | LlmProvider::ClaudeCode | LlmProvider::ClaudeCodeApiKey => {
                self.anthropic_model.output_price_per_mtok()
            }
            LlmProvider::Grok => self.grok_model.output_price_per_mtok(),
            LlmProvider::Groq => self.groq_model.output_price_per_mtok(),
            LlmProvider::DeepSeek => self.deepseek_model.output_price_per_mtok(),
        }
    }

    /// Calculate cost in USD for a given token count and price per `MTok`
    #[must_use]
    pub fn token_cost(tokens: usize, price_per_mtok: f32) -> f64 {
        tokens.to_f64() * price_per_mtok.to_f64() / 1_000_000.0
    }
}
