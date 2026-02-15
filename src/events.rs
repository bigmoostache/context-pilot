use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

use crate::actions::{Action, find_context_by_id, parse_context_pattern};
use crate::constants::{SCROLL_ARROW_AMOUNT, SCROLL_PAGE_AMOUNT};
use crate::core::panels::get_panel;
use crate::llms::{AnthropicModel, DeepSeekModel, GrokModel, GroqModel, LlmProvider};
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
                    KeyCode::Char('h') => return Some(Action::ToggleConfigView),
                    KeyCode::Char('p') => return Some(Action::OpenCommandPalette),
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
            // But not if modifier keys are held (Ctrl/Shift/Alt+Enter = newline)
            let has_modifier = key.modifiers.contains(KeyModifiers::CONTROL)
                || key.modifiers.contains(KeyModifiers::SHIFT)
                || key.modifiers.contains(KeyModifiers::ALT);
            if ((key.code == KeyCode::Enter && !has_modifier) || key.code == KeyCode::Char(' '))
                && let Some(id) = parse_context_pattern(&state.input)
                && find_context_by_id(state, &id).is_some()
            {
                return Some(Action::InputSubmit);
            }

            // Let the current panel handle the key first
            if let Some(ctx) = state.context.get(state.selected_context) {
                let panel = get_panel(&ctx.context_type);
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
        // Bracketed paste: store in buffer, insert placeholder sentinel
        // Normalize line endings: terminals may send \r\n or \r instead of \n
        Event::Paste(text) => {
            let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
            Some(Action::PasteText(normalized))
        }
        _ => Some(Action::None),
    }
}

/// Handle key events when config view is open
fn handle_config_event(key: &KeyEvent, _state: &State) -> Option<Action> {
    match key.code {
        // Escape closes config
        KeyCode::Esc => Some(Action::ToggleConfigView),
        // Number keys select provider
        KeyCode::Char('1') => Some(Action::ConfigSelectProvider(LlmProvider::Anthropic)),
        KeyCode::Char('2') => Some(Action::ConfigSelectProvider(LlmProvider::ClaudeCode)),
        KeyCode::Char('3') => Some(Action::ConfigSelectProvider(LlmProvider::Grok)),
        KeyCode::Char('4') => Some(Action::ConfigSelectProvider(LlmProvider::Groq)),
        KeyCode::Char('5') => Some(Action::ConfigSelectProvider(LlmProvider::DeepSeek)),
        // Letter keys select model based on current provider
        KeyCode::Char('a') => match _state.llm_provider {
            LlmProvider::Anthropic | LlmProvider::ClaudeCode => {
                Some(Action::ConfigSelectAnthropicModel(AnthropicModel::ClaudeOpus45))
            }
            LlmProvider::Grok => Some(Action::ConfigSelectGrokModel(GrokModel::Grok41Fast)),
            LlmProvider::Groq => Some(Action::ConfigSelectGroqModel(GroqModel::GptOss120b)),
            LlmProvider::DeepSeek => Some(Action::ConfigSelectDeepSeekModel(DeepSeekModel::DeepseekChat)),
        },
        KeyCode::Char('b') => match _state.llm_provider {
            LlmProvider::Anthropic | LlmProvider::ClaudeCode => {
                Some(Action::ConfigSelectAnthropicModel(AnthropicModel::ClaudeSonnet45))
            }
            LlmProvider::Grok => Some(Action::ConfigSelectGrokModel(GrokModel::Grok4Fast)),
            LlmProvider::Groq => Some(Action::ConfigSelectGroqModel(GroqModel::GptOss20b)),
            LlmProvider::DeepSeek => Some(Action::ConfigSelectDeepSeekModel(DeepSeekModel::DeepseekReasoner)),
        },
        KeyCode::Char('c') => match _state.llm_provider {
            LlmProvider::Anthropic | LlmProvider::ClaudeCode => {
                Some(Action::ConfigSelectAnthropicModel(AnthropicModel::ClaudeHaiku45))
            }
            LlmProvider::Grok | LlmProvider::DeepSeek => Some(Action::None),
            LlmProvider::Groq => Some(Action::ConfigSelectGroqModel(GroqModel::Llama33_70b)),
        },
        KeyCode::Char('d') => match _state.llm_provider {
            LlmProvider::Groq => Some(Action::ConfigSelectGroqModel(GroqModel::Llama31_8b)),
            _ => Some(Action::None),
        },
        // Theme selection - t/T to cycle through themes
        KeyCode::Char('t') => Some(Action::ConfigNextTheme),
        KeyCode::Char('T') => Some(Action::ConfigPrevTheme),
        // Up/Down select which bar to edit
        KeyCode::Up => Some(Action::ConfigSelectPrevBar),
        KeyCode::Down => Some(Action::ConfigSelectNextBar),
        // Left/Right adjust the selected bar
        KeyCode::Left => Some(Action::ConfigDecreaseSelectedBar),
        KeyCode::Right => Some(Action::ConfigIncreaseSelectedBar),
        // Any other key is ignored in config view
        _ => Some(Action::None),
    }
}
