use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
};

use crate::constants::icons;
use crate::state::{MessageStatus, MessageType, State};
use super::{theme, helpers::wrap_text, markdown::*};

pub fn render_conversation(frame: &mut Frame, state: &mut State, area: Rect) {
    let base_style = Style::default().bg(theme::BG_SURFACE);

    // Add margin around the panel
    let inner_area = Rect::new(
        area.x + 1,
        area.y,
        area.width.saturating_sub(2),
        area.height
    );

    // Panel with rounded border
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(theme::BORDER))
        .style(base_style)
        .title(Span::styled(
            if state.is_streaming { " Conversation ● " } else { " Conversation " },
            Style::default().fg(theme::ACCENT).bold()
        ))
        .title_alignment(Alignment::Left);

    let content_area = block.inner(inner_area);
    frame.render_widget(block, inner_area);

    // Build conversation content
    let mut text: Vec<Line> = Vec::new();

    if state.messages.is_empty() {
        // Empty state
        text.push(Line::from(""));
        text.push(Line::from(""));
        text.push(Line::from(vec![
            Span::styled("  Start a conversation by typing below", Style::default().fg(theme::TEXT_MUTED).italic()),
        ]));
    } else {
        for msg in &state.messages {
            if msg.status == MessageStatus::Forgotten {
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
                    // Build params string
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
                        Span::styled(" ", base_style),
                        Span::styled(&tool_use.name, Style::default().fg(theme::TEXT)),
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

                    // Prefix: "✓ Rx   " - same width as tool calls "⚙ Tx   "
                    let prefix_width = 8; // "✓ " + 4-char ID + " "
                    let wrap_width = content_area.width.saturating_sub(prefix_width as u16 + 2) as usize;

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
                                    Span::styled(" ", base_style),
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

            // Status icon
            let status_icon = match msg.status {
                MessageStatus::Full => icons::STATUS_FULL,
                MessageStatus::Summarized => icons::STATUS_SUMMARIZED,
                MessageStatus::Forgotten => icons::STATUS_FORGOTTEN,
            };

            // Message content
            let content = match msg.status {
                MessageStatus::Summarized => msg.tl_dr.as_deref().unwrap_or(&msg.content),
                _ => &msg.content,
            };

            // Fixed-width padded ID (4 chars)
            let padded_id = format!("{:<4}", msg.id);

            // Calculate available width for text (after icon + id + status + spaces)
            let prefix = format!("{} {}{} ", role_icon, padded_id, status_icon);
            let prefix_width = prefix.chars().count();
            let text_width = content_area.width.saturating_sub(2) as usize; // -2 for margins
            let wrap_width = text_width.saturating_sub(prefix_width);

            if content.trim().is_empty() {
                if msg.role == "assistant" && state.is_streaming && state.messages.last().map(|m| m.id.clone()) == Some(msg.id.clone()) {
                    // Show thinking indicator
                    text.push(Line::from(vec![
                        Span::styled(format!("{} ", role_icon), Style::default().fg(role_color)),
                        Span::styled(padded_id.clone(), Style::default().fg(role_color).bold()),
                        Span::styled(status_icon, Style::default().fg(theme::TEXT_MUTED)),
                        Span::styled(" ", base_style),
                        Span::styled("...", Style::default().fg(theme::TEXT_MUTED).italic()),
                    ]));
                } else {
                    text.push(Line::from(vec![
                        Span::styled(format!("{} ", role_icon), Style::default().fg(role_color)),
                        Span::styled(padded_id.clone(), Style::default().fg(role_color).bold()),
                        Span::styled(status_icon, Style::default().fg(theme::TEXT_MUTED)),
                    ]));
                }
            } else {
                // Process each paragraph (split by newlines)
                let mut is_first_line = true;
                let is_assistant = msg.role == "assistant";

                // For assistant messages, pre-process to find and format tables
                let lines: Vec<&str> = content.lines().collect();
                let mut i = 0;

                while i < lines.len() {
                    let line = lines[i];

                    if line.is_empty() {
                        // Empty line - just add indent
                        text.push(Line::from(vec![
                            Span::styled(" ".repeat(prefix_width), base_style),
                        ]));
                        i += 1;
                        continue;
                    }

                    // For assistant messages, check for tables and parse markdown
                    if is_assistant {
                        // Check if this is a table row
                        if line.trim().starts_with('|') && line.trim().ends_with('|') {
                            // Collect all consecutive table rows
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

                            // Render the table with aligned columns
                            let table_spans = render_markdown_table(&table_lines, base_style);
                            for (idx, row_spans) in table_spans.into_iter().enumerate() {
                                if is_first_line && idx == 0 {
                                    let mut line_spans = vec![
                                        Span::styled(format!("{} ", role_icon), Style::default().fg(role_color)),
                                        Span::styled(padded_id.clone(), Style::default().fg(role_color).bold()),
                                        Span::styled(status_icon, Style::default().fg(theme::TEXT_MUTED)),
                                        Span::styled(" ", base_style),
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

                        // Regular markdown line
                        let md_spans = parse_markdown_line(line, base_style);

                        if is_first_line {
                            let mut line_spans = vec![
                                Span::styled(format!("{} ", role_icon), Style::default().fg(role_color)),
                                Span::styled(padded_id.clone(), Style::default().fg(role_color).bold()),
                                Span::styled(status_icon, Style::default().fg(theme::TEXT_MUTED)),
                                Span::styled(" ", base_style),
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
                        // User messages: plain text with word wrap
                        let wrapped = wrap_text(line, wrap_width);

                        for line_text in wrapped.iter() {
                            if is_first_line {
                                text.push(Line::from(vec![
                                    Span::styled(format!("{} ", role_icon), Style::default().fg(role_color)),
                                    Span::styled(padded_id.clone(), Style::default().fg(role_color).bold()),
                                    Span::styled(status_icon, Style::default().fg(theme::TEXT_MUTED)),
                                    Span::styled(" ", base_style),
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

            // Status badge on separate line if present
            if msg.status == MessageStatus::Summarized {
                text.push(Line::from(vec![
                    Span::styled(" ".repeat(prefix_width), base_style),
                    Span::styled(" TL;DR ", Style::default().fg(theme::BG_BASE).bg(theme::WARNING)),
                ]));
            }

            text.push(Line::from(""));
        }
    }

    // Padding at end for scroll
    for _ in 0..3 {
        text.push(Line::from(""));
    }

    // Calculate scroll
    let viewport_width = content_area.width.saturating_sub(2) as usize;
    let viewport_height = content_area.height as usize;

    let content_height: usize = text.iter()
        .map(|line| {
            let char_count: usize = line.spans.iter()
                .map(|span| span.content.chars().count())
                .sum();
            if char_count == 0 || viewport_width == 0 { 1 } else { (char_count + viewport_width - 1) / viewport_width }
        })
        .sum();

    let max_scroll = content_height.saturating_sub(viewport_height) as f32;
    state.max_scroll = max_scroll;

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
