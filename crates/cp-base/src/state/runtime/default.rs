//! `Default` implementation for [`State`] (extracted from `runtime.rs` for the 500-line cap).

use std::collections::HashMap;

use super::super::data::config::ViewMode;
use super::super::flags::{ConfigOverlay, StatusBools, UiState};
use super::State;
use super::bundle::ThreadRuntime;
use crate::config::llm::types::LlmProvider;

impl Default for State {
    // `State` now holds only fleet-shared data plus the resident thread's
    // per-thread bundle; the ~45 per-thread leaf fields moved onto
    // [`ThreadRuntime`](super::bundle::ThreadRuntime) and are initialized by its
    // own `Default`. This initializer is a short linear literal of the shared
    // fields + `resident: ThreadRuntime::default()`.
    fn default() -> Self {
        Self {
            resident: ThreadRuntime::default(),
            flags: StatusBools {
                ui: UiState { dirty: true, ..UiState::default() },
                config: ConfigOverlay { reverie_enabled: true, ..ConfigOverlay::default() },
                ..StatusBools::default()
            },
            config_selected_bar: 0,
            global_next_uid: 1,
            cleaning_threshold: 0.70,
            context_budget: None,
            tools: vec![],
            active_modules: std::collections::HashSet::new(),
            active_theme: crate::config::DEFAULT_THEME.to_owned(),
            llm_provider: LlmProvider::default(),
            anthropic_model: crate::config::llm::models::AnthropicModel::default(),
            grok_model: crate::config::llm::models::GrokModel::default(),
            groq_model: crate::config::llm::models::GroqModel::default(),
            deepseek_model: crate::config::llm::models::DeepSeekModel::default(),
            minimax_model: crate::config::llm::models::MiniMaxModel::default(),
            openrouter_model: crate::config::llm::openrouter_model::OpenRouterModel::default(),
            claude_code_v2_model: crate::config::llm::models::ClaudeCodeV2Model::default(),
            view_mode: ViewMode::Normal,
            reveries: HashMap::new(),
            api_check_result: None,
            highlight_ir_fn: None,
            shared_module_data: HashMap::new(),
            init_is_global: None,
            resident_thread_id: None,
        }
    }
}
