use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

use crate::actions::{parse_context_pattern, find_context_by_id, Action};
use crate::constants::{SCROLL_ARROW_AMOUNT, SCROLL_PAGE_AMOUNT};
use crate::llms::{AnthropicModel, GrokModel, LlmProvider};
use crate::panels::get_panel;
use crate::state::State;

pub fn handle_event(event: &Event, state: &State) -> Option<Action> {
    match event {
        Event::Key(key) => {
            let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

            // Global Ctrl shortcuts (always handled first)
            if ctrl {
                match key.code {
                    KeyCode::Char('q') => return None, // Quit
                    KeyCode::Char('l') => return Some(Action::ClearConversation),
                    KeyCode::Char('n') => return Some(Action::NewContext),
                    KeyCode::Char('k') => return Some(Action::StartContextCleaning),
                    KeyCode::Char('h') => return Some(Action::ToggleConfigView),
                    _ => {}
                }
            }

            // Config view handles its own keys when open
            if state.config_view {
                return handle_config_event(key, state);
            }

            // Escape stops streaming
            if key.code == KeyCode::Esc && state.is_streaming {
                return Some(Action::StopStreaming);
            }

            // F12 toggles performance monitor
            if key.code == KeyCode::F(12) {
                return Some(Action::TogglePerfMonitor);
            }

            // Enter or Space on context pattern (p1, P2, etc.) submits immediately
            if key.code == KeyCode::Enter || key.code == KeyCode::Char(' ') {
                if let Some(id) = parse_context_pattern(&state.input) {
                    if find_context_by_id(state, &id).is_some() {
                        return Some(Action::InputSubmit);
                    }
                }
            }

            // Let the current panel handle the key first
            if let Some(ctx) = state.context.get(state.selected_context) {
                let panel = get_panel(ctx.context_type);
                if let Some(action) = panel.handle_key(key, state) {
                    return Some(action);
                }
            }

            // Global fallback handling (scrolling, context switching)
            let shift = key.modifiers.contains(KeyModifiers::SHIFT);
            let action = match key.code {
                KeyCode::Tab if shift => Action::SelectPrevContext,
                KeyCode::Tab => Action::SelectNextContext,
                KeyCode::BackTab => Action::SelectPrevContext, // Shift+Tab on some terminals
                KeyCode::Up => Action::ScrollUp(SCROLL_ARROW_AMOUNT),
                KeyCode::Down => Action::ScrollDown(SCROLL_ARROW_AMOUNT),
                KeyCode::PageUp => Action::ScrollUp(SCROLL_PAGE_AMOUNT),
                KeyCode::PageDown => Action::ScrollDown(SCROLL_PAGE_AMOUNT),
                _ => Action::None,
            };
            Some(action)
        }
        _ => Some(Action::None),
    }
}

/// Handle key events when config view is open
fn handle_config_event(key: &KeyEvent, state: &State) -> Option<Action> {
    match key.code {
        // Escape or Ctrl+H closes config
        KeyCode::Esc => Some(Action::ToggleConfigView),
        // Number keys select provider
        KeyCode::Char('1') => Some(Action::ConfigSelectProvider(LlmProvider::Anthropic)),
        KeyCode::Char('2') => Some(Action::ConfigSelectProvider(LlmProvider::Grok)),
        // Letter keys select model based on current provider
        KeyCode::Char('a') => match state.llm_provider {
            LlmProvider::Anthropic => Some(Action::ConfigSelectAnthropicModel(AnthropicModel::ClaudeOpus45)),
            LlmProvider::Grok => Some(Action::ConfigSelectGrokModel(GrokModel::Grok41Reasoning)),
        },
        KeyCode::Char('b') => match state.llm_provider {
            LlmProvider::Anthropic => Some(Action::ConfigSelectAnthropicModel(AnthropicModel::ClaudeSonnet45)),
            LlmProvider::Grok => Some(Action::ConfigSelectGrokModel(GrokModel::Grok4Reasoning)),
        },
        KeyCode::Char('c') => match state.llm_provider {
            LlmProvider::Anthropic => Some(Action::ConfigSelectAnthropicModel(AnthropicModel::ClaudeHaiku45)),
            LlmProvider::Grok => Some(Action::None), // Only 2 Grok models
        },
        KeyCode::Char('d') => Some(Action::None), // No 4th model for either provider
        // Arrow keys adjust cleaning threshold
        KeyCode::Left => Some(Action::ConfigDecreaseCleaningThreshold),
        KeyCode::Right => Some(Action::ConfigIncreaseCleaningThreshold),
        // Any other key is ignored in config view
        _ => Some(Action::None),
    }
}
