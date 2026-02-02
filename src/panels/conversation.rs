use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
};

use super::{ContextItem, Panel};
use crate::actions::Action;
use crate::constants::icons;
use crate::state::{MessageStatus, MessageType, State};
use crate::ui::{theme, helpers::{wrap_text, count_wrapped_lines}, markdown::*};

pub struct ConversationPanel;

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
            let is_alpha = marker.chars().all(|c| c.is_ascii_alphabetic())
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

        // Alphabetic: a, b, c, ... or A, B, C, ...
        if marker.chars().all(|c| c.is_ascii_alphabetic()) {
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
        let alt = key.modifiers.contains(KeyModifiers::ALT);

        // Alt+Enter = newline
        if alt && key.code == KeyCode::Enter {
            return Some(Action::InputChar('\n'));
        }

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
                // Smart Enter: handle list continuation
                match detect_list_action(&state.input) {
                    Some(ListAction::Continue(text)) => Some(Action::InsertText(text)),
                    Some(ListAction::RemoveItem) => Some(Action::RemoveListItem),
                    None => Some(Action::InputSubmit),
                }
            }
            KeyCode::Home => Some(Action::CursorHome),
            KeyCode::End => Some(Action::CursorEnd),
            // Arrow keys: let global handle for scrolling
            _ => None,
        }
    }

    fn content(&self, state: &State, base_style: Style) -> Vec<Line<'static>> {
        let mut text: Vec<Line<'static>> = Vec::new();

        if state.messages.is_empty() {
            text.push(Line::from(""));
            text.push(Line::from(""));
            text.push(Line::from(vec![
                Span::styled("  Start a conversation by typing below".to_string(), Style::default().fg(theme::TEXT_MUTED).italic()),
            ]));
            return text;
        }

        for msg in &state.messages {
            if msg.status == MessageStatus::Deleted {
                continue;
            }

            // Skip empty text messages (unless streaming)
            let is_last = state.messages.last().map(|m| m.id.clone()) == Some(msg.id.clone());
            let is_streaming_this = state.is_streaming && is_last && msg.role == "assistant";
            if msg.message_type == MessageType::TextMessage
                && msg.content.trim().is_empty()
                && !is_streaming_this
            {
                continue;
            }

            // Fixed-width ID (4 chars, left-padded)
            let padded_id = format!("{:<4}", msg.id);

            // Handle tool call messages
            if msg.message_type == MessageType::ToolCall {
                for tool_use in &msg.tool_uses {
                    let params: Vec<String> = tool_use.input.as_object()
                        .map(|obj| {
                            obj.iter().map(|(k, v)| {
                                let val = match v {
                                    serde_json::Value::String(s) => {
                                        if s.len() > 30 { format!("\"{}...\"", &s[..27]) } else { format!("\"{}\"", s) }
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

                    text.push(Line::from(vec![
                        Span::styled(format!("{} ", icons::MSG_TOOL_CALL), Style::default().fg(theme::SUCCESS)),
                        Span::styled(padded_id.clone(), Style::default().fg(theme::SUCCESS).bold()),
                        Span::styled(" ".to_string(), base_style),
                        Span::styled(tool_use.name.clone(), Style::default().fg(theme::TEXT)),
                        Span::styled(params_str, Style::default().fg(theme::TEXT_MUTED)),
                    ]));
                }
                text.push(Line::from(""));
                continue;
            }

            // Handle tool result messages
            if msg.message_type == MessageType::ToolResult {
                for result in &msg.tool_results {
                    let (status_icon, status_color) = if result.is_error {
                        (icons::MSG_ERROR, theme::WARNING)
                    } else {
                        (icons::MSG_TOOL_RESULT, theme::SUCCESS)
                    };

                    let prefix_width = 8;
                    // Using fixed wrap width since we don't have content_area here
                    let wrap_width = 80;

                    let mut is_first = true;
                    for line in result.content.lines() {
                        if line.is_empty() {
                            text.push(Line::from(vec![
                                Span::styled(" ".repeat(prefix_width), base_style),
                            ]));
                            continue;
                        }

                        let wrapped = wrap_text(line, wrap_width);
                        for wrapped_line in wrapped {
                            if is_first {
                                text.push(Line::from(vec![
                                    Span::styled(format!("{} ", status_icon), Style::default().fg(status_color)),
                                    Span::styled(padded_id.clone(), Style::default().fg(status_color).bold()),
                                    Span::styled(" ".to_string(), base_style),
                                    Span::styled(wrapped_line, Style::default().fg(theme::TEXT_SECONDARY)),
                                ]));
                                is_first = false;
                            } else {
                                text.push(Line::from(vec![
                                    Span::styled(" ".repeat(prefix_width), base_style),
                                    Span::styled(wrapped_line, Style::default().fg(theme::TEXT_SECONDARY)),
                                ]));
                            }
                        }
                    }
                }
                text.push(Line::from(""));
                continue;
            }

            // Regular text message
            let (role_icon, role_color) = if msg.role == "user" {
                (icons::MSG_USER, theme::USER)
            } else {
                (icons::MSG_ASSISTANT, theme::ASSISTANT)
            };

            let status_icon = match msg.status {
                MessageStatus::Full => icons::STATUS_FULL,
                MessageStatus::Summarized => icons::STATUS_SUMMARIZED,
                MessageStatus::Deleted => icons::STATUS_DELETED,
            };

            let content = match msg.status {
                MessageStatus::Summarized => msg.tl_dr.as_deref().unwrap_or(&msg.content),
                _ => &msg.content,
            };

            let padded_id = format!("{:<4}", msg.id);
            let prefix = format!("{} {}{} ", role_icon, padded_id, status_icon);
            let prefix_width = prefix.chars().count();
            let wrap_width = 80;

            if content.trim().is_empty() {
                if msg.role == "assistant" && state.is_streaming && state.messages.last().map(|m| m.id.clone()) == Some(msg.id.clone()) {
                    text.push(Line::from(vec![
                        Span::styled(format!("{} ", role_icon), Style::default().fg(role_color)),
                        Span::styled(padded_id.clone(), Style::default().fg(role_color).bold()),
                        Span::styled(status_icon.to_string(), Style::default().fg(theme::TEXT_MUTED)),
                        Span::styled(" ".to_string(), base_style),
                        Span::styled("...".to_string(), Style::default().fg(theme::TEXT_MUTED).italic()),
                    ]));
                } else {
                    text.push(Line::from(vec![
                        Span::styled(format!("{} ", role_icon), Style::default().fg(role_color)),
                        Span::styled(padded_id.clone(), Style::default().fg(role_color).bold()),
                        Span::styled(status_icon.to_string(), Style::default().fg(theme::TEXT_MUTED)),
                    ]));
                }
            } else {
                let mut is_first_line = true;
                let is_assistant = msg.role == "assistant";
                let lines: Vec<&str> = content.lines().collect();
                let mut i = 0;

                while i < lines.len() {
                    let line = lines[i];

                    if line.is_empty() {
                        text.push(Line::from(vec![
                            Span::styled(" ".repeat(prefix_width), base_style),
                        ]));
                        i += 1;
                        continue;
                    }

                    if is_assistant {
                        if line.trim().starts_with('|') && line.trim().ends_with('|') {
                            let mut table_lines: Vec<&str> = vec![line];
                            let mut j = i + 1;
                            while j < lines.len() {
                                let next = lines[j].trim();
                                if next.starts_with('|') && next.ends_with('|') {
                                    table_lines.push(lines[j]);
                                    j += 1;
                                } else {
                                    break;
                                }
                            }

                            let table_spans = render_markdown_table(&table_lines, base_style);
                            for (idx, row_spans) in table_spans.into_iter().enumerate() {
                                if is_first_line && idx == 0 {
                                    let mut line_spans = vec![
                                        Span::styled(format!("{} ", role_icon), Style::default().fg(role_color)),
                                        Span::styled(padded_id.clone(), Style::default().fg(role_color).bold()),
                                        Span::styled(status_icon.to_string(), Style::default().fg(theme::TEXT_MUTED)),
                                        Span::styled(" ".to_string(), base_style),
                                    ];
                                    line_spans.extend(row_spans);
                                    text.push(Line::from(line_spans));
                                    is_first_line = false;
                                } else {
                                    let mut line_spans = vec![
                                        Span::styled(" ".repeat(prefix_width), base_style),
                                    ];
                                    line_spans.extend(row_spans);
                                    text.push(Line::from(line_spans));
                                }
                            }

                            i = j;
                            continue;
                        }

                        let md_spans = parse_markdown_line(line, base_style);

                        if is_first_line {
                            let mut line_spans = vec![
                                Span::styled(format!("{} ", role_icon), Style::default().fg(role_color)),
                                Span::styled(padded_id.clone(), Style::default().fg(role_color).bold()),
                                Span::styled(status_icon.to_string(), Style::default().fg(theme::TEXT_MUTED)),
                                Span::styled(" ".to_string(), base_style),
                            ];
                            line_spans.extend(md_spans);
                            text.push(Line::from(line_spans));
                            is_first_line = false;
                        } else {
                            let mut line_spans = vec![
                                Span::styled(" ".repeat(prefix_width), base_style),
                            ];
                            line_spans.extend(md_spans);
                            text.push(Line::from(line_spans));
                        }
                    } else {
                        let wrapped = wrap_text(line, wrap_width);

                        for line_text in wrapped.iter() {
                            if is_first_line {
                                text.push(Line::from(vec![
                                    Span::styled(format!("{} ", role_icon), Style::default().fg(role_color)),
                                    Span::styled(padded_id.clone(), Style::default().fg(role_color).bold()),
                                    Span::styled(status_icon.to_string(), Style::default().fg(theme::TEXT_MUTED)),
                                    Span::styled(" ".to_string(), base_style),
                                    Span::styled(line_text.clone(), Style::default().fg(theme::TEXT)),
                                ]));
                                is_first_line = false;
                            } else {
                                text.push(Line::from(vec![
                                    Span::styled(" ".repeat(prefix_width), base_style),
                                    Span::styled(line_text.clone(), Style::default().fg(theme::TEXT)),
                                ]));
                            }
                        }
                    }
                    i += 1;
                }
            }

            if msg.status == MessageStatus::Summarized {
                text.push(Line::from(vec![
                    Span::styled(" ".repeat(prefix_width), base_style),
                    Span::styled(" TL;DR ".to_string(), Style::default().fg(theme::BG_BASE).bg(theme::WARNING)),
                ]));
            }

            // Dev mode: show token counts for assistant messages
            if state.dev_mode && msg.role == "assistant" && (msg.input_tokens > 0 || msg.content_token_count > 0) {
                text.push(Line::from(vec![
                    Span::styled(" ".repeat(prefix_width), base_style),
                    Span::styled(
                        format!("[in:{} out:{}]", msg.input_tokens, msg.content_token_count),
                        Style::default().fg(theme::TEXT_MUTED).italic()
                    ),
                ]));
            }

            text.push(Line::from(""));
        }

        // Always show draft input area at the bottom
        {
            let role_icon = icons::MSG_USER;
            let role_color = theme::USER;
            let prefix_width = 8;
            let wrap_width = 80;
            let cursor_char = "▎"; // Visible cursor character

            // Insert cursor character at cursor position
            let input_with_cursor = if state.input_cursor >= state.input.len() {
                format!("{}{}", state.input, cursor_char)
            } else {
                format!("{}{}{}",
                    &state.input[..state.input_cursor],
                    cursor_char,
                    &state.input[state.input_cursor..])
            };

            if state.input.is_empty() {
                // Show empty input line with just cursor
                text.push(Line::from(vec![
                    Span::styled(format!("{} ", role_icon), Style::default().fg(role_color)),
                    Span::styled("... ", Style::default().fg(role_color).dim()),
                    Span::styled(" ", base_style),
                    Span::styled(cursor_char, Style::default().fg(theme::ACCENT)),
                ]));
            } else {
                // Show the draft input with cursor
                let mut is_first_line = true;
                for line in input_with_cursor.lines() {
                    if line.is_empty() {
                        text.push(Line::from(vec![
                            Span::styled(" ".repeat(prefix_width), base_style),
                        ]));
                        continue;
                    }

                    // Split line around cursor char to style it differently
                    let wrapped = wrap_text(line, wrap_width);
                    for line_text in wrapped.iter() {
                        let spans = if line_text.contains(cursor_char) {
                            // Style cursor differently
                            let parts: Vec<&str> = line_text.splitn(2, cursor_char).collect();
                            vec![
                                Span::styled(parts.get(0).unwrap_or(&"").to_string(), Style::default().fg(theme::TEXT)),
                                Span::styled(cursor_char, Style::default().fg(theme::ACCENT).bold()),
                                Span::styled(parts.get(1).unwrap_or(&"").to_string(), Style::default().fg(theme::TEXT)),
                            ]
                        } else {
                            vec![Span::styled(line_text.clone(), Style::default().fg(theme::TEXT))]
                        };

                        if is_first_line {
                            let mut line_spans = vec![
                                Span::styled(format!("{} ", role_icon), Style::default().fg(role_color)),
                                Span::styled("... ", Style::default().fg(role_color).dim()),
                                Span::styled(" ".to_string(), base_style),
                            ];
                            line_spans.extend(spans);
                            text.push(Line::from(line_spans));
                            is_first_line = false;
                        } else {
                            let mut line_spans = vec![
                                Span::styled(" ".repeat(prefix_width), base_style),
                            ];
                            line_spans.extend(spans);
                            text.push(Line::from(line_spans));
                        }
                    }
                }
                // Handle trailing newline (cursor on new empty line)
                if input_with_cursor.ends_with('\n') {
                    text.push(Line::from(vec![
                        Span::styled(" ".repeat(prefix_width), base_style),
                    ]));
                }
            }
            text.push(Line::from(""));
        }

        // Padding at end for scroll
        for _ in 0..3 {
            text.push(Line::from(""));
        }

        text
    }

    /// Override render to add scrollbar and auto-scroll behavior
    fn render(&self, frame: &mut Frame, state: &mut State, area: Rect) {
        let base_style = Style::default().bg(theme::BG_SURFACE);
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
            .border_style(Style::default().fg(theme::BORDER))
            .style(base_style)
            .title(Span::styled(format!(" {} ", title), Style::default().fg(theme::ACCENT).bold()));

        let content_area = block.inner(inner_area);
        frame.render_widget(block, inner_area);

        let text = self.content(state, base_style);

        // Calculate scroll with wrapped line count
        let viewport_width = content_area.width as usize;
        let viewport_height = content_area.height as usize;
        let content_height: usize = text.iter()
            .map(|line| count_wrapped_lines(line, viewport_width))
            .sum();

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

        let paragraph = Paragraph::new(text)
            .style(base_style)
            .wrap(Wrap { trim: false })
            .scroll((state.scroll_offset.round() as u16, 0));

        frame.render_widget(paragraph, content_area);

        // Scrollbar
        if content_height > viewport_height {
            let scrollbar = Scrollbar::default()
                .orientation(ScrollbarOrientation::VerticalRight)
                .style(Style::default().fg(theme::BG_ELEVATED))
                .thumb_style(Style::default().fg(theme::ACCENT_DIM));

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
