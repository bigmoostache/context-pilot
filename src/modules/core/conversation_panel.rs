use std::rc::Rc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

use crate::core::panels::{ContextItem, Panel};
use crate::actions::Action;
use crate::state::{
    hash_values, ContextType, FullContentCache, MessageRenderCache, InputRenderCache,
    MessageStatus, MessageType, State,
};
use crate::ui::theme;

use super::conversation_list::{self, ListAction};
use super::conversation_render;

pub struct ConversationPanel;

impl ConversationPanel {
    /// Compute hash for message cache invalidation
    fn compute_message_hash(msg: &crate::state::Message, viewport_width: u16, dev_mode: bool) -> u64 {
        // Include all fields that affect rendering
        let status_num = match msg.status {
            MessageStatus::Full => 0u8,
            MessageStatus::Summarized => 1,
            MessageStatus::Deleted => 2,
            MessageStatus::Detached => 3,
        };
        let tl_dr_str = msg.tl_dr.as_deref().unwrap_or("");
        let tool_uses_len = msg.tool_uses.len();
        let tool_results_len = msg.tool_results.len();

        hash_values(&[
            msg.content.as_str(),
            tl_dr_str,
            &format!("{}{}{}{}{}{}",
                status_num, viewport_width, dev_mode as u8,
                tool_uses_len, tool_results_len, msg.input_tokens),
        ])
    }

    /// Compute hash for input cache invalidation
    fn compute_input_hash(input: &str, cursor: usize, viewport_width: u16) -> u64 {
        hash_values(&[input, &format!("{}{}", cursor, viewport_width)])
    }

    /// Compute a hash of all content that affects rendering
    fn compute_full_content_hash(state: &State, viewport_width: u16) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();

        // Hash viewport width
        std::hash::Hash::hash(&viewport_width, &mut hasher);
        std::hash::Hash::hash(&state.dev_mode, &mut hasher);
        std::hash::Hash::hash(&state.is_streaming, &mut hasher);

        // Hash conversation history panel count (invalidate when panels added/removed)
        let history_count = state.context.iter()
            .filter(|c| c.context_type == ContextType::ConversationHistory)
            .count();
        std::hash::Hash::hash(&history_count, &mut hasher);

        // Hash all message content that affects rendering
        for msg in &state.messages {
            std::hash::Hash::hash(&msg.id, &mut hasher);
            std::hash::Hash::hash(&msg.content, &mut hasher);
            std::hash::Hash::hash(&msg.role, &mut hasher);
            std::hash::Hash::hash(&msg.tl_dr, &mut hasher);
            std::hash::Hash::hash(&msg.status, &mut hasher);
            std::hash::Hash::hash(&msg.tool_uses.len(), &mut hasher);
            std::hash::Hash::hash(&msg.tool_results.len(), &mut hasher);
            std::hash::Hash::hash(&msg.input_tokens, &mut hasher);
        }

        // Hash input
        std::hash::Hash::hash(&state.input, &mut hasher);
        std::hash::Hash::hash(&state.input_cursor, &mut hasher);

        std::hash::Hasher::finish(&hasher)
    }

    /// Build content with caching - called from render() which has &mut State
    fn build_content_cached(state: &mut State, base_style: Style) -> Vec<Line<'static>> {
        let _guard = crate::profile!("panel::conversation::content");
        let viewport_width = state.last_viewport_width;

        // Compute full content hash for top-level cache check
        let full_hash = Self::compute_full_content_hash(state, viewport_width);

        // Check full content cache first - if valid, return immediately
        if let Some(ref cached) = state.full_content_cache {
            if cached.content_hash == full_hash {
                // Full cache hit - return cached lines (just clone the Rc's inner vec)
                return (*cached.lines).clone();
            }
        }

        // Cache miss - need to rebuild
        // Check if viewport width changed - invalidate per-message caches
        let width_changed = state.message_cache.values()
            .next()
            .map(|c| c.viewport_width != viewport_width)
            .unwrap_or(false);
        if width_changed {
            state.message_cache.clear();
            state.input_cache = None;
        }

        let mut text: Vec<Line<'static>> = Vec::new();

        // Prepend frozen ConversationHistory panels (oldest first)
        {
            let mut history_panels: Vec<_> = state.context.iter()
                .filter(|c| c.context_type == ContextType::ConversationHistory)
                .collect();
            history_panels.sort_by_key(|c| c.last_refresh_ms);

            for ctx in &history_panels {
                if let Some(ref msgs) = ctx.history_messages {
                    // Separator header
                    text.push(Line::from(vec![
                        Span::styled(
                            format!("── {} ──", ctx.name),
                            Style::default().fg(theme::text_muted()).bold(),
                        ),
                    ]));

                    // Render each frozen message with full formatting
                    for msg in msgs {
                        let lines = conversation_render::render_message(msg, viewport_width, base_style, false, state.dev_mode);
                        text.extend(lines);
                    }

                    // Separator footer
                    text.push(Line::from(vec![
                        Span::styled(
                            "── ── ── ──".to_string(),
                            Style::default().fg(theme::text_muted()),
                        ),
                    ]));
                    text.push(Line::from(""));
                }
            }
        }

        if state.messages.is_empty() {
            text.push(Line::from(""));
            text.push(Line::from(""));
            text.push(Line::from(vec![
                Span::styled("  Start a conversation by typing below".to_string(),
                    Style::default().fg(theme::text_muted()).italic()),
            ]));
        } else {
            let last_msg_id = state.messages.last().map(|m| m.id.clone());

            for msg in &state.messages {
                if msg.status == MessageStatus::Deleted {
                    continue;
                }

                let is_last = last_msg_id.as_ref() == Some(&msg.id);
                let is_streaming_this = state.is_streaming && is_last && msg.role == "assistant";

                // Skip empty text messages (unless streaming)
                if msg.message_type == MessageType::TextMessage
                    && msg.content.trim().is_empty()
                    && !is_streaming_this
                {
                    continue;
                }

                // Compute hash for this message
                let hash = Self::compute_message_hash(msg, viewport_width, state.dev_mode);

                // Check per-message cache
                if let Some(cached) = state.message_cache.get(&msg.id) {
                    if cached.content_hash == hash && cached.viewport_width == viewport_width {
                        // Cache hit - extend from Rc without full clone
                        text.extend(cached.lines.iter().cloned());
                        continue;
                    }
                }

                // Cache miss - render message
                let lines = conversation_render::render_message(msg, viewport_width, base_style, is_streaming_this, state.dev_mode);

                // Store in per-message cache (but not for streaming message)
                if !is_streaming_this {
                    state.message_cache.insert(msg.id.clone(), MessageRenderCache {
                        lines: Rc::new(lines.clone()),
                        content_hash: hash,
                        viewport_width,
                    });
                }

                text.extend(lines);
            }
        }

        // Render input area with caching
        let input_hash = Self::compute_input_hash(&state.input, state.input_cursor, viewport_width);

        if let Some(ref cached) = state.input_cache {
            if cached.input_hash == input_hash && cached.viewport_width == viewport_width {
                // Cache hit
                text.extend(cached.lines.iter().cloned());
            } else {
                // Cache miss
                let input_lines = conversation_render::render_input(&state.input, state.input_cursor, viewport_width, base_style);
                state.input_cache = Some(InputRenderCache {
                    lines: Rc::new(input_lines.clone()),
                    input_hash,
                    viewport_width,
                });
                text.extend(input_lines);
            }
        } else {
            // No cache
            let input_lines = conversation_render::render_input(&state.input, state.input_cursor, viewport_width, base_style);
            state.input_cache = Some(InputRenderCache {
                lines: Rc::new(input_lines.clone()),
                input_hash,
                viewport_width,
            });
            text.extend(input_lines);
        }

        // Padding at end for scroll
        for _ in 0..3 {
            text.push(Line::from(""));
        }

        // Store in full content cache
        state.full_content_cache = Some(FullContentCache {
            lines: Rc::new(text.clone()),
            content_hash: full_hash,
        });

        text
    }
}

impl Panel for ConversationPanel {
    // Conversations are sent to the API as messages, not as context items
    fn context(&self, _state: &State) -> Vec<ContextItem> {
        Vec::new()
    }

    fn title(&self, state: &State) -> String {
        if state.is_streaming {
            "Conversation *".to_string()
        } else {
            "Conversation".to_string()
        }
    }

    fn handle_key(&self, key: &KeyEvent, state: &State) -> Option<Action> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        // Ctrl+Backspace for delete word
        if ctrl && key.code == KeyCode::Backspace {
            return Some(Action::DeleteWordLeft);
        }

        // Regular typing and editing
        match key.code {
            KeyCode::Char(c) => Some(Action::InputChar(c)),
            KeyCode::Backspace => Some(Action::InputBackspace),
            KeyCode::Delete => Some(Action::InputDelete),
            KeyCode::Left => Some(Action::CursorWordLeft),
            KeyCode::Right => Some(Action::CursorWordRight),
            KeyCode::Enter => {
                // Send if: cursor at end AND (input empty OR ends with empty line)
                let at_end = state.input_cursor >= state.input.len();
                let ends_with_empty_line = state.input.ends_with('\n')
                    || state.input.lines().last().map(|l| l.trim().is_empty()).unwrap_or(true);

                if at_end && ends_with_empty_line {
                    // Send message
                    Some(Action::InputSubmit)
                } else {
                    // Check for list continuation, otherwise add newline
                    match conversation_list::detect_list_action(&state.input) {
                        Some(ListAction::Continue(text)) => Some(Action::InsertText(text)),
                        Some(ListAction::RemoveItem) => Some(Action::RemoveListItem),
                        None => Some(Action::InputChar('\n')),
                    }
                }
            }
            KeyCode::Home => Some(Action::CursorHome),
            KeyCode::End => Some(Action::CursorEnd),
            // Arrow keys: let global handle for scrolling
            _ => None,
        }
    }

    fn content(&self, _state: &State, _base_style: Style) -> Vec<Line<'static>> {
        // Note: This is not called - render() is overridden and uses build_content_cached()
        // which has &mut State access for cache updates.
        Vec::new()
    }

    /// Override render to add scrollbar and auto-scroll behavior
    fn render(&self, frame: &mut Frame, state: &mut State, area: Rect) {
        let base_style = Style::default().bg(theme::bg_surface());
        let title = self.title(state);

        let inner_area = Rect::new(
            area.x + 1,
            area.y,
            area.width.saturating_sub(2),
            area.height
        );

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(ratatui::widgets::BorderType::Rounded)
            .border_style(Style::default().fg(theme::border()))
            .style(base_style)
            .title(Span::styled(format!(" {} ", title), Style::default().fg(theme::accent()).bold()));

        let content_area = block.inner(inner_area);
        frame.render_widget(block, inner_area);

        // Update viewport width BEFORE building content so it can pre-wrap lines
        state.last_viewport_width = content_area.width;

        // Use cached content builder (has &mut State for cache updates)
        let text = Self::build_content_cached(state, base_style);

        // Since we pre-wrap in content(), each Line = 1 visual line
        let viewport_height = content_area.height as usize;
        let content_height = text.len();

        let max_scroll = content_height.saturating_sub(viewport_height) as f32;
        state.max_scroll = max_scroll;

        // Auto-scroll to bottom when not manually scrolled
        if state.user_scrolled && state.scroll_offset >= max_scroll - 0.5 {
            state.user_scrolled = false;
        }
        if !state.user_scrolled {
            state.scroll_offset = max_scroll;
        }
        state.scroll_offset = state.scroll_offset.clamp(0.0, max_scroll);

        let paragraph = {
            let _guard = crate::profile!("conv::paragraph_new");
            Paragraph::new(text)
                .style(base_style)
                // NO .wrap() - we pre-wrap in content() for performance
                .scroll((state.scroll_offset.round() as u16, 0))
        };

        {
            let _guard = crate::profile!("conv::frame_render");
            frame.render_widget(paragraph, content_area);
        }

        // Scrollbar
        if content_height > viewport_height {
            let scrollbar = Scrollbar::default()
                .orientation(ScrollbarOrientation::VerticalRight)
                .style(Style::default().fg(theme::bg_elevated()))
                .thumb_style(Style::default().fg(theme::accent_dim()));

            let mut scrollbar_state = ScrollbarState::new(max_scroll as usize)
                .position(state.scroll_offset.round() as usize);

            frame.render_stateful_widget(
                scrollbar,
                inner_area.inner(Margin { horizontal: 0, vertical: 1 }),
                &mut scrollbar_state
            );
        }

    }
}
