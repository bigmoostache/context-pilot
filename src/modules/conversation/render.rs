use ratatui::prelude::*;

use crate::infra::constants::icons;
use crate::state::{Message, MessageStatus, MessageType};
use crate::ui::{
    helpers::wrap_text,
    markdown::{parse_markdown_line, render_markdown_table},
    theme,
};

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::modules::{ToolVisualizer, build_visualizer_registry};

/// Lazily built registry of `tool_name` -> visualizer function.
static VISUALIZER_REGISTRY: OnceLock<HashMap<String, ToolVisualizer>> = OnceLock::new();

fn get_visualizer_registry() -> &'static HashMap<String, ToolVisualizer> {
    VISUALIZER_REGISTRY.get_or_init(build_visualizer_registry)
}

/// Render a single message to lines (without caching logic)
#[expect(clippy::wildcard_enum_match_arm, reason = "remaining variants are handled uniformly")]
pub(crate) fn render_message(
    msg: &Message,
    viewport_width: u16,
    base_style: Style,
    is_streaming_this: bool,
    dev_mode: bool,
) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    // Handle tool call messages
    if msg.message_type == MessageType::ToolCall {
        for tool_use in &msg.tool_uses {
            let params: Vec<String> = tool_use
                .input
                .as_object()
                .map(|obj| {
                    obj.iter()
                        .map(|(k, v)| {
                            let val = match v {
                                serde_json::Value::String(s) => {
                                    if s.len() > 30 {
                                        format!("\"{}...\"", &s[..s.floor_char_boundary(27)])
                                    } else {
                                        format!("\"{s}\"")
                                    }
                                }
                                _ => v.to_string(),
                            };
                            format!("{k}={val}")
                        })
                        .collect()
                })
                .unwrap_or_default();

            let params_str = if params.is_empty() { String::new() } else { format!(" {}", params.join(" ")) };

            lines.push(Line::from(vec![
                Span::styled(icons::msg_tool_call(), Style::default().fg(theme::success())),
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

            let prefix_width = 4;
            let wrap_width = (viewport_width as usize).saturating_sub(prefix_width + 1).max(20);

            // Check if a module registered a custom visualizer for this tool
            let registry = get_visualizer_registry();
            let custom_lines = if result.tool_name.is_empty() {
                None
            } else {
                registry.get(&result.tool_name).map(|visualizer| visualizer(&result.content, wrap_width))
            };

            #[expect(clippy::branches_sharing_code, reason = "factoring out shared code would reduce clarity")]
            if let Some(vis_lines) = custom_lines {
                // Use module-provided visualization
                let mut is_first = true;
                for vis_line in vis_lines {
                    if is_first {
                        let mut line_spans = vec![
                            Span::styled(status_icon.clone(), Style::default().fg(status_color)),
                            Span::styled(" ".to_string(), base_style),
                        ];
                        line_spans.extend(vis_line.spans);
                        lines.push(Line::from(line_spans));
                        is_first = false;
                    } else {
                        let mut line_spans = vec![Span::styled(" ".repeat(prefix_width), base_style)];
                        line_spans.extend(vis_line.spans);
                        lines.push(Line::from(line_spans));
                    }
                }
            } else {
                // Fallback: plain text rendering with wrapping
                let mut is_first = true;
                for line in result.content.lines() {
                    if line.is_empty() {
                        lines.push(Line::from(vec![Span::styled(" ".repeat(prefix_width), base_style)]));
                        continue;
                    }

                    let wrapped = wrap_text(line, wrap_width);
                    for wrapped_line in wrapped {
                        if is_first {
                            lines.push(Line::from(vec![
                                Span::styled(status_icon.clone(), Style::default().fg(status_color)),
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
        MessageStatus::Deleted | MessageStatus::Detached => icons::status_deleted(),
    };

    let content = &msg.content;

    let prefix = format!("{role_icon}{status_icon} ");
    let prefix_width = prefix.chars().count();
    let wrap_width = (viewport_width as usize).saturating_sub(prefix_width + 2).max(20);

    if content.trim().is_empty() {
        if msg.role == "assistant" && is_streaming_this {
            lines.push(Line::from(vec![
                Span::styled(role_icon, Style::default().fg(role_color)),
                Span::styled(status_icon, Style::default().fg(theme::text_muted())),
                Span::styled(" ".to_string(), base_style),
                Span::styled("...".to_string(), Style::default().fg(theme::text_muted()).italic()),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled(role_icon, Style::default().fg(role_color)),
                Span::styled(status_icon, Style::default().fg(theme::text_muted())),
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
                lines.push(Line::from(vec![Span::styled(" ".repeat(prefix_width), base_style)]));
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

                    let table_spans = render_markdown_table(&table_lines, base_style, wrap_width);
                    for (idx, row_spans) in table_spans.into_iter().enumerate() {
                        if is_first_line && idx == 0 {
                            let mut line_spans = vec![
                                Span::styled(role_icon.clone(), Style::default().fg(role_color)),
                                Span::styled(status_icon.clone(), Style::default().fg(theme::text_muted())),
                                Span::styled(" ".to_string(), base_style),
                            ];
                            line_spans.extend(row_spans);
                            lines.push(Line::from(line_spans));
                            is_first_line = false;
                        } else {
                            let mut line_spans = vec![Span::styled(" ".repeat(prefix_width), base_style)];
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
                            Span::styled(status_icon.clone(), Style::default().fg(theme::text_muted())),
                            Span::styled(" ".to_string(), base_style),
                        ];
                        line_spans.extend(md_spans);
                        lines.push(Line::from(line_spans));
                        is_first_line = false;
                    } else {
                        let mut line_spans = vec![Span::styled(" ".repeat(prefix_width), base_style)];
                        line_spans.extend(md_spans);
                        lines.push(Line::from(line_spans));
                    }
                }
            } else {
                // User message - wrap without markdown
                let wrapped = wrap_text(line, wrap_width);

                for line_text in &wrapped {
                    if is_first_line {
                        lines.push(Line::from(vec![
                            Span::styled(role_icon.clone(), Style::default().fg(role_color)),
                            Span::styled(status_icon.clone(), Style::default().fg(theme::text_muted())),
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

    // Dev mode: show token counts
    if dev_mode && msg.role == "assistant" && (msg.input_tokens > 0 || msg.content_token_count > 0) {
        lines.push(Line::from(vec![
            Span::styled(" ".repeat(prefix_width), base_style),
            Span::styled(
                format!("[in:{} out:{}]", msg.input_tokens, msg.content_token_count),
                Style::default().fg(theme::text_muted()).italic(),
            ),
        ]));
    }

    lines.push(Line::from(""));
    lines
}

pub(super) use super::render_input::render_input;
