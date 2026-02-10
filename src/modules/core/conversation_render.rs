use ratatui::prelude::*;

use crate::constants::icons;
use crate::state::{Message, MessageStatus, MessageType};
use crate::ui::{theme, helpers::wrap_text, markdown::*};

/// Render a single message to lines (without caching logic)
pub(crate) fn render_message(
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
        MessageStatus::Deleted | MessageStatus::Detached => icons::status_deleted(),
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
pub(super) fn render_input(input: &str, cursor: usize, viewport_width: u16, base_style: Style, command_ids: &[String]) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let role_icon = icons::msg_user();
    let role_color = theme::user();
    let prefix_width = 8;
    let wrap_width = (viewport_width as usize).saturating_sub(prefix_width + 2).max(20);
    let cursor_char = "\u{258e}";

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
                let mut spans = build_input_spans(line_text, cursor_char, command_ids);

                // Add command hints if this line segment contains the cursor and starts with /
                if line_text.contains(cursor_char) {
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

/// Build spans for a single input line, with cursor and command highlighting.
fn build_input_spans(line_text: &str, cursor_char: &str, command_ids: &[String]) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();

    // Strip cursor char to get the "clean" text for analysis
    let clean_line = line_text.replace(cursor_char, "");
    let trimmed = clean_line.trim_start();
    let leading_spaces = clean_line.len() - trimmed.len();

    // Check if line starts with / and find the command token
    let (matched_cmd_len, is_command) = if trimmed.starts_with('/') && !command_ids.is_empty() {
        let after_slash = &trimmed[1..];
        let cmd_end = after_slash.find(|c: char| c.is_whitespace()).unwrap_or(after_slash.len());
        let cmd_id = &after_slash[..cmd_end];
        if command_ids.iter().any(|id| id == cmd_id) {
            // +1 for the slash itself
            (leading_spaces + 1 + cmd_end, true)
        } else {
            (0, false)
        }
    } else {
        (0, false)
    };

    if is_command {
        // Split the line into command part (accent color) and rest (normal text)
        let mut cmd_part = String::new();
        let mut rest_part = String::new();
        let mut chars_consumed: usize = 0;
        let mut in_cmd = true;

        for ch in line_text.chars() {
            // Skip cursor char for counting purposes
            if ch.to_string() == cursor_char {
                if in_cmd {
                    cmd_part.push(ch);
                } else {
                    rest_part.push(ch);
                }
                continue;
            }
            if in_cmd && chars_consumed >= matched_cmd_len {
                in_cmd = false;
            }
            if in_cmd {
                cmd_part.push(ch);
            } else {
                rest_part.push(ch);
            }
            chars_consumed += 1;
        }

        // Split cmd_part and rest_part by cursor_char for cursor rendering
        fn push_with_cursor(spans: &mut Vec<Span<'static>>, text: &str, cursor_char: &str, color: ratatui::style::Color) {
            if text.contains(cursor_char) {
                let parts: Vec<&str> = text.splitn(2, cursor_char).collect();
                if !parts[0].is_empty() {
                    spans.push(Span::styled(parts[0].to_string(), Style::default().fg(color)));
                }
                spans.push(Span::styled(cursor_char.to_string(), Style::default().fg(theme::accent()).bold()));
                if parts.len() > 1 && !parts[1].is_empty() {
                    spans.push(Span::styled(parts[1].to_string(), Style::default().fg(color)));
                }
            } else if !text.is_empty() {
                spans.push(Span::styled(text.to_string(), Style::default().fg(color)));
            }
        }

        push_with_cursor(&mut spans, &cmd_part, cursor_char, theme::accent());
        push_with_cursor(&mut spans, &rest_part, cursor_char, theme::text());
    } else {
        // No command — render with normal text color + cursor
        if line_text.contains(cursor_char) {
            let parts: Vec<&str> = line_text.splitn(2, cursor_char).collect();
            spans.push(Span::styled(parts.get(0).unwrap_or(&"").to_string(), Style::default().fg(theme::text())));
            spans.push(Span::styled(cursor_char.to_string(), Style::default().fg(theme::accent()).bold()));
            if let Some(rest) = parts.get(1) {
                spans.push(Span::styled(rest.to_string(), Style::default().fg(theme::text())));
            }
        } else {
            spans.push(Span::styled(line_text.to_string(), Style::default().fg(theme::text())));
        }
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

    let partial = &trimmed[1..]; // after the slash
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
    if matches.len() == 1 && matches[0] == partial {
        return vec![];
    }

    if matches.is_empty() {
        return vec![];
    }

    let hint_text = matches.iter().map(|id| format!("/{}", id)).collect::<Vec<_>>().join("  ");
    vec![
        Span::styled("  ", Style::default()),
        Span::styled(hint_text, Style::default().fg(theme::text_muted()).italic()),
    ]
}
