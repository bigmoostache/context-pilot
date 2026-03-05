use crate::state::State;

use super::ActionResult;

/// Trigger an API connectivity check and save.
pub(crate) fn api_check(state: &mut State) -> ActionResult {
    state.flags.lifecycle.api_check_in_progress = true;
    state.api_check_result = None;
    state.flags.ui.dirty = true;
    ActionResult::StartApiCheck
}

/// Handle `ConfigIncreaseSelectedBar` action
pub(crate) fn handle_config_increase_bar(state: &mut State) -> ActionResult {
    match state.config_selected_bar {
        0 => {
            // Context budget
            let max_budget = state.model_context_window();
            let step = max_budget / 20; // 5% steps
            let current = state.context_budget.unwrap_or(max_budget);
            state.context_budget = Some((current + step).min(max_budget));
        }
        1 => {
            // Cleaning threshold
            state.cleaning_threshold = (state.cleaning_threshold + 0.05).min(0.95);
        }
        2 => {
            // Target proportion
            state.cleaning_target_proportion = (state.cleaning_target_proportion + 0.05).min(0.95);
        }
        3 => {
            // Max cost guard rail ($0.50 steps)
            let spine = cp_mod_spine::SpineState::get_mut(state);
            let current = spine.config.max_cost.unwrap_or(0.0);
            spine.config.max_cost = Some(current + 0.50);
        }
        _ => {}
    }
    state.flags.ui.dirty = true;
    ActionResult::Save
}

/// Handle `ConfigDecreaseSelectedBar` action
pub(crate) fn handle_config_decrease_bar(state: &mut State) -> ActionResult {
    match state.config_selected_bar {
        0 => {
            // Context budget
            let max_budget = state.model_context_window();
            let step = max_budget / 20; // 5% steps
            let min_budget = max_budget / 10; // Minimum 10% of context
            let current = state.context_budget.unwrap_or(max_budget);
            state.context_budget = Some((current.saturating_sub(step)).max(min_budget));
        }
        1 => {
            // Cleaning threshold
            state.cleaning_threshold = (state.cleaning_threshold - 0.05).max(0.30);
        }
        2 => {
            // Target proportion
            state.cleaning_target_proportion = (state.cleaning_target_proportion - 0.05).max(0.30);
        }
        3 => {
            // Max cost guard rail ($0.50 steps, min $0 = disabled)
            let spine = cp_mod_spine::SpineState::get_mut(state);
            let current = spine.config.max_cost.unwrap_or(0.0);
            let new_val = current - 0.50;
            spine.config.max_cost = if new_val <= 0.0 { None } else { Some(new_val) };
        }
        _ => {}
    }
    state.flags.ui.dirty = true;
    ActionResult::Save
}

/// Handle `ConfigNextTheme` action
pub(crate) fn handle_config_next_theme(state: &mut State) -> ActionResult {
    use crate::infra::config::THEME_ORDER;
    let current_idx = THEME_ORDER.iter().position(|&t| t == state.active_theme).unwrap_or(0);
    let next_idx = (current_idx + 1) % THEME_ORDER.len();
    state.active_theme = THEME_ORDER[next_idx].to_string();
    crate::infra::config::set_active_theme(&state.active_theme);
    state.flags.ui.dirty = true;
    ActionResult::Save
}

/// Handle `ConfigPrevTheme` action
pub(crate) fn handle_config_prev_theme(state: &mut State) -> ActionResult {
    use crate::infra::config::THEME_ORDER;
    let current_idx = THEME_ORDER.iter().position(|&t| t == state.active_theme).unwrap_or(0);
    let prev_idx = if current_idx == 0 { THEME_ORDER.len() - 1 } else { current_idx - 1 };
    state.active_theme = THEME_ORDER[prev_idx].to_string();
    crate::infra::config::set_active_theme(&state.active_theme);
    state.flags.ui.dirty = true;
    ActionResult::Save
}
