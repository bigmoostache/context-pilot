use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

use cp_base::panels::scroll_key_action;

use crate::app::actions::{Action, find_context_by_id, parse_context_pattern};
use crate::app::panels::get_panel;
use crate::llms::LlmProvider;
use crate::state::State;

/// Config-overlay model selection dispatch (extracted to keep this file
/// under the 500-line structure limit).
mod models;

/// Outcome of a partial key-dispatch helper: quit the app, produce an action,
/// or decline (let the caller fall through to the next handler).
enum Dispatch {
    /// Quit signal (Ctrl+Q).
    Quit,
    /// Handled — produced this action.
    Act(Action),
    /// Not handled here — fall through to the next stage.
    Fallthrough,
}

/// Map a terminal event to an application action.
///
/// Returns `None` for Ctrl+Q (quit signal), `Some(Action)` for everything else.
pub(crate) fn handle_event(event: &Event, state: &State) -> Option<Action> {
    if let &Event::Key(key) = event {
        return handle_key_event(&key, state);
    }
    // Bracketed paste: store in buffer, insert placeholder sentinel.
    // Normalize line endings: terminals may send \r\n or \r instead of \n.
    if let Event::Paste(text) = event.clone() {
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        return Some(Action::PasteText(normalized));
    }
    Some(Action::None)
}

/// Handle a key event through the staged pipeline. `None` = quit.
fn handle_key_event(key: &KeyEvent, state: &State) -> Option<Action> {
    // Global Ctrl shortcuts (always handled first).
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match handle_ctrl_shortcuts(key, state) {
            Dispatch::Quit => return None,
            Dispatch::Act(action) => return Some(action),
            Dispatch::Fallthrough => {}
        }
    }

    // Config view handles its own keys when open.
    if state.flags.config.config_view {
        return Some(handle_config_event(key, state));
    }

    if let Some(action) = handle_index_overlay_key(key, state) {
        return Some(action);
    }

    // Escape stops streaming.
    if key.code == KeyCode::Esc && state.flags.stream.phase.is_streaming() {
        return Some(Action::StopStreaming);
    }

    // Threads view: intercept navigation keys before panel/scroll handling.
    if state.view_mode == cp_base::state::data::config::ViewMode::Threads
        && let Dispatch::Act(action) = handle_threads_nav(key, state)
    {
        return Some(action);
    }

    // F12 toggles performance monitor.
    if key.code == KeyCode::F(12) {
        return Some(Action::TogglePerfMonitor);
    }

    if let Some(action) = handle_context_pattern_submit(key, state) {
        return Some(action);
    }

    if let Some(action) = handle_panel_key(key, state) {
        return Some(action);
    }

    Some(handle_global_fallback(key))
}

/// Ctrl+key shortcuts. Handles the Threads-view Ctrl+A/Ctrl+U overrides first,
/// then the global bindings.
fn handle_ctrl_shortcuts(key: &KeyEvent, state: &State) -> Dispatch {
    if state.view_mode == cp_base::state::data::config::ViewMode::Threads
        && let Some(action) = handle_threads_ctrl(key, state)
    {
        return Dispatch::Act(action);
    }
    match key.code {
        KeyCode::Char('q') => Dispatch::Quit,
        KeyCode::Char('l') => Dispatch::Act(Action::ClearConversation),
        KeyCode::Char('n') => Dispatch::Act(Action::NewContext),
        KeyCode::Char('h') => Dispatch::Act(Action::ToggleConfigView),
        KeyCode::Char('i') => Dispatch::Act(Action::ToggleIndexOverlay),
        KeyCode::Char('v') => Dispatch::Act(Action::CycleViewMode),
        KeyCode::Char('o') => Dispatch::Act(Action::ResetSessionCosts),
        KeyCode::Char('p') => Dispatch::Act(Action::OpenCommandPalette),
        KeyCode::Char('u') => Dispatch::Act(Action::HistoryPrev),
        KeyCode::Char('d') => Dispatch::Act(Action::HistoryNext),
        KeyCode::Char('c') => Dispatch::Act(if state.flags.overlays.index_status {
            Action::CopyIndexOverlay
        } else {
            Action::CopyPanelContent
        }),
        KeyCode::Backspace
        | KeyCode::Enter
        | KeyCode::Left
        | KeyCode::Right
        | KeyCode::Up
        | KeyCode::Down
        | KeyCode::Home
        | KeyCode::End
        | KeyCode::PageUp
        | KeyCode::PageDown
        | KeyCode::Tab
        | KeyCode::BackTab
        | KeyCode::Delete
        | KeyCode::Insert
        | KeyCode::F(_)
        | KeyCode::Char(_)
        | KeyCode::Null
        | KeyCode::Esc
        | KeyCode::CapsLock
        | KeyCode::ScrollLock
        | KeyCode::NumLock
        | KeyCode::PrintScreen
        | KeyCode::Pause
        | KeyCode::Menu
        | KeyCode::KeypadBegin
        | KeyCode::Media(_)
        | KeyCode::Modifier(_) => Dispatch::Fallthrough,
    }
}

/// Threads-view Ctrl overrides: Ctrl+A archive/restore, Ctrl+U toggle archived
/// view. `None` = not one of these (fall through to global Ctrl bindings).
fn handle_threads_ctrl(key: &KeyEvent, state: &State) -> Option<Action> {
    let viewing_archived = cp_mod_threads::types::FocusState::get(state).viewing_archived;
    match key.code {
        KeyCode::Char('a') => {
            Some(if viewing_archived { Action::ThreadArchiveConfirm } else { Action::ThreadArchiveStart })
        }
        KeyCode::Char('u') => Some(Action::ThreadToggleArchivedView),
        KeyCode::Backspace
        | KeyCode::Enter
        | KeyCode::Left
        | KeyCode::Right
        | KeyCode::Up
        | KeyCode::Down
        | KeyCode::Home
        | KeyCode::End
        | KeyCode::PageUp
        | KeyCode::PageDown
        | KeyCode::Tab
        | KeyCode::BackTab
        | KeyCode::Delete
        | KeyCode::Insert
        | KeyCode::F(_)
        | KeyCode::Char(_)
        | KeyCode::Null
        | KeyCode::Esc
        | KeyCode::CapsLock
        | KeyCode::ScrollLock
        | KeyCode::NumLock
        | KeyCode::PrintScreen
        | KeyCode::Pause
        | KeyCode::Menu
        | KeyCode::KeypadBegin
        | KeyCode::Media(_)
        | KeyCode::Modifier(_) => None,
    }
}

/// Index-overlay keys: Esc dismisses, all other keys are consumed (return
/// `Action::None`). `None` when the overlay is closed.
fn handle_index_overlay_key(key: &KeyEvent, state: &State) -> Option<Action> {
    if !state.flags.overlays.index_status {
        return None;
    }
    Some(if key.code == KeyCode::Esc { Action::ToggleIndexOverlay } else { Action::None })
}

/// Threads-view navigation (non-Ctrl): archive-confirm y/n, Tab/BackTab select,
/// Esc exit. `Fallthrough` when the key isn't a threads-nav key.
fn handle_threads_nav(key: &KeyEvent, state: &State) -> Dispatch {
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let confirming = cp_mod_threads::types::FocusState::get(state).confirming_archive;
    match key.code {
        KeyCode::Char('y') if confirming => Dispatch::Act(Action::ThreadArchiveConfirm),
        _ if confirming => Dispatch::Act(Action::ThreadArchiveCancel),
        KeyCode::Tab if !shift => Dispatch::Act(Action::ThreadSelectNext),
        KeyCode::BackTab => Dispatch::Act(Action::ThreadSelectPrev),
        KeyCode::Esc => Dispatch::Act(Action::CycleViewMode),
        KeyCode::Backspace
        | KeyCode::Enter
        | KeyCode::Left
        | KeyCode::Right
        | KeyCode::Up
        | KeyCode::Down
        | KeyCode::Home
        | KeyCode::End
        | KeyCode::PageUp
        | KeyCode::PageDown
        | KeyCode::Tab
        | KeyCode::Delete
        | KeyCode::Insert
        | KeyCode::F(_)
        | KeyCode::Char(_)
        | KeyCode::Null
        | KeyCode::CapsLock
        | KeyCode::ScrollLock
        | KeyCode::NumLock
        | KeyCode::PrintScreen
        | KeyCode::Pause
        | KeyCode::Menu
        | KeyCode::KeypadBegin
        | KeyCode::Media(_)
        | KeyCode::Modifier(_) => Dispatch::Fallthrough,
    }
}

/// Enter/Space on a context pattern (p1, P2, …) submits immediately — unless a
/// modifier is held (Ctrl/Shift/Alt+Enter = newline). `None` when not applicable.
fn handle_context_pattern_submit(key: &KeyEvent, state: &State) -> Option<Action> {
    let has_modifier = key.modifiers.contains(KeyModifiers::CONTROL)
        || key.modifiers.contains(KeyModifiers::SHIFT)
        || key.modifiers.contains(KeyModifiers::ALT);
    let is_submit = (key.code == KeyCode::Enter && !has_modifier) || key.code == KeyCode::Char(' ');
    if is_submit
        && let Some(id) = parse_context_pattern(&state.input)
        && find_context_by_id(state, &id).is_some()
    {
        return Some(Action::InputSubmit);
    }
    None
}

/// Let the active panel handle the key. In Threads view the conversation panel
/// always owns input routing. `None` when no panel consumes the key.
fn handle_panel_key(key: &KeyEvent, state: &State) -> Option<Action> {
    if state.view_mode == cp_base::state::data::config::ViewMode::Threads {
        let ctx = state.context.iter().find(|c| c.context_type.as_str() == crate::state::Kind::CONVERSATION)?;
        return get_panel(&ctx.context_type).handle_key(key, state);
    }
    let ctx = state.context.get(state.selected_context)?;
    get_panel(&ctx.context_type).handle_key(key, state)
}

/// Global fallback: scrolling + context switching. Returns `Action::None` for
/// unhandled keys.
fn handle_global_fallback(key: &KeyEvent) -> Action {
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    match key.code {
        KeyCode::Tab if shift => Action::SelectPrevContext,
        KeyCode::Tab => Action::SelectNextContext,
        KeyCode::BackTab => Action::SelectPrevContext,
        KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown => {
            scroll_key_action(key).unwrap_or(Action::None)
        }
        KeyCode::Backspace
        | KeyCode::Enter
        | KeyCode::Left
        | KeyCode::Right
        | KeyCode::Home
        | KeyCode::End
        | KeyCode::Delete
        | KeyCode::Insert
        | KeyCode::F(_)
        | KeyCode::Char(_)
        | KeyCode::Null
        | KeyCode::Esc
        | KeyCode::CapsLock
        | KeyCode::ScrollLock
        | KeyCode::NumLock
        | KeyCode::PrintScreen
        | KeyCode::Pause
        | KeyCode::Menu
        | KeyCode::KeypadBegin
        | KeyCode::Media(_)
        | KeyCode::Modifier(_) => Action::None,
    }
}

/// Handle key events when config view is open
const fn handle_config_event(key: &KeyEvent, state: &State) -> Action {
    match key.code {
        // Escape closes config
        KeyCode::Esc => Action::ToggleConfigView,
        // Number keys select provider
        KeyCode::Char('1') => Action::ConfigSelectProvider(LlmProvider::Anthropic),
        KeyCode::Char('2') => Action::ConfigSelectProvider(LlmProvider::ClaudeCode),
        KeyCode::Char('3') => Action::ConfigSelectProvider(LlmProvider::Grok),
        KeyCode::Char('4') => Action::ConfigSelectProvider(LlmProvider::Groq),
        KeyCode::Char('5') => Action::ConfigSelectProvider(LlmProvider::DeepSeek),
        KeyCode::Char('6') => Action::ConfigSelectProvider(LlmProvider::ClaudeCodeApiKey),
        KeyCode::Char('7') => Action::ConfigSelectProvider(LlmProvider::MiniMax),
        KeyCode::Char('8') => Action::ConfigSelectProvider(LlmProvider::ClaudeCodeV2),
        // Letter keys select model based on current provider
        KeyCode::Char('a') => dispatch_primary_model(state, 0),
        KeyCode::Char('b') => dispatch_primary_model(state, 1),
        KeyCode::Char('c') => dispatch_primary_model(state, 2),
        KeyCode::Char('d') => dispatch_primary_model(state, 3),
        // Theme selection - t/T to cycle through themes
        KeyCode::Char('t') => Action::ConfigNextTheme,
        KeyCode::Char('T') => Action::ConfigPrevTheme,
        // Toggle auto-continuation
        KeyCode::Char('s') => Action::ConfigToggleAutoContinue,
        // Toggle reverie (context optimizer)
        KeyCode::Char('r') => Action::ConfigToggleReverie,
        // Think reminder threshold adjustment
        KeyCode::Char(']') => Action::ConfigThinkThresholdUp,
        KeyCode::Char('[') => Action::ConfigThinkThresholdDown,
        KeyCode::Down => Action::ConfigSelectNextBar,
        // Left/Right adjust the selected bar
        KeyCode::Left => Action::ConfigDecreaseSelectedBar,
        KeyCode::Right => Action::ConfigIncreaseSelectedBar,
        // Any other key is ignored in config view
        KeyCode::Backspace
        | KeyCode::Enter
        | KeyCode::Up
        | KeyCode::Home
        | KeyCode::End
        | KeyCode::PageUp
        | KeyCode::PageDown
        | KeyCode::Tab
        | KeyCode::BackTab
        | KeyCode::Delete
        | KeyCode::Insert
        | KeyCode::F(_)
        | KeyCode::Char(_)
        | KeyCode::Null
        | KeyCode::CapsLock
        | KeyCode::ScrollLock
        | KeyCode::NumLock
        | KeyCode::PrintScreen
        | KeyCode::Pause
        | KeyCode::Menu
        | KeyCode::KeypadBegin
        | KeyCode::Media(_)
        | KeyCode::Modifier(_) => Action::None,
    }
}

/// Dispatch primary model selection based on provider and index (0=a, 1=b, 2=c, 3=d)
const fn dispatch_primary_model(state: &State, idx: usize) -> Action {
    models::dispatch_primary_model(state, idx)
}
