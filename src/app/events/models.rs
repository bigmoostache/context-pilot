//! Config-overlay model selection dispatch.
//!
//! Extracted from [`crate::app::events`] to keep that file under the
//! 500-line structure limit. Translates a provider + letter-key index
//! (`a`/`b`/`c`/`d` → 0/1/2/3) into the concrete
//! `Action::ConfigSelect*Model` variant.

use crate::app::actions::Action;
use crate::llms::{AnthropicModel, ClaudeCodeV2Model, DeepSeekModel, GrokModel, GroqModel, LlmProvider, MiniMaxModel};
use crate::state::State;

/// Dispatch primary model selection based on provider and index (0=a, 1=b, 2=c, 3=d).
pub(super) const fn dispatch_primary_model(state: &State, idx: usize) -> Action {
    match state.llm_provider {
        LlmProvider::Anthropic | LlmProvider::ClaudeCode | LlmProvider::ClaudeCodeApiKey => anthropic_model(idx),
        LlmProvider::Grok => grok_model(idx),
        LlmProvider::Groq => groq_model(idx),
        LlmProvider::DeepSeek => deepseek_model(idx),
        LlmProvider::MiniMax => minimax_model(idx),
        LlmProvider::ClaudeCodeV2 => claude_code_v2_model(idx),
    }
}

/// Anthropic-family model for letter index (`a`/`b`/`c`).
const fn anthropic_model(idx: usize) -> Action {
    match idx {
        0 => Action::ConfigSelectAnthropicModel(AnthropicModel::ClaudeOpus45),
        1 => Action::ConfigSelectAnthropicModel(AnthropicModel::ClaudeSonnet45),
        2 => Action::ConfigSelectAnthropicModel(AnthropicModel::ClaudeHaiku45),
        _ => Action::None,
    }
}

/// Grok model for letter index (`a`/`b`).
const fn grok_model(idx: usize) -> Action {
    match idx {
        0 => Action::ConfigSelectGrokModel(GrokModel::Grok41Fast),
        1 => Action::ConfigSelectGrokModel(GrokModel::Grok4Fast),
        _ => Action::None,
    }
}

/// Groq model for letter index (`a`/`b`/`c`/`d`).
const fn groq_model(idx: usize) -> Action {
    match idx {
        0 => Action::ConfigSelectGroqModel(GroqModel::GptOss120b),
        1 => Action::ConfigSelectGroqModel(GroqModel::GptOss20b),
        2 => Action::ConfigSelectGroqModel(GroqModel::Llama33_70b),
        3 => Action::ConfigSelectGroqModel(GroqModel::Llama31_8b),
        _ => Action::None,
    }
}

/// `DeepSeek` model for letter index (`a`/`b`).
const fn deepseek_model(idx: usize) -> Action {
    match idx {
        0 => Action::ConfigSelectDeepSeekModel(DeepSeekModel::V4Flash),
        1 => Action::ConfigSelectDeepSeekModel(DeepSeekModel::V4Pro),
        _ => Action::None,
    }
}

/// `MiniMax` model for letter index (`a`/`b`).
const fn minimax_model(idx: usize) -> Action {
    match idx {
        0 => Action::ConfigSelectMiniMaxModel(MiniMaxModel::M27),
        1 => Action::ConfigSelectMiniMaxModel(MiniMaxModel::M27Highspeed),
        _ => Action::None,
    }
}

/// Claude Code V2 model for letter index (`a`/`b`/`c`).
const fn claude_code_v2_model(idx: usize) -> Action {
    match idx {
        0 => Action::ConfigSelectClaudeCodeV2Model(ClaudeCodeV2Model::ClaudeOpus48),
        1 => Action::ConfigSelectClaudeCodeV2Model(ClaudeCodeV2Model::ClaudeFable5),
        2 => Action::ConfigSelectClaudeCodeV2Model(ClaudeCodeV2Model::ClaudeSonnet46),
        _ => Action::None,
    }
}
