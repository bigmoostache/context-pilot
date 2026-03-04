use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

use crate::app::actions::{Action, find_context_by_id, parse_context_pattern};
use crate::app::panels::get_panel;
use crate::infra::constants::{SCROLL_ARROW_AMOUNT, SCROLL_PAGE_AMOUNT};
use crate::llms::{AnthropicModel, DeepSeekModel, GrokModel, GroqModel, LlmProvider};
use crate::state::State;

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
    let secondary = _state.config_secondary_mode;
    match key.code {
        // Escape closes config
        KeyCode::Esc => Some(Action::ToggleConfigView),
        // Number keys select provider (main or secondary depending on Tab mode)
        KeyCode::Char('1') => {
            if secondary {
                Some(Action::ConfigSelectSecondaryProvider(LlmProvider::Anthropic))
            } else {
                Some(Action::ConfigSelectProvider(LlmProvider::Anthropic))
            }
        }
        KeyCode::Char('2') => {
            if secondary {
                Some(Action::ConfigSelectSecondaryProvider(LlmProvider::ClaudeCode))
            } else {
                Some(Action::ConfigSelectProvider(LlmProvider::ClaudeCode))
            }
        }
        KeyCode::Char('3') => {
            if secondary {
                Some(Action::ConfigSelectSecondaryProvider(LlmProvider::Grok))
            } else {
                Some(Action::ConfigSelectProvider(LlmProvider::Grok))
            }
        }
        KeyCode::Char('4') => {
            if secondary {
                Some(Action::ConfigSelectSecondaryProvider(LlmProvider::Groq))
            } else {
                Some(Action::ConfigSelectProvider(LlmProvider::Groq))
            }
        }
        KeyCode::Char('5') => {
            if secondary {
                Some(Action::ConfigSelectSecondaryProvider(LlmProvider::DeepSeek))
            } else {
                Some(Action::ConfigSelectProvider(LlmProvider::DeepSeek))
            }
        }
        KeyCode::Char('6') => {
            if secondary {
                Some(Action::ConfigSelectSecondaryProvider(LlmProvider::ClaudeCodeApiKey))
            } else {
                Some(Action::ConfigSelectProvider(LlmProvider::ClaudeCodeApiKey))
            }
        }
        // Letter keys select model based on current provider and Tab mode
        KeyCode::Char('a') => {
            if secondary {
                dispatch_secondary_model(_state, 0)
            } else {
                dispatch_primary_model(_state, 0)
            }
        }
        KeyCode::Char('b') => {
            if secondary {
                dispatch_secondary_model(_state, 1)
            } else {
                dispatch_primary_model(_state, 1)
            }
        }
        KeyCode::Char('c') => {
            if secondary {
                dispatch_secondary_model(_state, 2)
            } else {
                dispatch_primary_model(_state, 2)
            }
        }
        KeyCode::Char('d') => {
            if secondary {
                dispatch_secondary_model(_state, 3)
            } else {
                dispatch_primary_model(_state, 3)
            }
        }
        // Theme selection - t/T to cycle through themes
        KeyCode::Char('t') => Some(Action::ConfigNextTheme),
        KeyCode::Char('T') => Some(Action::ConfigPrevTheme),
        // Toggle auto-continuation
        KeyCode::Char('s') => Some(Action::ConfigToggleAutoContinue),
        // Toggle reverie (context optimizer)
        KeyCode::Char('r') => Some(Action::ConfigToggleReverie),
        // Tab toggles between main/secondary model selection
        KeyCode::Tab => Some(Action::ConfigToggleSecondaryMode),
        KeyCode::Down => Some(Action::ConfigSelectNextBar),
        // Left/Right adjust the selected bar
        KeyCode::Left => Some(Action::ConfigDecreaseSelectedBar),
        KeyCode::Right => Some(Action::ConfigIncreaseSelectedBar),
        // Any other key is ignored in config view
        _ => Some(Action::None),
    }
}

/// Dispatch primary model selection based on provider and index (0=a, 1=b, 2=c, 3=d)
fn dispatch_primary_model(state: &State, idx: usize) -> Option<Action> {
    match state.llm_provider {
        LlmProvider::Anthropic | LlmProvider::ClaudeCode | LlmProvider::ClaudeCodeApiKey => match idx {
            0 => Some(Action::ConfigSelectAnthropicModel(AnthropicModel::ClaudeOpus45)),
            1 => Some(Action::ConfigSelectAnthropicModel(AnthropicModel::ClaudeSonnet45)),
            2 => Some(Action::ConfigSelectAnthropicModel(AnthropicModel::ClaudeHaiku45)),
            _ => Some(Action::None),
        },
        LlmProvider::Grok => match idx {
            0 => Some(Action::ConfigSelectGrokModel(GrokModel::Grok41Fast)),
            1 => Some(Action::ConfigSelectGrokModel(GrokModel::Grok4Fast)),
            _ => Some(Action::None),
        },
        LlmProvider::Groq => match idx {
            0 => Some(Action::ConfigSelectGroqModel(GroqModel::GptOss120b)),
            1 => Some(Action::ConfigSelectGroqModel(GroqModel::GptOss20b)),
            2 => Some(Action::ConfigSelectGroqModel(GroqModel::Llama33_70b)),
            3 => Some(Action::ConfigSelectGroqModel(GroqModel::Llama31_8b)),
            _ => Some(Action::None),
        },
        LlmProvider::DeepSeek => match idx {
            0 => Some(Action::ConfigSelectDeepSeekModel(DeepSeekModel::DeepseekChat)),
            1 => Some(Action::ConfigSelectDeepSeekModel(DeepSeekModel::DeepseekReasoner)),
            _ => Some(Action::None),
        },
    }
}

/// Dispatch secondary model selection based on secondary provider and index
fn dispatch_secondary_model(state: &State, idx: usize) -> Option<Action> {
    match state.secondary_provider {
        LlmProvider::Anthropic | LlmProvider::ClaudeCode | LlmProvider::ClaudeCodeApiKey => match idx {
            0 => Some(Action::ConfigSelectSecondaryAnthropicModel(AnthropicModel::ClaudeOpus45)),
            1 => Some(Action::ConfigSelectSecondaryAnthropicModel(AnthropicModel::ClaudeSonnet45)),
            2 => Some(Action::ConfigSelectSecondaryAnthropicModel(AnthropicModel::ClaudeHaiku45)),
            _ => Some(Action::None),
        },
        LlmProvider::Grok => match idx {
            0 => Some(Action::ConfigSelectSecondaryGrokModel(GrokModel::Grok41Fast)),
            1 => Some(Action::ConfigSelectSecondaryGrokModel(GrokModel::Grok4Fast)),
            _ => Some(Action::None),
        },
        LlmProvider::Groq => match idx {
            0 => Some(Action::ConfigSelectSecondaryGroqModel(GroqModel::GptOss120b)),
            1 => Some(Action::ConfigSelectSecondaryGroqModel(GroqModel::GptOss20b)),
            2 => Some(Action::ConfigSelectSecondaryGroqModel(GroqModel::Llama33_70b)),
            3 => Some(Action::ConfigSelectSecondaryGroqModel(GroqModel::Llama31_8b)),
            _ => Some(Action::None),
        },
        LlmProvider::DeepSeek => match idx {
            0 => Some(Action::ConfigSelectSecondaryDeepSeekModel(DeepSeekModel::DeepseekChat)),
            1 => Some(Action::ConfigSelectSecondaryDeepSeekModel(DeepSeekModel::DeepseekReasoner)),
            _ => Some(Action::None),
        },
    }
}
