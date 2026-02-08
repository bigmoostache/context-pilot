use std::rc::Rc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

use crate::core::panels::{ContextItem, Panel};
use crate::actions::Action;
use crate::constants::icons;
use crate::state::{
    hash_values, FullContentCache, Message, MessageRenderCache, InputRenderCache,
    MessageStatus, MessageType, State,
};
use crate::ui::{theme, helpers::wrap_text, markdown::*};

pub struct ConversationPanel;

impl ConversationPanel {
    /// Compute hash for message cache invalidation
    fn compute_message_hash(msg: &Message, viewport_width: u16, dev_mode: bool) -> u64 {
        // Include all fields that affect rendering
        let status_num = match msg.status {
            MessageStatus::Full => 0u8,
            MessageStatus::Summarized => 1,
            MessageStatus::Deleted => 2,
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

    /// Render a single message to lines (without caching logic)
    fn render_message(
        msg: &Message,
        viewport_width: u16,
        base_style: Style,
        is_streaming_this: bool,
        dev_mode: bool,
    ) -> Vec<Line<'static>> {
        let mut lines: Vec<Line<'static>> = Vec::new();
        let padded_id = format!("{:<4}", msg.id);

        // Handle tool call messages
        if msg.message_type == MessageType::ToolCall {
            for tool_use in &msg.tool_uses {
                let params: Vec<String> = tool_use.input.as_object()
                    .map(|obj| {
                        obj.iter().map(|(k, v)| {
                            let val = match v {
                                serde_json::Value::String(s) => {
                                    if s.len() > 30 { format!("\"{}...\"", &s[..s.floor_char_boundary(27)]) } else { format!("\"{}\"", s) }
                                }
                                _ => v.to_string(),
                            };
                            format!("{}={}", k, val)
                        }).collect()
                    })
                    .unwrap_or_default();

                let params_str = if params.is_empty() {
                    String::new()
                } else {
                    format!(" {}", params.join(" "))
                };

                lines.push(Line::from(vec![
                    Span::styled(icons::msg_tool_call(), Style::default().fg(theme::success())),
                    Span::styled(padded_id.clone(), Style::default().fg(theme::success()).bold()),
                    Span::styled(" ".to_string(), base_style),
                    Span::styled(tool_use.name.clone(), Style::default().fg(theme::text())),
                    Span::styled(params_str, Style::default().fg(theme::text_muted())),
                ]));
            }
            lines.push(Line::from(""));
            return lines;
        }

        // Handle tool result messages
        if msg.message_type == MessageType::ToolResult {
            for result in &msg.tool_results {
                let (status_icon, status_color) = if result.is_error {
                    (icons::msg_error(), theme::warning())
                } else {
                    (icons::msg_tool_result(), theme::success())
                };

                let prefix_width = 8;
                let wrap_width = (viewport_width as usize).saturating_sub(prefix_width + 2).max(20);

                let mut is_first = true;
                for line in result.content.lines() {
                    if line.is_empty() {
                        lines.push(Line::from(vec![
                            Span::styled(" ".repeat(prefix_width), base_style),
                        ]));
                        continue;
                    }

                    let wrapped = wrap_text(line, wrap_width);
                    for wrapped_line in wrapped {
                        if is_first {
                            lines.push(Line::from(vec![
                                Span::styled(status_icon.clone(), Style::default().fg(status_color)),
                                Span::styled(padded_id.clone(), Style::default().fg(status_color).bold()),
                                Span::styled(" ".to_string(), base_style),
                                Span::styled(wrapped_line, Style::default().fg(theme::text_secondary())),
                            ]));
                            is_first = false;
                        } else {
                            lines.push(Line::from(vec![
                                Span::styled(" ".repeat(prefix_width), base_style),
                                Span::styled(wrapped_line, Style::default().fg(theme::text_secondary())),
                            ]));
                        }
                    }
                }
            }
            lines.push(Line::from(""));
            return lines;
        }

        // Regular text message
        let (role_icon, role_color) = if msg.role == "user" {
            (icons::msg_user(), theme::user())
        } else {
            (icons::msg_assistant(), theme::assistant())
        };

        let status_icon = match msg.status {
            MessageStatus::Full => icons::status_full(),
            MessageStatus::Summarized => icons::status_summarized(),
            MessageStatus::Deleted => icons::status_deleted(),
        };

        let content = match msg.status {
            MessageStatus::Summarized => msg.tl_dr.as_deref().unwrap_or(&msg.content),
            _ => &msg.content,
        };

        let prefix = format!("{}{}{}", role_icon, padded_id, status_icon);
        let prefix_width = prefix.chars().count();
        let wrap_width = (viewport_width as usize).saturating_sub(prefix_width + 2).max(20);

        if content.trim().is_empty() {
            if msg.role == "assistant" && is_streaming_this {
                lines.push(Line::from(vec![
                    Span::styled(role_icon.clone(), Style::default().fg(role_color)),
                    Span::styled(padded_id.clone(), Style::default().fg(role_color).bold()),
                    Span::styled(status_icon.to_string(), Style::default().fg(theme::text_muted())),
                    Span::styled(" ".to_string(), base_style),
                    Span::styled("...".to_string(), Style::default().fg(theme::text_muted()).italic()),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::styled(role_icon.clone(), Style::default().fg(role_color)),
                    Span::styled(padded_id.clone(), Style::default().fg(role_color).bold()),
                    Span::styled(status_icon.to_string(), Style::default().fg(theme::text_muted())),
                ]));
            }
        } else {
            let mut is_first_line = true;
            let is_assistant = msg.role == "assistant";
            let content_lines: Vec<&str> = content.lines().collect();
            let mut i = 0;

            while i < content_lines.len() {
                let line = content_lines[i];

                if line.is_empty() {
                    lines.push(Line::from(vec![
                        Span::styled(" ".repeat(prefix_width), base_style),
                    ]));
                    i += 1;
                    continue;
                }

                if is_assistant {
                    // Check for markdown table
                    if line.trim().starts_with('|') && line.trim().ends_with('|') {
                        let mut table_lines: Vec<&str> = vec![line];
                        let mut j = i + 1;
                        while j < content_lines.len() {
                            let next = content_lines[j].trim();
                            if next.starts_with('|') && next.ends_with('|') {
                                table_lines.push(content_lines[j]);
                                j += 1;
                            } else {
                                break;
                            }
                        }

                        let table_spans = render_markdown_table(&table_lines, base_style);
                        for (idx, row_spans) in table_spans.into_iter().enumerate() {
                            if is_first_line && idx == 0 {
                                let mut line_spans = vec![
                                    Span::styled(role_icon.clone(), Style::default().fg(role_color)),
                                    Span::styled(padded_id.clone(), Style::default().fg(role_color).bold()),
                                    Span::styled(status_icon.to_string(), Style::default().fg(theme::text_muted())),
                                    Span::styled(" ".to_string(), base_style),
                                ];
                                line_spans.extend(row_spans);
                                lines.push(Line::from(line_spans));
                                is_first_line = false;
                            } else {
                                let mut line_spans = vec![
                                    Span::styled(" ".repeat(prefix_width), base_style),
                                ];
                                line_spans.extend(row_spans);
                                lines.push(Line::from(line_spans));
                            }
                        }

                        i = j;
                        continue;
                    }

                    // Regular markdown line - pre-wrap then parse
                    let wrapped = wrap_text(line, wrap_width);
                    for wrapped_line in &wrapped {
                        let md_spans = parse_markdown_line(wrapped_line, base_style);

                        if is_first_line {
                            let mut line_spans = vec![
                                Span::styled(role_icon.clone(), Style::default().fg(role_color)),
                                Span::styled(padded_id.clone(), Style::default().fg(role_color).bold()),
                                Span::styled(status_icon.to_string(), Style::default().fg(theme::text_muted())),
                                Span::styled(" ".to_string(), base_style),
                            ];
                            line_spans.extend(md_spans);
                            lines.push(Line::from(line_spans));
                            is_first_line = false;
                        } else {
                            let mut line_spans = vec![
                                Span::styled(" ".repeat(prefix_width), base_style),
                            ];
                            line_spans.extend(md_spans);
                            lines.push(Line::from(line_spans));
                        }
                    }
                } else {
                    // User message - wrap without markdown
                    let wrapped = wrap_text(line, wrap_width);

                    for line_text in wrapped.iter() {
                        if is_first_line {
                            lines.push(Line::from(vec![
                                Span::styled(role_icon.clone(), Style::default().fg(role_color)),
                                Span::styled(padded_id.clone(), Style::default().fg(role_color).bold()),
                                Span::styled(status_icon.to_string(), Style::default().fg(theme::text_muted())),
                                Span::styled(" ".to_string(), base_style),
                                Span::styled(line_text.clone(), Style::default().fg(theme::text())),
                            ]));
                            is_first_line = false;
                        } else {
                            lines.push(Line::from(vec![
                                Span::styled(" ".repeat(prefix_width), base_style),
                                Span::styled(line_text.clone(), Style::default().fg(theme::text())),
                            ]));
                        }
                    }
                }
                i += 1;
            }
        }

        if msg.status == MessageStatus::Summarized {
            lines.push(Line::from(vec![
                Span::styled(" ".repeat(prefix_width), base_style),
                Span::styled(" TL;DR ".to_string(), Style::default().fg(theme::bg_base()).bg(theme::warning())),
            ]));
        }

        // Dev mode: show token counts
        if dev_mode && msg.role == "assistant" && (msg.input_tokens > 0 || msg.content_token_count > 0) {
            lines.push(Line::from(vec![
                Span::styled(" ".repeat(prefix_width), base_style),
                Span::styled(
                    format!("[in:{} out:{}]", msg.input_tokens, msg.content_token_count),
                    Style::default().fg(theme::text_muted()).italic()
                ),
            ]));
        }

        lines.push(Line::from(""));
        lines
    }

    /// Render input area to lines
    fn render_input(input: &str, cursor: usize, viewport_width: u16, base_style: Style) -> Vec<Line<'static>> {
        let mut lines: Vec<Line<'static>> = Vec::new();
        let role_icon = icons::msg_user();
        let role_color = theme::user();
        let prefix_width = 8;
        let wrap_width = (viewport_width as usize).saturating_sub(prefix_width + 2).max(20);
        let cursor_char = "▎";

        // Insert cursor character at cursor position
        let input_with_cursor = if cursor >= input.len() {
            format!("{}{}", input, cursor_char)
        } else {
            format!("{}{}{}", &input[..cursor], cursor_char, &input[cursor..])
        };

        if input.is_empty() {
            lines.push(Line::from(vec![
                Span::styled(role_icon.clone(), Style::default().fg(role_color)),
                Span::styled("... ", Style::default().fg(role_color).dim()),
                Span::styled(" ", base_style),
                Span::styled(cursor_char, Style::default().fg(theme::accent())),
            ]));
        } else {
            let mut is_first_line = true;
            for line in input_with_cursor.lines() {
                if line.is_empty() {
                    lines.push(Line::from(vec![
                        Span::styled(" ".repeat(prefix_width), base_style),
                    ]));
                    continue;
                }

                let wrapped = wrap_text(line, wrap_width);
                for line_text in wrapped.iter() {
                    let spans = if line_text.contains(cursor_char) {
                        let parts: Vec<&str> = line_text.splitn(2, cursor_char).collect();
                        vec![
                            Span::styled(parts.get(0).unwrap_or(&"").to_string(), Style::default().fg(theme::text())),
                            Span::styled(cursor_char, Style::default().fg(theme::accent()).bold()),
                            Span::styled(parts.get(1).unwrap_or(&"").to_string(), Style::default().fg(theme::text())),
                        ]
                    } else {
                        vec![Span::styled(line_text.clone(), Style::default().fg(theme::text()))]
                    };

                    if is_first_line {
                        let mut line_spans = vec![
                            Span::styled(role_icon.clone(), Style::default().fg(role_color)),
                            Span::styled("... ", Style::default().fg(role_color).dim()),
                            Span::styled(" ".to_string(), base_style),
                        ];
                        line_spans.extend(spans);
                        lines.push(Line::from(line_spans));
                        is_first_line = false;
                    } else {
                        let mut line_spans = vec![
                            Span::styled(" ".repeat(prefix_width), base_style),
                        ];
                        line_spans.extend(spans);
                        lines.push(Line::from(line_spans));
                    }
                }
            }
            if input_with_cursor.ends_with('\n') {
                lines.push(Line::from(vec![
                    Span::styled(" ".repeat(prefix_width), base_style),
                ]));
            }
        }
        lines.push(Line::from(""));
        lines
    }

    /// Compute a hash of all content that affects rendering
    fn compute_full_content_hash(state: &State, viewport_width: u16) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();

        // Hash viewport width
        std::hash::Hash::hash(&viewport_width, &mut hasher);
        std::hash::Hash::hash(&state.dev_mode, &mut hasher);
        std::hash::Hash::hash(&state.is_streaming, &mut hasher);

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
                let lines = Self::render_message(msg, viewport_width, base_style, is_streaming_this, state.dev_mode);

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
                let input_lines = Self::render_input(&state.input, state.input_cursor, viewport_width, base_style);
                state.input_cache = Some(InputRenderCache {
                    lines: Rc::new(input_lines.clone()),
                    input_hash,
                    viewport_width,
                });
                text.extend(input_lines);
            }
        } else {
            // No cache
            let input_lines = Self::render_input(&state.input, state.input_cursor, viewport_width, base_style);
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

/// Actions for list continuation behavior
enum ListAction {
    Continue(String),  // Insert list continuation (e.g., "\n- " or "\n2. ")
    RemoveItem,        // Remove empty list item but keep the newline
}

/// Increment alphabetical list marker: a->b, z->aa, A->B, Z->AA
fn next_alpha_marker(marker: &str) -> String {
    let chars: Vec<char> = marker.chars().collect();
    let is_upper = chars[0].is_ascii_uppercase();
    let base = if is_upper { b'A' } else { b'a' };

    // Convert to number (a=0, b=1, ..., z=25, aa=26, ab=27, ...)
    let mut num: usize = 0;
    for c in &chars {
        num = num * 26 + (c.to_ascii_lowercase() as usize - b'a' as usize);
    }
    num += 1; // Increment

    // Convert back to letters
    let mut result = String::new();
    let mut n = num;
    loop {
        result.insert(0, (base + (n % 26) as u8) as char);
        n /= 26;
        if n == 0 { break; }
        n -= 1; // Adjust for 1-based (a=1, not a=0 for multi-char)
    }
    result
}

/// Detect list context and return appropriate action
/// - On non-empty list item: continue the list
/// - On empty list item (just "- " or "1. "): remove it, keep newline
/// - On empty line or non-list: None (send message)
fn detect_list_action(input: &str) -> Option<ListAction> {
    // Get the current line - handle trailing newline specially
    // (lines() doesn't return empty trailing lines)
    let current_line = if input.ends_with('\n') {
        "" // Cursor is on a new empty line
    } else {
        input.lines().last().unwrap_or("")
    };
    let trimmed = current_line.trim_start();

    // Completely empty line - send the message
    if trimmed.is_empty() {
        return None;
    }

    // Check for EMPTY list items (just the prefix with nothing after)
    // Unordered: exactly "- " or "* "
    if trimmed == "- " || trimmed == "* " {
        return Some(ListAction::RemoveItem);
    }

    // Ordered (numeric or alphabetic): exactly "X. " with nothing after
    if let Some(dot_pos) = trimmed.find(". ") {
        let marker = &trimmed[..dot_pos];
        let after = &trimmed[dot_pos + 2..];
        if after.is_empty() {
            // Check if it's a valid marker (numeric or alphabetic)
            let is_numeric = marker.chars().all(|c| c.is_ascii_digit());
            let is_alpha = marker.len() == 1
                && marker.chars().all(|c| c.is_ascii_alphabetic())
                && (marker.chars().all(|c| c.is_ascii_lowercase())
                    || marker.chars().all(|c| c.is_ascii_uppercase()));
            if is_numeric || is_alpha {
                return Some(ListAction::RemoveItem);
            }
        }
    }

    // Check for NON-EMPTY list items - continue the list
    // Unordered list: "- text" or "* text"
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
        let prefix = &trimmed[..2];
        let indent = current_line.len() - trimmed.len();
        return Some(ListAction::Continue(format!("\n{}{}", " ".repeat(indent), prefix)));
    }

    // Ordered list: "1. text", "a. text", "A. text", etc.
    if let Some(dot_pos) = trimmed.find(". ") {
        let marker = &trimmed[..dot_pos];
        let indent = current_line.len() - trimmed.len();

        // Numeric: 1, 2, 3, ...
        if marker.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(num) = marker.parse::<usize>() {
                return Some(ListAction::Continue(format!("\n{}{}. ", " ".repeat(indent), num + 1)));
            }
        }

        // Alphabetic: a, b, c, ... or A, B, C, ... (single char only)
        if marker.len() == 1 && marker.chars().all(|c| c.is_ascii_alphabetic()) {
            let all_lower = marker.chars().all(|c| c.is_ascii_lowercase());
            let all_upper = marker.chars().all(|c| c.is_ascii_uppercase());
            if all_lower || all_upper {
                let next = next_alpha_marker(marker);
                return Some(ListAction::Continue(format!("\n{}{}. ", " ".repeat(indent), next)));
            }
        }
    }

    None // Not a list line, send the message
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
                    match detect_list_action(&state.input) {
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
