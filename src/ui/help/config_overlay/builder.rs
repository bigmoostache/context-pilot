//! Config overlay IR builder — assembles [`ConfigOverlay`] from application state.
//!
//! Extracted from the conversation IR builder for file-length compliance.
//! No ratatui, no `Frame` — pure state → data transformation.

use cp_render::Semantic;
use cp_render::conversation::{ConfigBudgetBar, ConfigModel, ConfigOverlay, ConfigProvider, ConfigToggle};

use crate::state::State;
use cp_base::cast::Safe as _;

/// Type alias for the model-entry builder closure (`clippy::type_complexity`).
type ModelEntryFn = dyn Fn(bool, &str, &dyn crate::llms::ModelInfo) -> ConfigModel;

/// Build the config overlay IR data from application state.
#[must_use]
pub(crate) fn build_config_overlay(state: &State) -> ConfigOverlay {
    use crate::llms::{LlmProvider, ModelInfo};

    let secondary_mode = state.flags.config.config_secondary_mode;
    let active_provider = if secondary_mode { state.secondary_provider } else { state.llm_provider };

    // Providers
    let provider_list: [(LlmProvider, &str, &str); 8] = [
        (LlmProvider::Anthropic, "1", "Anthropic Claude"),
        (LlmProvider::ClaudeCode, "2", "Claude Code (OAuth)"),
        (LlmProvider::ClaudeCodeApiKey, "6", "Claude Code (API Key)"),
        (LlmProvider::Grok, "3", "Grok (xAI)"),
        (LlmProvider::Groq, "4", "Groq"),
        (LlmProvider::DeepSeek, "5", "DeepSeek"),
        (LlmProvider::MiniMax, "7", "MiniMax (Token Plan)"),
        (LlmProvider::ClaudeCodeV2, "8", "Claude Code V2 (OAuth)"),
    ];

    let providers = provider_list
        .iter()
        .map(|(p, key, name)| ConfigProvider {
            key: (*key).into(),
            name: (*name).into(),
            selected: active_provider == *p,
        })
        .collect();

    // Models — helper closure to build a single entry
    let model_entry = |selected: bool, key: &str, model: &dyn ModelInfo| -> ConfigModel {
        ConfigModel {
            key: key.into(),
            name: model.display_name().to_owned(),
            context_window: crate::ui::helpers::format_number(model.context_window()),
            pricing: format!("${:.0}/${:.0}", model.input_price_per_mtok(), model.output_price_per_mtok()),
            selected,
        }
    };

    let (model_section_title, models) = build_models(state, &model_entry);

    // Budget bars
    let budget_bars = build_budget_bars(state);

    // Toggles
    let toggles = build_toggles(state);

    ConfigOverlay {
        secondary_mode,
        providers,
        model_section_title,
        models,
        budget_bars,
        selected_bar: state.config_selected_bar,
        toggles,
    }
}

/// Build model entries for the active provider + mode.
fn build_models(state: &State, model_entry: &ModelEntryFn) -> (String, Vec<ConfigModel>) {
    use crate::llms::{AnthropicModel, DeepSeekModel, GrokModel, GroqModel, LlmProvider, MiniMaxModel};

    if state.flags.config.config_secondary_mode {
        let title = "Secondary Model (Reverie)".to_owned();
        let models = match state.secondary_provider {
            LlmProvider::Anthropic | LlmProvider::ClaudeCode | LlmProvider::ClaudeCodeApiKey => {
                vec![
                    model_entry(
                        state.secondary_anthropic_model == AnthropicModel::ClaudeOpus45,
                        "a",
                        &AnthropicModel::ClaudeOpus45,
                    ),
                    model_entry(
                        state.secondary_anthropic_model == AnthropicModel::ClaudeSonnet45,
                        "b",
                        &AnthropicModel::ClaudeSonnet45,
                    ),
                    model_entry(
                        state.secondary_anthropic_model == AnthropicModel::ClaudeHaiku45,
                        "c",
                        &AnthropicModel::ClaudeHaiku45,
                    ),
                ]
            }
            LlmProvider::Grok => vec![
                model_entry(state.secondary_grok_model == GrokModel::Grok41Fast, "a", &GrokModel::Grok41Fast),
                model_entry(state.secondary_grok_model == GrokModel::Grok4Fast, "b", &GrokModel::Grok4Fast),
            ],
            LlmProvider::Groq => vec![
                model_entry(state.secondary_groq_model == GroqModel::GptOss120b, "a", &GroqModel::GptOss120b),
                model_entry(state.secondary_groq_model == GroqModel::GptOss20b, "b", &GroqModel::GptOss20b),
                model_entry(state.secondary_groq_model == GroqModel::Llama33_70b, "c", &GroqModel::Llama33_70b),
                model_entry(state.secondary_groq_model == GroqModel::Llama31_8b, "d", &GroqModel::Llama31_8b),
            ],
            LlmProvider::DeepSeek => vec![
                model_entry(state.secondary_deepseek_model == DeepSeekModel::V4Flash, "a", &DeepSeekModel::V4Flash),
                model_entry(state.secondary_deepseek_model == DeepSeekModel::V4Pro, "b", &DeepSeekModel::V4Pro),
            ],
            LlmProvider::MiniMax => vec![
                model_entry(state.secondary_minimax_model == MiniMaxModel::M27, "a", &MiniMaxModel::M27),
                model_entry(
                    state.secondary_minimax_model == MiniMaxModel::M27Highspeed,
                    "b",
                    &MiniMaxModel::M27Highspeed,
                ),
            ],
            LlmProvider::ClaudeCodeV2 => {
                use crate::llms::ClaudeCodeV2Model;
                vec![
                    model_entry(
                        state.secondary_claude_code_v2_model == ClaudeCodeV2Model::ClaudeOpus48,
                        "a",
                        &ClaudeCodeV2Model::ClaudeOpus48,
                    ),
                    model_entry(
                        state.secondary_claude_code_v2_model == ClaudeCodeV2Model::ClaudeFable5,
                        "b",
                        &ClaudeCodeV2Model::ClaudeFable5,
                    ),
                    model_entry(
                        state.secondary_claude_code_v2_model == ClaudeCodeV2Model::ClaudeSonnet46,
                        "c",
                        &ClaudeCodeV2Model::ClaudeSonnet46,
                    ),
                ]
            }
        };
        (title, models)
    } else {
        let title = "Model".to_owned();
        let models = match state.llm_provider {
            LlmProvider::Anthropic | LlmProvider::ClaudeCode | LlmProvider::ClaudeCodeApiKey => {
                vec![
                    model_entry(
                        state.anthropic_model == AnthropicModel::ClaudeOpus45,
                        "a",
                        &AnthropicModel::ClaudeOpus45,
                    ),
                    model_entry(
                        state.anthropic_model == AnthropicModel::ClaudeSonnet45,
                        "b",
                        &AnthropicModel::ClaudeSonnet45,
                    ),
                    model_entry(
                        state.anthropic_model == AnthropicModel::ClaudeHaiku45,
                        "c",
                        &AnthropicModel::ClaudeHaiku45,
                    ),
                ]
            }
            LlmProvider::Grok => vec![
                model_entry(state.grok_model == GrokModel::Grok41Fast, "a", &GrokModel::Grok41Fast),
                model_entry(state.grok_model == GrokModel::Grok4Fast, "b", &GrokModel::Grok4Fast),
            ],
            LlmProvider::Groq => vec![
                model_entry(state.groq_model == GroqModel::GptOss120b, "a", &GroqModel::GptOss120b),
                model_entry(state.groq_model == GroqModel::GptOss20b, "b", &GroqModel::GptOss20b),
                model_entry(state.groq_model == GroqModel::Llama33_70b, "c", &GroqModel::Llama33_70b),
                model_entry(state.groq_model == GroqModel::Llama31_8b, "d", &GroqModel::Llama31_8b),
            ],
            LlmProvider::DeepSeek => vec![
                model_entry(state.deepseek_model == DeepSeekModel::V4Flash, "a", &DeepSeekModel::V4Flash),
                model_entry(state.deepseek_model == DeepSeekModel::V4Pro, "b", &DeepSeekModel::V4Pro),
            ],
            LlmProvider::MiniMax => vec![
                model_entry(state.minimax_model == MiniMaxModel::M27, "a", &MiniMaxModel::M27),
                model_entry(state.minimax_model == MiniMaxModel::M27Highspeed, "b", &MiniMaxModel::M27Highspeed),
            ],
            LlmProvider::ClaudeCodeV2 => {
                use crate::llms::ClaudeCodeV2Model;
                vec![
                    model_entry(
                        state.claude_code_v2_model == ClaudeCodeV2Model::ClaudeOpus48,
                        "a",
                        &ClaudeCodeV2Model::ClaudeOpus48,
                    ),
                    model_entry(
                        state.claude_code_v2_model == ClaudeCodeV2Model::ClaudeFable5,
                        "b",
                        &ClaudeCodeV2Model::ClaudeFable5,
                    ),
                    model_entry(
                        state.claude_code_v2_model == ClaudeCodeV2Model::ClaudeSonnet46,
                        "c",
                        &ClaudeCodeV2Model::ClaudeSonnet46,
                    ),
                ]
            }
        };
        (title, models)
    }
}

/// Build the budget bar entries.
fn build_budget_bars(state: &State) -> Vec<ConfigBudgetBar> {
    use cp_base::state::data::model_helpers::ModelPricing as _;

    let max_budget = state.model_context_window();
    let effective_budget = state.effective_context_budget();
    let fmt = crate::ui::helpers::format_number;

    let budget_pct = (effective_budget.to_f64() / max_budget.to_f64() * 100.0).to_usize();
    let threshold_pct = (state.cleaning_threshold * 100.0).to_usize();

    vec![
        ConfigBudgetBar {
            label: "Context Budget".into(),
            percent: budget_pct,
            fill_ratio: effective_budget.to_f64() / max_budget.to_f64(),
            value_display: format!("{}% {} tok", budget_pct, fmt(effective_budget)),
            extra: None,
            semantic: Semantic::Success,
            selected: state.config_selected_bar == 0,
        },
        ConfigBudgetBar {
            label: "Clean Trigger".into(),
            percent: threshold_pct,
            fill_ratio: f64::from(state.cleaning_threshold),
            value_display: format!("{}% {} tok", threshold_pct, fmt(state.cleaning_threshold_tokens())),
            extra: None,
            semantic: Semantic::Warning,
            selected: state.config_selected_bar == 1,
        },
    ]
}

/// Build the toggle entries.
fn build_toggles(state: &State) -> Vec<ConfigToggle> {
    let spine_cfg = &cp_mod_spine::types::SpineState::get(state).config;
    let auto_on = spine_cfg.continue_until_todos_done;
    let rev_on = state.flags.config.reverie_enabled;
    let think_threshold =
        state.get_ext::<crate::modules::questions::ThinkState>().map_or(-5, |ts| ts.reminder_threshold);

    vec![
        ConfigToggle {
            label: "Auto-continue".into(),
            enabled: auto_on,
            value_display: if auto_on { "ON".into() } else { "OFF".into() },
            key_hint: "s".into(),
            adjust_keys: None,
        },
        ConfigToggle {
            label: "Reverie".into(),
            enabled: rev_on,
            value_display: if rev_on { "ON".into() } else { "OFF".into() },
            key_hint: "r".into(),
            adjust_keys: None,
        },
        ConfigToggle {
            label: "Think nudge".into(),
            enabled: think_threshold < 0,
            value_display: format!("{think_threshold}"),
            key_hint: String::new(),
            adjust_keys: Some(("[".into(), "]".into())),
        },
    ]
}
