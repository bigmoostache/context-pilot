//! Config-overlay model selection dispatch.
//!
//! Extracted from [`crate::app::events`] to keep that file under the
//! 500-line structure limit. These two `const fn`s translate a provider +
//! letter-key index (`a`/`b`/`c`/`d` → 0/1/2/3) into the concrete
//! `Action::ConfigSelect*Model` variant, for the primary and secondary
//! (reverie) model slots respectively.

use crate::app::actions::Action;
use crate::llms::{AnthropicModel, ClaudeCodeV2Model, DeepSeekModel, GrokModel, GroqModel, LlmProvider, MiniMaxModel};
use crate::state::State;

/// Dispatch primary model selection based on provider and index (0=a, 1=b, 2=c, 3=d).
pub(super) const fn dispatch_primary_model(state: &State, idx: usize) -> Action {
    match state.llm_provider {
        LlmProvider::Anthropic | LlmProvider::ClaudeCode | LlmProvider::ClaudeCodeApiKey => match idx {
            0 => Action::ConfigSelectAnthropicModel(AnthropicModel::ClaudeOpus45),
            1 => Action::ConfigSelectAnthropicModel(AnthropicModel::ClaudeSonnet45),
            2 => Action::ConfigSelectAnthropicModel(AnthropicModel::ClaudeHaiku45),
            _ => Action::None,
        },
        LlmProvider::Grok => match idx {
            0 => Action::ConfigSelectGrokModel(GrokModel::Grok41Fast),
            1 => Action::ConfigSelectGrokModel(GrokModel::Grok4Fast),
            _ => Action::None,
        },
        LlmProvider::Groq => match idx {
            0 => Action::ConfigSelectGroqModel(GroqModel::GptOss120b),
            1 => Action::ConfigSelectGroqModel(GroqModel::GptOss20b),
            2 => Action::ConfigSelectGroqModel(GroqModel::Llama33_70b),
            3 => Action::ConfigSelectGroqModel(GroqModel::Llama31_8b),
            _ => Action::None,
        },
        LlmProvider::DeepSeek => match idx {
            0 => Action::ConfigSelectDeepSeekModel(DeepSeekModel::V4Flash),
            1 => Action::ConfigSelectDeepSeekModel(DeepSeekModel::V4Pro),
            _ => Action::None,
        },
        LlmProvider::MiniMax => match idx {
            0 => Action::ConfigSelectMiniMaxModel(MiniMaxModel::M27),
            1 => Action::ConfigSelectMiniMaxModel(MiniMaxModel::M27Highspeed),
            _ => Action::None,
        },
        LlmProvider::ClaudeCodeV2 => match idx {
            0 => Action::ConfigSelectClaudeCodeV2Model(ClaudeCodeV2Model::ClaudeOpus48),
            1 => Action::ConfigSelectClaudeCodeV2Model(ClaudeCodeV2Model::ClaudeFable5),
            2 => Action::ConfigSelectClaudeCodeV2Model(ClaudeCodeV2Model::ClaudeSonnet46),
            _ => Action::None,
        },
    }
}

/// Dispatch secondary model selection based on secondary provider and index.
pub(super) const fn dispatch_secondary_model(state: &State, idx: usize) -> Action {
    match state.secondary_provider {
        LlmProvider::Anthropic | LlmProvider::ClaudeCode | LlmProvider::ClaudeCodeApiKey => match idx {
            0 => Action::ConfigSelectSecondaryAnthropicModel(AnthropicModel::ClaudeOpus45),
            1 => Action::ConfigSelectSecondaryAnthropicModel(AnthropicModel::ClaudeSonnet45),
            2 => Action::ConfigSelectSecondaryAnthropicModel(AnthropicModel::ClaudeHaiku45),
            _ => Action::None,
        },
        LlmProvider::Grok => match idx {
            0 => Action::ConfigSelectSecondaryGrokModel(GrokModel::Grok41Fast),
            1 => Action::ConfigSelectSecondaryGrokModel(GrokModel::Grok4Fast),
            _ => Action::None,
        },
        LlmProvider::Groq => match idx {
            0 => Action::ConfigSelectSecondaryGroqModel(GroqModel::GptOss120b),
            1 => Action::ConfigSelectSecondaryGroqModel(GroqModel::GptOss20b),
            2 => Action::ConfigSelectSecondaryGroqModel(GroqModel::Llama33_70b),
            3 => Action::ConfigSelectSecondaryGroqModel(GroqModel::Llama31_8b),
            _ => Action::None,
        },
        LlmProvider::DeepSeek => match idx {
            0 => Action::ConfigSelectSecondaryDeepSeekModel(DeepSeekModel::V4Flash),
            1 => Action::ConfigSelectSecondaryDeepSeekModel(DeepSeekModel::V4Pro),
            _ => Action::None,
        },
        LlmProvider::MiniMax => match idx {
            0 => Action::ConfigSelectSecondaryMiniMaxModel(MiniMaxModel::M27),
            1 => Action::ConfigSelectSecondaryMiniMaxModel(MiniMaxModel::M27Highspeed),
            _ => Action::None,
        },
        LlmProvider::ClaudeCodeV2 => match idx {
            0 => Action::ConfigSelectSecondaryClaudeCodeV2Model(ClaudeCodeV2Model::ClaudeOpus48),
            1 => Action::ConfigSelectSecondaryClaudeCodeV2Model(ClaudeCodeV2Model::ClaudeFable5),
            2 => Action::ConfigSelectSecondaryClaudeCodeV2Model(ClaudeCodeV2Model::ClaudeSonnet46),
            _ => Action::None,
        },
    }
}
