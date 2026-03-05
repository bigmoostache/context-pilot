use ratatui::prelude::{Color, Line, Span, Style};

use crate::infra::constants::icons;
use crate::ui::{helpers::wrap_text, theme};

/// Sentinel marker used to represent paste placeholders in the input string.
/// Format: \x00{index}\x00 where index is the `paste_buffers` index.
const SENTINEL_CHAR: char = '\x00';

/// Placeholder prefix used in display text for paste placeholders.
/// These are Unicode private-use-area characters unlikely to appear in normal text.
const PASTE_PLACEHOLDER_START: char = '\u{E000}';
/// Placeholder suffix used in display text for paste placeholders.
const PASTE_PLACEHOLDER_END: char = '\u{E001}';

/// Pre-process input string: replace sentinel markers with display placeholders,
/// adjusting cursor position accordingly. Returns (`display_string`, `adjusted_cursor`).
fn expand_paste_sentinels(
    raw_input: &str,
    raw_cursor: usize,
    paste_buffers: &[String],
    paste_buffer_labels: &[Option<String>],
) -> (String, usize) {
    if !raw_input.contains(SENTINEL_CHAR) {
        return (raw_input.to_string(), raw_cursor);
    }

    let mut result = String::new();
    let mut new_cursor = raw_cursor;
    let mut i = 0;
    let bytes = raw_input.as_bytes();

    while i < bytes.len() {
        let Some(&byte_val) = bytes.get(i) else { break };
        if byte_val == 0 {
            // Found sentinel start — find the index and closing \x00
            let start = i;
            i = i.saturating_add(1);
            let idx_start = i;
            while i < bytes.len() {
                let Some(&inner_byte) = bytes.get(i) else { break };
                if inner_byte == 0 {
                    break;
                }
                i = i.saturating_add(1);
            }
            if i < bytes.len() {
                // Found closing \x00
                let idx_str = raw_input.get(idx_start..i).unwrap_or("");
                i = i.saturating_add(1); // skip closing \x00
                let sentinel_len = i.saturating_sub(start);

                if let Ok(idx) = idx_str.parse::<usize>() {
                    let label = paste_buffer_labels.get(idx).and_then(|l| l.as_ref());
                    let display_text = label.map_or_else(
                        || {
                            // Paste: show line/token stats
                            let (token_count, line_count) = paste_buffers
                                .get(idx)
                                .map_or((0, 0), |s| (crate::state::estimate_tokens(s), s.lines().count().max(1)));
                            format!(
                                "{}📋 Paste #{} ({} lines, {} tok){}",
                                PASTE_PLACEHOLDER_START,
                                idx.saturating_add(1),
                                line_count,
                                token_count,
                                PASTE_PLACEHOLDER_END
                            )
                        },
                        |cmd_name| {
                            // Command: show full content
                            let content = paste_buffers.get(idx).map_or("", |s| s.as_str());
                            format!("{PASTE_PLACEHOLDER_START}⚡/{cmd_name}\n{content}{PASTE_PLACEHOLDER_END}")
                        },
                    );
                    let placeholder = &display_text;
                    let placeholder_len = placeholder.len();

                    // Adjust cursor if it's after this sentinel
                    if raw_cursor > start {
                        if raw_cursor >= start.saturating_add(sentinel_len) {
                            // Cursor is past the sentinel — adjust by difference
                            new_cursor = new_cursor.saturating_add(placeholder_len).saturating_sub(sentinel_len);
                        } else {
                            // Cursor is inside the sentinel — place it at end of placeholder
                            new_cursor = result.len().saturating_add(placeholder_len);
                        }
                    }

                    result.push_str(placeholder);
                } else {
                    // Invalid index — keep as-is
                    result.push_str(raw_input.get(start..i).unwrap_or(""));
                }
            } else {
                // No closing \x00 — keep as-is
                result.push_str(raw_input.get(start..).unwrap_or(""));
            }
        } else {
            let remainder_ch = raw_input.get(i..).unwrap_or("").chars().next().unwrap_or('\0');
            result.push(remainder_ch);
            i = i.saturating_add(remainder_ch.len_utf8());
        }
    }

    (result, new_cursor)
}

/// Render input area to lines.
///
/// Handles cursor rendering, paste placeholder expansion, command highlighting,
/// and command hint display.
#[expect(clippy::too_many_arguments, reason = "render parameters are cohesive and all required for input display")]
pub(super) fn render_input(
    raw_input: &str,
    raw_cursor: usize,
    viewport_width: u16,
    base_style: Style,
    command_ids: &[String],
    paste_buffers: &[String],
    paste_buffer_labels: &[Option<String>],
) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let role_icon = icons::msg_user();
    let role_color = theme::user();
    let prefix_width: usize = 8;
    let wrap_width = (viewport_width as usize).saturating_sub(prefix_width.saturating_add(2)).max(20);
    let cursor_char = "\u{258e}";

    // Keep originals before reassignment (needed for send-hint condition)
    let original_input = raw_input;
    let original_cursor = raw_cursor;

    // Pre-process: expand paste sentinels to display placeholders
    let (display_input, display_cursor) =
        expand_paste_sentinels(raw_input, raw_cursor, paste_buffers, paste_buffer_labels);
    let input = &display_input;
    let cursor_pos = display_cursor;

    // Insert cursor character at cursor position
    let input_with_cursor = if cursor_pos >= input.len() {
        format!("{input}{cursor_char}")
    } else {
        format!(
            "{}{}{}",
            input.get(..cursor_pos).unwrap_or(""),
            cursor_char,
            input.get(cursor_pos..).unwrap_or("")
        )
    };

    if input.is_empty() {
        lines.push(Line::from(vec![
            Span::styled(role_icon, Style::default().fg(role_color)),
            Span::styled("... ", Style::default().fg(role_color).dim()),
            Span::styled(" ", base_style),
            Span::styled(cursor_char, Style::default().fg(theme::accent())),
        ]));
    } else {
        let mut is_first_line = true;
        let mut in_paste_block = false;
        for line in input_with_cursor.lines() {
            if line.is_empty() {
                lines.push(Line::from(vec![Span::styled(" ".repeat(prefix_width), base_style)]));
                continue;
            }

            let wrapped = wrap_text(line, wrap_width);
            for line_text in &wrapped {
                // Check if this line enters or exits a paste placeholder block
                let has_start = line_text.contains(PASTE_PLACEHOLDER_START);
                let has_end = line_text.contains(PASTE_PLACEHOLDER_END);
                if has_start {
                    in_paste_block = true;
                }

                let mut spans = if in_paste_block {
                    // Inside a paste/command block — render entire line in accent, strip markers
                    let clean = line_text.replace([PASTE_PLACEHOLDER_START, PASTE_PLACEHOLDER_END], "");
                    if clean.contains(cursor_char) {
                        let parts: Vec<&str> = clean.splitn(2, cursor_char).collect();
                        let first_part = parts.first().copied().unwrap_or("");
                        vec![
                            Span::styled(first_part.to_string(), Style::default().fg(theme::accent())),
                            Span::styled(cursor_char.to_string(), Style::default().fg(theme::accent()).bold()),
                            Span::styled(
                                parts.get(1).unwrap_or(&"").to_string(),
                                Style::default().fg(theme::accent()),
                            ),
                        ]
                    } else {
                        vec![Span::styled(clean, Style::default().fg(theme::accent()))]
                    }
                } else {
                    build_input_spans(line_text, cursor_char, command_ids)
                };

                if has_end {
                    in_paste_block = false;
                }

                // Add command hints if this line segment contains the cursor and starts with /
                if line_text.contains(cursor_char) && !in_paste_block {
                    let clean_line = line_text.replace(cursor_char, "");
                    let hints = build_command_hints(&clean_line, command_ids);
                    spans.extend(hints);
                }

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
                    let mut line_spans = vec![Span::styled(" ".repeat(prefix_width), base_style)];
                    line_spans.extend(spans);
                    lines.push(Line::from(line_spans));
                }
            }
        }
        if input_with_cursor.ends_with('\n') {
            lines.push(Line::from(vec![Span::styled(" ".repeat(prefix_width), base_style)]));
        }
    }

    // Show hint when next Enter will send
    let at_end = original_cursor >= original_input.len();
    let ends_with_empty_line =
        original_input.ends_with('\n') || original_input.lines().last().is_some_and(|l| l.trim().is_empty());
    if !original_input.is_empty() && at_end && ends_with_empty_line {
        lines.push(Line::from(Span::styled("  ↵ Enter to send", Style::default().fg(theme::text_muted()))));
    }

    lines.push(Line::from(""));
    lines
}

/// Build spans for a single input line, with cursor, command highlighting, and paste placeholders.
fn build_input_spans(line_text: &str, cursor_char: &str, command_ids: &[String]) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();

    // Split into segments: normal text and paste placeholders
    let segments = split_paste_placeholders(line_text);

    for segment in segments {
        match segment {
            InputSegment::Text(text) => {
                spans.extend(build_text_spans(&text, cursor_char, command_ids, line_text));
            }
            InputSegment::PastePlaceholder(text) => {
                // Render as colored placeholder — check if cursor is inside
                if text.contains(cursor_char) {
                    let clean = text.replace(cursor_char, "");
                    spans.push(Span::styled(clean, Style::default().fg(theme::bg_base()).bg(theme::accent())));
                    spans.push(Span::styled(cursor_char.to_string(), Style::default().fg(theme::accent()).bold()));
                } else {
                    spans.push(Span::styled(text, Style::default().fg(theme::bg_base()).bg(theme::accent())));
                }
            }
        }
    }

    spans
}

/// Represents a segment of input text, either plain text or a paste placeholder.
enum InputSegment {
    /// Normal text content.
    Text(String),
    /// Content of a paste placeholder (between start/end markers).
    PastePlaceholder(String),
}

/// Split a line into text segments and paste placeholder segments.
fn split_paste_placeholders(line: &str) -> Vec<InputSegment> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut char_iter = line.chars();

    while let Some(next_ch) = char_iter.next() {
        if next_ch == PASTE_PLACEHOLDER_START {
            // Flush current text
            if !current.is_empty() {
                segments.push(InputSegment::Text(std::mem::take(&mut current)));
            }
            // Collect until PASTE_PLACEHOLDER_END
            let mut placeholder = String::new();
            for inner_ch in char_iter.by_ref() {
                if inner_ch == PASTE_PLACEHOLDER_END {
                    break;
                }
                placeholder.push(inner_ch);
            }
            segments.push(InputSegment::PastePlaceholder(placeholder));
        } else {
            current.push(next_ch);
        }
    }
    if !current.is_empty() {
        segments.push(InputSegment::Text(current));
    }
    segments
}

/// Build spans for a plain text segment (no paste placeholders).
fn build_text_spans(text: &str, cursor_char: &str, command_ids: &[String], _full_line: &str) -> Vec<Span<'static>> {
    /// Push text with cursor highlighting into spans.
    fn push_with_cursor(spans: &mut Vec<Span<'static>>, text: &str, cursor_char: &str, color: Color) {
        if text.contains(cursor_char) {
            let parts: Vec<&str> = text.splitn(2, cursor_char).collect();
            let first_part = parts.first().copied().unwrap_or("");
            if !first_part.is_empty() {
                spans.push(Span::styled(first_part.to_string(), Style::default().fg(color)));
            }
            spans.push(Span::styled(cursor_char.to_string(), Style::default().fg(theme::accent()).bold()));
            let second_part = parts.get(1).copied().unwrap_or("");
            if !second_part.is_empty() {
                spans.push(Span::styled(second_part.to_string(), Style::default().fg(color)));
            }
        } else if !text.is_empty() {
            spans.push(Span::styled(text.to_string(), Style::default().fg(color)));
        }
    }

    let mut spans: Vec<Span<'static>> = Vec::new();

    // Strip cursor char to get the "clean" text for analysis
    let clean_text = text.replace(cursor_char, "");
    let trimmed = clean_text.trim_start();
    let leading_spaces = clean_text.len().saturating_sub(trimmed.len());

    // Check if text starts with / and find the command token
    let (matched_cmd_len, is_command) = if trimmed.starts_with('/') && !command_ids.is_empty() {
        let after_slash = trimmed.get(1..).unwrap_or("");
        let cmd_end = after_slash.find(|c: char| c.is_whitespace()).unwrap_or(after_slash.len());
        let cmd_id = after_slash.get(..cmd_end).unwrap_or("");
        if command_ids.iter().any(|id| id == cmd_id) {
            // +1 for the slash itself
            (leading_spaces.saturating_add(1).saturating_add(cmd_end), true)
        } else {
            (0, false)
        }
    } else {
        (0, false)
    };

    if is_command {
        // Split the text into command part (accent color) and rest (normal text)
        let mut cmd_part = String::new();
        let mut rest_part = String::new();
        let mut chars_consumed: usize = 0;
        let mut in_cmd = true;

        for text_ch in text.chars() {
            // Skip cursor char for counting purposes
            if text_ch.to_string() == cursor_char {
                if in_cmd {
                    cmd_part.push(text_ch);
                } else {
                    rest_part.push(text_ch);
                }
                continue;
            }
            if in_cmd && chars_consumed >= matched_cmd_len {
                in_cmd = false;
            }
            if in_cmd {
                cmd_part.push(text_ch);
            } else {
                rest_part.push(text_ch);
            }
            chars_consumed = chars_consumed.saturating_add(1);
        }

        // Split cmd_part and rest_part by cursor_char for cursor rendering
        push_with_cursor(&mut spans, &cmd_part, cursor_char, theme::accent());
        push_with_cursor(&mut spans, &rest_part, cursor_char, theme::text());
    } else {
        // No command — render with normal text color + cursor
        push_with_cursor(&mut spans, text, cursor_char, theme::text());
    }

    spans
}

/// Show available command hints when user types `/` at start of a line.
/// Returns hint spans to append after the input line, or empty vec if no hints.
fn build_command_hints(clean_line: &str, command_ids: &[String]) -> Vec<Span<'static>> {
    let trimmed = clean_line.trim_start();
    if !trimmed.starts_with('/') || command_ids.is_empty() {
        return vec![];
    }

    let partial = trimmed.get(1..).unwrap_or(""); // after the slash
    // If there's a space, user is past the command name — no hints
    if partial.contains(' ') {
        return vec![];
    }

    // Find matching commands
    let matches: Vec<&String> = if partial.is_empty() {
        command_ids.iter().collect()
    } else {
        command_ids.iter().filter(|id| id.starts_with(partial)).collect()
    };

    // Don't show hints if exact match already typed
    if matches.len() == 1 {
        let first_match = matches.first().map_or("", |s| s.as_str());
        if first_match == partial {
            return vec![];
        }
    }

    if matches.is_empty() {
        return vec![];
    }

    let hint_text = matches.iter().map(|id| format!("/{id}")).collect::<Vec<_>>().join("  ");
    vec![
        Span::styled("  ", Style::default()),
        Span::styled(hint_text, Style::default().fg(theme::text_muted()).italic()),
    ]
}
