//! `Default` implementation for [`State`] (extracted from `runtime.rs` for the 500-line cap).

use std::collections::HashMap;

use super::super::data::config::ViewMode;
use super::super::flags::{ConfigOverlay, StatusBools, UiState};
use super::State;
use crate::config::llm_types::LlmProvider;

impl Default for State {
    // State is the flat aggregate root of the whole application — ~80 leaf
    // fields (UI cursors, id counters, model selections, cache/stream/tick token
    // + USD telemetry, panel-diff snapshots, per-frame caches). The Default impl
    // is a single linear struct literal: one `field: value` line each. Grouping
    // the telemetry counters into sub-structs to shave lines would ripple across
    // 136 field-access sites for zero behavioural gain, so the initializer stays
    // flat and this one impl carries the length expect (threshold 60, unchanged).
    #[expect(
        clippy::too_many_lines,
        reason = "flat aggregate root-state initializer; sub-grouping fields would churn 136 access sites for no gain"
    )]
    fn default() -> Self {
        Self {
            // NOTE: context and tools are initialized empty here.
            // The binary populates them via the module registry during init.
            context: vec![],
            messages: vec![],
            input: String::new(),
            input_cursor: 0,
            input_selection_anchor: None,
            paste_buffers: vec![],
            paste_buffer_labels: vec![],
            selected_context: 0,
            flags: StatusBools {
                ui: UiState { dirty: true, ..UiState::default() },
                config: ConfigOverlay { reverie_enabled: true, ..ConfigOverlay::default() },
                ..StatusBools::default()
            },
            streaming_tool: None,
            last_stop_reason: None,
            scroll_offset: 0.0,
            scroll_accel: 1.0,
            max_scroll: 0.0,
            streaming_estimated_tokens: 0,
            next_user_id: 1,
            next_assistant_id: 1,
            next_tool_id: 1,
            next_result_id: 1,
            global_next_uid: 1,
            tools: vec![],
            active_modules: std::collections::HashSet::new(),
            config_selected_bar: 0,
            active_theme: crate::config::DEFAULT_THEME.to_owned(),
            llm_provider: LlmProvider::default(),
            anthropic_model: crate::config::models::AnthropicModel::default(),
            grok_model: crate::config::models::GrokModel::default(),
            groq_model: crate::config::models::GroqModel::default(),
            deepseek_model: crate::config::models::DeepSeekModel::default(),
            minimax_model: crate::config::models::MiniMaxModel::default(),
            claude_code_v2_model: crate::config::models::ClaudeCodeV2Model::default(),
            view_mode: ViewMode::Normal,
            reveries: HashMap::new(),
            cache_hit_tokens: 0,
            cache_miss_tokens: 0,
            total_output_tokens: 0,
            uncached_input_tokens: 0,
            stream_cache_hit_tokens: 0,
            stream_cache_miss_tokens: 0,
            stream_output_tokens: 0,
            stream_uncached_input_tokens: 0,
            tick_cache_hit_tokens: 0,
            tick_cache_miss_tokens: 0,
            tick_output_tokens: 0,
            tick_uncached_input_tokens: 0,
            cleaning_threshold: 0.70,
            context_budget: None,
            cost_hit_usd: 0.0,
            cost_miss_usd: 0.0,
            cost_output_usd: 0.0,
            stream_cost_hit_usd: 0.0,
            stream_cost_miss_usd: 0.0,
            stream_cost_output_usd: 0.0,
            tick_cost_hit_usd: 0.0,
            tick_cost_miss_usd: 0.0,
            tick_cost_output_usd: 0.0,
            api_check_result: None,
            api_retry_count: 0,
            guard_rail_blocked: None,
            previous_panel_hash_list: vec![],
            previous_panel_order: vec![],
            previous_panel_id_types: vec![],
            previous_breakpoint_panel_ids: vec![],
            frozen_context_snapshot: None,
            tool_sleep_until_ms: 0,
            cache_engine_json: None,
            tempo: true,
            tick_telemetry: None,
            tick_alive_breakpoints: 0,
            tick_alive_bp_positions: vec![],
            last_viewport_width: 0,
            message_cache: HashMap::new(),
            input_cache: None,
            full_content_cache: None,
            highlight_ir_fn: None,
            module_data: HashMap::new(),
        }
    }
}
