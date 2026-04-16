use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

use cp_base::panels::scroll_key_action;

use crate::app::actions::{Action, find_context_by_id, parse_context_pattern};
use crate::app::panels::get_panel;
use crate::llms::{AnthropicModel, DeepSeekModel, GrokModel, GroqModel, LlmProvider, MiniMaxModel};
use crate::state::State;

/// Map a terminal event to an application action.
///
/// Returns `None` for Ctrl+Q (quit signal), `Some(Action)` for everything else.
pub(crate) fn handle_event(event: &Event, state: &State) -> Option<Action> {
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
                    KeyCode::Char('v') => return Some(Action::CycleSidebarMode),
                    KeyCode::Char('o') => return Some(Action::ResetSessionCosts),
                    KeyCode::Char('p') => return Some(Action::OpenCommandPalette),
                    // All other keys: fall through to normal handling
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
                    | KeyCode::Modifier(_) => {}
                }
            }

            // Config view handles its own keys when open
            if state.flags.config.config_view {
                return Some(handle_config_event(key, state));
            }

            // Escape stops streaming
            if key.code == KeyCode::Esc && state.flags.stream.phase.is_streaming() {
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
                KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown => {
                    return scroll_key_action(key);
                }
                KeyCode::Backspace
                | KeyCode::Enter
                | KeyCode::Left
                | KeyCode::Right
                | KeyCode::Home
                | KeyCode::End
                | KeyCode::Insert
                | KeyCode::Delete
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
            };
            Some(action)
        }
        // Bracketed paste: store in buffer, insert placeholder sentinel
        // Normalize line endings: terminals may send \r\n or \r instead of \n
        Event::Paste(text) => {
            let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
            Some(Action::PasteText(normalized))
        }
        Event::FocusGained | Event::FocusLost | Event::Mouse(_) | Event::Resize(_, _) => Some(Action::None),
    }
}

/// Handle key events when config view is open
const fn handle_config_event(key: &KeyEvent, state: &State) -> Action {
    let secondary = state.flags.config.config_secondary_mode;
    match key.code {
        // Escape closes config
        KeyCode::Esc => Action::ToggleConfigView,
        // Number keys select provider (main or secondary depending on Tab mode)
        KeyCode::Char('1') => {
            if secondary {
                Action::ConfigSelectSecondaryProvider(LlmProvider::Anthropic)
            } else {
                Action::ConfigSelectProvider(LlmProvider::Anthropic)
            }
        }
        KeyCode::Char('2') => {
            if secondary {
                Action::ConfigSelectSecondaryProvider(LlmProvider::ClaudeCode)
            } else {
                Action::ConfigSelectProvider(LlmProvider::ClaudeCode)
            }
        }
        KeyCode::Char('3') => {
            if secondary {
                Action::ConfigSelectSecondaryProvider(LlmProvider::Grok)
            } else {
                Action::ConfigSelectProvider(LlmProvider::Grok)
            }
        }
        KeyCode::Char('4') => {
            if secondary {
                Action::ConfigSelectSecondaryProvider(LlmProvider::Groq)
            } else {
                Action::ConfigSelectProvider(LlmProvider::Groq)
            }
        }
        KeyCode::Char('5') => {
            if secondary {
                Action::ConfigSelectSecondaryProvider(LlmProvider::DeepSeek)
            } else {
                Action::ConfigSelectProvider(LlmProvider::DeepSeek)
            }
        }
        KeyCode::Char('6') => {
            if secondary {
                Action::ConfigSelectSecondaryProvider(LlmProvider::ClaudeCodeApiKey)
            } else {
                Action::ConfigSelectProvider(LlmProvider::ClaudeCodeApiKey)
            }
        }
        KeyCode::Char('7') => {
            if secondary {
                Action::ConfigSelectSecondaryProvider(LlmProvider::MiniMax)
            } else {
                Action::ConfigSelectProvider(LlmProvider::MiniMax)
            }
        }
        // Letter keys select model based on current provider and Tab mode
        KeyCode::Char('a') => {
            if secondary {
                dispatch_secondary_model(state, 0)
            } else {
                dispatch_primary_model(state, 0)
            }
        }
        KeyCode::Char('b') => {
            if secondary {
                dispatch_secondary_model(state, 1)
            } else {
                dispatch_primary_model(state, 1)
            }
        }
        KeyCode::Char('c') => {
            if secondary {
                dispatch_secondary_model(state, 2)
            } else {
                dispatch_primary_model(state, 2)
            }
        }
        KeyCode::Char('d') => {
            if secondary {
                dispatch_secondary_model(state, 3)
            } else {
                dispatch_primary_model(state, 3)
            }
        }
        // Theme selection - t/T to cycle through themes
        KeyCode::Char('t') => Action::ConfigNextTheme,
        KeyCode::Char('T') => Action::ConfigPrevTheme,
        // Toggle auto-continuation
        KeyCode::Char('s') => Action::ConfigToggleAutoContinue,
        // Toggle reverie (context optimizer)
        KeyCode::Char('r') => Action::ConfigToggleReverie,
        // Tab toggles between main/secondary model selection
        KeyCode::Tab => Action::ConfigToggleSecondaryMode,
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
    match state.llm_provider {
        LlmProvider::Anthropic | LlmProvider::ClaudeCode | LlmProvider::ClaudeCodeApiKey => match idx {
            0 => Action::ConfigSelectAnthropicModel(AnthropicModel::ClaudeOpus45),
            1 => Action::ConfigSelectAnthropicModel(AnthropicModel::ClaudeSonnet45),
            2 => Action::ConfigSelectAnthropicModel(AnthropicModel::ClaudeHaiku45),
            _ => Action::None,
        },
        LlmProvider::Grok => match idx {
            0 => Action::ConfigSelectGrokModel(GrokModel::Grok41Fast),
            1 => Action::ConfigSelectGrokModel(GrokModel::Grok4Fast),
            _ => Action::None,
        },
        LlmProvider::Groq => match idx {
            0 => Action::ConfigSelectGroqModel(GroqModel::GptOss120b),
            1 => Action::ConfigSelectGroqModel(GroqModel::GptOss20b),
            2 => Action::ConfigSelectGroqModel(GroqModel::Llama33_70b),
            3 => Action::ConfigSelectGroqModel(GroqModel::Llama31_8b),
            _ => Action::None,
        },
        LlmProvider::DeepSeek => match idx {
            0 => Action::ConfigSelectDeepSeekModel(DeepSeekModel::DeepseekChat),
            1 => Action::ConfigSelectDeepSeekModel(DeepSeekModel::DeepseekReasoner),
            _ => Action::None,
        },
        LlmProvider::MiniMax => match idx {
            0 => Action::ConfigSelectMiniMaxModel(MiniMaxModel::M27),
            1 => Action::ConfigSelectMiniMaxModel(MiniMaxModel::M27Highspeed),
            _ => Action::None,
        },
    }
}

/// Dispatch secondary model selection based on secondary provider and index
const fn dispatch_secondary_model(state: &State, idx: usize) -> Action {
    match state.secondary_provider {
        LlmProvider::Anthropic | LlmProvider::ClaudeCode | LlmProvider::ClaudeCodeApiKey => match idx {
            0 => Action::ConfigSelectSecondaryAnthropicModel(AnthropicModel::ClaudeOpus45),
            1 => Action::ConfigSelectSecondaryAnthropicModel(AnthropicModel::ClaudeSonnet45),
            2 => Action::ConfigSelectSecondaryAnthropicModel(AnthropicModel::ClaudeHaiku45),
            _ => Action::None,
        },
        LlmProvider::Grok => match idx {
            0 => Action::ConfigSelectSecondaryGrokModel(GrokModel::Grok41Fast),
            1 => Action::ConfigSelectSecondaryGrokModel(GrokModel::Grok4Fast),
            _ => Action::None,
        },
        LlmProvider::Groq => match idx {
            0 => Action::ConfigSelectSecondaryGroqModel(GroqModel::GptOss120b),
            1 => Action::ConfigSelectSecondaryGroqModel(GroqModel::GptOss20b),
            2 => Action::ConfigSelectSecondaryGroqModel(GroqModel::Llama33_70b),
            3 => Action::ConfigSelectSecondaryGroqModel(GroqModel::Llama31_8b),
            _ => Action::None,
        },
        LlmProvider::DeepSeek => match idx {
            0 => Action::ConfigSelectSecondaryDeepSeekModel(DeepSeekModel::DeepseekChat),
            1 => Action::ConfigSelectSecondaryDeepSeekModel(DeepSeekModel::DeepseekReasoner),
            _ => Action::None,
        },
        LlmProvider::MiniMax => match idx {
            0 => Action::ConfigSelectSecondaryMiniMaxModel(MiniMaxModel::M27),
            1 => Action::ConfigSelectSecondaryMiniMaxModel(MiniMaxModel::M27Highspeed),
            _ => Action::None,
        },
    }
}
