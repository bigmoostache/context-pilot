use cp_base::panels::time_arith;
use cp_base::state::data::model_helpers::ModelPricing as _;

use crate::state::State;

use super::ActionResult;

/// Number of config bars available.
const CONFIG_BAR_COUNT: usize = 2;

/// Trigger an API connectivity check and save.
pub(crate) fn api_check(state: &mut State) -> ActionResult {
    state.flags.lifecycle.api_check_in_progress = true;
    state.api_check_result = None;
    state.flags.ui.dirty = true;
    ActionResult::StartApiCheck
}

/// Advance to the next config bar index, wrapping around.
pub(crate) const fn next_bar(current: usize) -> usize {
    wrap_next(current, CONFIG_BAR_COUNT)
}

/// Go to the previous config bar index, wrapping around.
pub(crate) const fn prev_bar(current: usize) -> usize {
    if current == 0 { CONFIG_BAR_COUNT.saturating_sub(1) } else { current.saturating_sub(1) }
}

/// Compute `(current + 1) % len` without triggering arithmetic lint.
pub(crate) const fn wrap_next(current: usize, len: usize) -> usize {
    let next = current.saturating_add(1);
    if next >= len { 0 } else { next }
}

/// Handle `ConfigIncreaseSelectedBar` action.
pub(crate) fn handle_config_increase_bar(state: &mut State) -> ActionResult {
    match state.config_selected_bar {
        0 => {
            // Context budget
            let max_budget = state.model_context_window();
            let step = budget_step(max_budget);
            let current = state.context_budget.unwrap_or(max_budget);
            state.context_budget = Some(current.saturating_add(step).min(max_budget));
        }
        1 => {
            // Cleaning threshold
            state.cleaning_threshold = (state.cleaning_threshold + 0.05).min(0.95);
        }
        _ => {}
    }
    state.flags.ui.dirty = true;
    ActionResult::Save
}

/// Handle `ConfigDecreaseSelectedBar` action.
pub(crate) fn handle_config_decrease_bar(state: &mut State) -> ActionResult {
    match state.config_selected_bar {
        0 => {
            // Context budget
            let max_budget = state.model_context_window();
            let step = budget_step(max_budget);
            let min_budget = budget_min(max_budget);
            let current = state.context_budget.unwrap_or(max_budget);
            state.context_budget = Some((current.saturating_sub(step)).max(min_budget));
        }
        1 => {
            // Cleaning threshold
            state.cleaning_threshold = (state.cleaning_threshold - 0.05).max(0.30);
        }
        _ => {}
    }
    state.flags.ui.dirty = true;
    ActionResult::Save
}

/// Handle `ConfigNextTheme` action.
pub(crate) fn handle_config_next_theme(state: &mut State) -> ActionResult {
    use crate::infra::config::THEME_ORDER;
    let current_idx = THEME_ORDER.iter().position(|&t| t == state.active_theme).unwrap_or(0);
    let next_idx = wrap_next(current_idx, THEME_ORDER.len());
    let Some(theme) = THEME_ORDER.get(next_idx) else { return ActionResult::Nothing };
    state.active_theme = (*theme).to_string();
    crate::infra::config::set_active_theme(&state.active_theme);
    state.flags.ui.dirty = true;
    ActionResult::Save
}

/// Handle `ConfigPrevTheme` action.
pub(crate) fn handle_config_prev_theme(state: &mut State) -> ActionResult {
    use crate::infra::config::THEME_ORDER;
    let current_idx = THEME_ORDER.iter().position(|&t| t == state.active_theme).unwrap_or(0);
    let prev_idx = if current_idx == 0 { THEME_ORDER.len().saturating_sub(1) } else { current_idx.saturating_sub(1) };
    let Some(theme) = THEME_ORDER.get(prev_idx) else { return ActionResult::Nothing };
    state.active_theme = (*theme).to_string();
    crate::infra::config::set_active_theme(&state.active_theme);
    state.flags.ui.dirty = true;
    ActionResult::Save
}

/// Compute 5% step for budget adjustment.
const fn budget_step(max_budget: usize) -> usize {
    time_arith::five_pct(max_budget)
}

/// Compute minimum 10% budget floor.
const fn budget_min(max_budget: usize) -> usize {
    time_arith::ten_pct(max_budget)
}
