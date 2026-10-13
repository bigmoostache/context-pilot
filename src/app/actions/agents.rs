//! Agent action handlers — dispatches `Agent*` action variants.
//!
//! Extracted from `mod.rs` to keep the central dispatch under the 500-line limit.

use cp_mod_agents::types::{AgentsState, FocusState};

use crate::state::State;

use super::{Action, ActionResult};

/// True while the human types a new agent's name: Agents view, active list,
/// not drilled in, selection on the virtual "+ New Agent" row.
pub(crate) fn editing_new_agent_title(state: &State) -> bool {
    if state.view_mode != cp_base::state::data::config::ViewMode::Agents {
        return false;
    }
    let focus = FocusState::get(state);
    if focus.viewing_archived || focus.drilled_agent_id.is_some() {
        return false;
    }
    focus.selected_agent_idx >= AgentsState::get(state).visible_indices(false).len()
}

/// The textarea keystrokes currently edit: the new-thread title on that row,
/// otherwise the executing agent's composer.
pub(crate) fn active_textarea(state: &State) -> &cp_base::state::runtime::textarea::TextArea {
    if editing_new_agent_title(state) { &FocusState::get(state).new_agent_title } else { &state.agent().composer }
}

/// Actions that read or mutate the focused textarea (and so must target the
/// title textarea while it is active).
pub(super) const fn is_title_edit(action: &Action) -> bool {
    matches!(
        action,
        Action::InputBackspace
            | Action::InputDelete
            | Action::DeleteWordLeft
            | Action::RemoveListItem
            | Action::CursorWordLeft
            | Action::CursorWordRight
            | Action::CursorHome
            | Action::CursorEnd
            | Action::CursorLeft
            | Action::CursorRight
            | Action::CursorLeftSelect
            | Action::CursorRightSelect
            | Action::CursorWordLeftSelect
            | Action::CursorWordRightSelect
            | Action::CursorHomeSelect
            | Action::CursorEndSelect
            | Action::SelectAll
            | Action::Undo
            | Action::CopySelection
            | Action::InputChar(_)
            | Action::InsertText(_)
            | Action::PasteText(_)
            | Action::InputSubmit
    )
}

/// Run `run` with the new-thread title swapped into `state.composer`, then swap
/// it back, so the shared editing handlers apply unchanged. Pastes become plain
/// inserts: a title has no paste-sentinel buffers.
pub(super) fn with_new_agent_title(
    state: &mut State,
    action: Action,
    run: fn(&mut State, Action) -> ActionResult,
) -> ActionResult {
    let parked_title = core::mem::take(&mut FocusState::get_mut(state).new_agent_title);
    let draft = core::mem::replace(&mut state.agent_mut().composer, parked_title);
    let edit = if let Action::PasteText(text) = action { Action::InsertText(text) } else { action };
    let result = run(state, edit);
    let edited_title = core::mem::replace(&mut state.agent_mut().composer, draft);
    FocusState::get_mut(state).new_agent_title = edited_title;
    result
}

/// Dispatch a no-data `Agent*` action variant to its handler.
///
/// Called from the central `apply_action` match for all `Agent*` variants
/// except `AgentQuestionChar` (which carries data). Uses equality checks
/// rather than an exhaustive `match` so the ~60 non-thread variants need not be
/// enumerated as a wildcard-free no-op arm.
pub(super) fn dispatch(state: &mut State, action: &Action) -> ActionResult {
    if let Some(result) = dispatch_selection(state, action) {
        return result;
    }
    if let Some(result) = dispatch_drill(state, action) {
        return result;
    }
    dispatch_archive(state, action)
}

/// Handle the selection/creation `Agent*` variants (next/prev/create-start/
/// create-cancel). Returns `None` when `action` is not one of them, so
/// [`dispatch`] can fall through to the archive handlers.
fn dispatch_selection(state: &mut State, action: &Action) -> Option<ActionResult> {
    if matches!(action, Action::AgentSelectNext) {
        return Some(select_next(state));
    }
    if matches!(action, Action::AgentSelectPrev) {
        return Some(select_prev(state));
    }
    if matches!(action, Action::AgentCreateStart) {
        return Some(create_start(state));
    }
    if matches!(action, Action::AgentCreateCancel) {
        return Some(create_cancel(state));
    }
    None
}

/// Handle the drill-in/out `Agent*` variants (G3). Split from
/// [`dispatch_selection`] to keep each helper under the cognitive-complexity cap.
fn dispatch_drill(state: &mut State, action: &Action) -> Option<ActionResult> {
    if matches!(action, Action::AgentDrillIn) {
        return Some(drill_in(state));
    }
    if matches!(action, Action::AgentDrillOut) {
        return Some(drill_out(state));
    }
    None
}

/// Handle the archive/restore `Agent*` variants (archive-start/confirm/cancel,
/// toggle-archived-view). Any non-thread variant no-ops (caller pre-filters).
fn dispatch_archive(state: &mut State, action: &Action) -> ActionResult {
    if matches!(action, Action::AgentArchiveStart) {
        return archive_start(state);
    }
    if matches!(action, Action::AgentArchiveConfirm) {
        return archive_confirm(state);
    }
    if matches!(action, Action::AgentArchiveCancel) {
        return archive_cancel(state);
    }
    if matches!(action, Action::AgentToggleArchivedView) {
        return toggle_archived_view(state);
    }
    // Non-thread variants never reach here (caller pre-filters); no-op fallback.
    ActionResult::Nothing
}

/// User-focus the agent the list cursor now sits on, so the footer (built from
/// the executing agent) tracks the selection as the human arrows up/down —
/// without entering the panel-centric view (that is [`drill_in`]'s job on
/// Right). This sets only [`FocusState::focused_agent_id`]; the loop's
/// `follow_focus` makes that agent executing on the next
/// tick (an id change), and the status bar then reflects its state.
///
/// This is TUI user-focus, not worker/exec focus — the background scheduler is
/// unaffected. No-op on the virtual "+ New Agent" entry or an empty selection
/// (the cursor position does not resolve to a real agent), leaving focus as-is.
fn focus_selected_agent(state: &mut State) {
    let focus = FocusState::get(state);
    let (viewing_archived, pos) = (focus.viewing_archived, focus.selected_agent_idx);
    let visible = AgentsState::get(state).visible_indices(viewing_archived);
    let Some(&real_idx) = visible.get(pos) else {
        return; // virtual "+ New Agent" or empty selection — keep current focus
    };
    let Some(id) = AgentsState::get(state).agents.get(real_idx).map(|t| t.id.clone()) else {
        return;
    };
    FocusState::get_mut(state).focused_agent_id = Some(id);
}

/// Navigate to the next agent (or wrap to first).
fn select_next(state: &mut State) -> ActionResult {
    let viewing_archived = FocusState::get(state).viewing_archived;
    let visible_count = AgentsState::get(state).visible_indices(viewing_archived).len();
    // Active view has a trailing virtual "+ New Agent" entry; archived view does not.
    let total = if viewing_archived { visible_count } else { visible_count.saturating_add(1) };
    let focus = FocusState::get_mut(state);
    focus.selected_agent_idx = if focus.selected_agent_idx >= total.saturating_sub(1) {
        0
    } else {
        focus.selected_agent_idx.saturating_add(1)
    };
    focus_selected_agent(state);
    state.agent_mut().scroll_offset = 0.0;
    state.agent_mut().stream.user_scrolled = false;
    state.flags.ui.dirty = true;
    ActionResult::Nothing
}

/// Navigate to the previous agent (or wrap to last).
fn select_prev(state: &mut State) -> ActionResult {
    let viewing_archived = FocusState::get(state).viewing_archived;
    let visible_count = AgentsState::get(state).visible_indices(viewing_archived).len();
    let total = if viewing_archived { visible_count } else { visible_count.saturating_add(1) };
    let focus = FocusState::get_mut(state);
    focus.selected_agent_idx = if focus.selected_agent_idx == 0 {
        total.saturating_sub(1)
    } else {
        focus.selected_agent_idx.saturating_sub(1)
    };
    focus_selected_agent(state);
    state.agent_mut().scroll_offset = 0.0;
    state.agent_mut().stream.user_scrolled = false;
    state.flags.ui.dirty = true;
    ActionResult::Nothing
}

/// Enter agent creation mode — switches input to naming.
fn create_start(state: &mut State) -> ActionResult {
    let focus = FocusState::get_mut(state);
    focus.creating_agent = true;
    state.agent_mut().composer.reset();
    state.flags.ui.dirty = true;
    ActionResult::Nothing
}

/// Cancel agent creation without creating.
fn create_cancel(state: &mut State) -> ActionResult {
    let focus = FocusState::get_mut(state);
    focus.creating_agent = false;
    state.agent_mut().composer.reset();
    state.flags.ui.dirty = true;
    ActionResult::Nothing
}

/// Open the selected agent's full panel view (Right arrow from the agent
/// list) by **switching the focused agent** and leaving the list for the
/// normal panel-centric view.
///
/// This is a real focus switch, not a read-only glance: it sets
/// [`FocusState::focused_agent_id`] to the selected agent and flips
/// [`view_mode`](cp_base::state::runtime::State::view_mode) to
/// [`Normal`](cp_base::state::data::config::ViewMode::Normal). The loop's
/// `follow_focus` then makes that agent executing on the
/// next tick, so the ordinary Normal render path paints *its* panels /
/// conversation — the same TUI as before, just for a different focused agent
/// (the previously-focused agent keeps running as a background agent).
/// `Left` on an empty composer cycles back to the agent list.
///
/// No-op on the virtual "+ New Agent" entry or an empty selection (the
/// position does not map to a real agent).
fn drill_in(state: &mut State) -> ActionResult {
    let focus = FocusState::get(state);
    let viewing_archived = focus.viewing_archived;
    let pos = focus.selected_agent_idx;
    let visible = AgentsState::get(state).visible_indices(viewing_archived);
    let Some(&real_idx) = visible.get(pos) else {
        return ActionResult::Nothing; // virtual "+ New Agent" or empty selection
    };
    let Some(id) = AgentsState::get(state).agents.get(real_idx).map(|t| t.id.clone()) else {
        return ActionResult::Nothing;
    };
    let focus_mut = FocusState::get_mut(state);
    focus_mut.focused_agent_id = Some(id);
    // Clear any stale read-only drill pointer (unused by this path, kept inert).
    focus_mut.drilled_agent_id = None;
    state.view_mode = cp_base::state::data::config::ViewMode::Normal;
    // Land on the Conversation panel so the composer is immediately typable —
    // without this the newly-focused agent keeps its own stale
    // `selected_context` (e.g. a File/Todo panel), and Char keys route to that
    // panel and silently no-op ("can't type" bug). Conversation is index 0.
    state.agent_mut().selected_context = 0;
    state.agent_mut().scroll_offset = 0.0;
    state.agent_mut().stream.user_scrolled = false;
    state.flags.ui.dirty = true;
    // Persist the focus switch (focused agent is keyed into its own state file).
    ActionResult::Save
}

/// Exit the drilled panel view back to the agent list (G3 — Left/Esc).
fn drill_out(state: &mut State) -> ActionResult {
    FocusState::get_mut(state).drilled_agent_id = None;
    state.agent_mut().scroll_offset = 0.0;
    state.agent_mut().stream.user_scrolled = false;
    state.flags.ui.dirty = true;
    ActionResult::Nothing
}

/// Start agent archive — show confirmation prompt.
///
/// Guards on the *visible* list for the current view (active or archived),
/// so the prompt only appears when there is actually an agent to act on.
fn archive_start(state: &mut State) -> ActionResult {
    let viewing_archived = FocusState::get(state).viewing_archived;
    let has_visible = !AgentsState::get(state).visible_indices(viewing_archived).is_empty();
    if has_visible {
        let focus = FocusState::get_mut(state);
        focus.confirming_archive = true;
        focus.archive_armed_at_ms = cp_base::panels::now_ms();
        state.flags.ui.dirty = true;
    }
    ActionResult::Nothing
}

/// Toggle between the active and archived agent lists (Ctrl+U).
///
/// Resets the selection to the top of the newly-shown list and clears any
/// pending archive confirmation, so the two views never share stale state.
fn toggle_archived_view(state: &mut State) -> ActionResult {
    let focus = FocusState::get_mut(state);
    focus.viewing_archived = !focus.viewing_archived;
    focus.selected_agent_idx = 0;
    focus.confirming_archive = false;
    state.agent_mut().scroll_offset = 0.0;
    state.agent_mut().stream.user_scrolled = false;
    state.flags.ui.dirty = true;
    ActionResult::Nothing
}

/// Confirm agent archive (active view) or restore (archived view).
///
/// `selected_agent_idx` is a position into the *visible* slice
/// ([`AgentsState::visible_indices`]) — the active agents in the normal
/// view, the archived ones in the archived view. We resolve it to a real
/// storage index, then **soft-delete** (set `archived = true`) or **restore**
/// (`archived = false`) instead of removing the agent, so it is retained in
/// state and the web frontend can still display it.
///
/// Cleans up all `FocusState` references to an agent being archived:
/// focused ID, last-read count, `MY_TURN` notification debounce.
fn archive_confirm(state: &mut State) -> ActionResult {
    let focus = FocusState::get(state);
    let viewing_archived = focus.viewing_archived;
    let selected_pos = focus.selected_agent_idx;
    FocusState::get_mut(state).confirming_archive = false;

    // Resolve the visible position to a real storage index.
    let visible = AgentsState::get(state).visible_indices(viewing_archived);
    let Some(&real_idx) = visible.get(selected_pos) else {
        state.flags.ui.dirty = true;
        return ActionResult::Nothing;
    };

    // Toggle the archived flag (archive in active view, restore in archived view).
    let toggled_id = {
        let ts = AgentsState::get_mut(state);
        ts.agents.get_mut(real_idx).map(|t| {
            t.archived = !viewing_archived;
            t.id.clone()
        })
    };

    // Clamp selection to the new visible-list length for the current view.
    let new_visible_len = AgentsState::get(state).visible_indices(viewing_archived).len();
    let focus_after = FocusState::get_mut(state);
    if focus_after.selected_agent_idx >= new_visible_len {
        focus_after.selected_agent_idx = new_visible_len.saturating_sub(1);
    }

    // Clean up focus references to an agent leaving the active list (archive only).
    if !viewing_archived && let Some(aid) = toggled_id {
        if focus_after.focused_agent_id.as_deref() == Some(&aid) {
            focus_after.focused_agent_id = None;
        }
        let _prev = focus_after.last_read_count.remove(&aid);
        if let Some(outcome) = cp_mod_git::worktree::release_agent(state, &aid) {
            log::info!("archive {aid}: {outcome}");
        }
    }

    state.flags.ui.dirty = true;
    ActionResult::Save
}

/// Cancel agent archive — dismiss confirmation.
fn archive_cancel(state: &mut State) -> ActionResult {
    FocusState::get_mut(state).confirming_archive = false;
    state.flags.ui.dirty = true;
    ActionResult::Nothing
}
