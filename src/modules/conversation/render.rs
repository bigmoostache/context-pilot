use ratatui::prelude::*;
use unicode_width::UnicodeWidthStr;

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
/// Render a single message to lines (without caching logic)
pub(crate) fn render_message(
    msg: &Message,
    viewport_width: u16,
    base_style: Style,
    is_streaming_this: bool,
    dev_mode: bool,
) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    // Handle tool call messages — YAML-style parameter display
    if msg.message_type == MessageType::ToolCall {
        let icon = icons::msg_tool_call();
        let prefix_width = UnicodeWidthStr::width(icon.as_str()) + 1; // icon display width + space
        let wrap_width = (viewport_width as usize).saturating_sub(prefix_width + 2).max(20);
        for tool_use in &msg.tool_uses {
            lines.push(Line::from(vec![
                Span::styled(icon.clone(), Style::default().fg(theme::success())),
                Span::styled(" ".to_string(), base_style),
                Span::styled(tool_use.name.clone(), Style::default().fg(theme::text()).bold()),
            ]));

            let param_prefix = " ".repeat(prefix_width);
            if let Some(obj) = tool_use.input.as_object() {
                for (key, val) in obj {
                    let val_str = match val {
                        serde_json::Value::String(s) => s.clone(),
                        serde_json::Value::Null
                        | serde_json::Value::Bool(_)
                        | serde_json::Value::Number(_)
                        | serde_json::Value::Array(_)
                        | serde_json::Value::Object(_) => val.to_string(),
                    };
                    render_param_lines(&mut lines, &param_prefix, key, &val_str, wrap_width, base_style);
                }
            }
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

            let mut is_first = true;
            let mut push_with_prefix = |line_spans: Vec<Span<'static>>, lines: &mut Vec<Line<'static>>| {
                if is_first {
                    let mut full = vec![
                        Span::styled(status_icon.clone(), Style::default().fg(status_color)),
                        Span::styled(" ".to_string(), base_style),
                    ];
                    full.extend(line_spans);
                    lines.push(Line::from(full));
                    is_first = false;
                } else {
                    let mut full = vec![Span::styled(" ".repeat(prefix_width), base_style)];
                    full.extend(line_spans);
                    lines.push(Line::from(full));
                }
            };

            if let Some(vis_lines) = custom_lines {
                // Use module-provided visualization
                for vis_line in vis_lines {
                    push_with_prefix(vis_line.spans, &mut lines);
                }
            } else {
                // Fallback: plain text rendering with wrapping
                for line in result.content.lines() {
                    if line.is_empty() {
                        lines.push(Line::from(vec![Span::styled(" ".repeat(prefix_width), base_style)]));
                        continue;
                    }

                    let wrapped = wrap_text(line, wrap_width);
                    for wrapped_line in wrapped {
                        push_with_prefix(
                            vec![Span::styled(wrapped_line, Style::default().fg(theme::text_secondary()))],
                            &mut lines,
                        );
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

    let prefix = format!("{role_icon}{status_icon}");
    let prefix_width = UnicodeWidthStr::width(prefix.as_str());
    let wrap_width = (viewport_width as usize).saturating_sub(prefix_width + 2).max(20);

    if content.trim().is_empty() {
        if msg.role == "assistant" && is_streaming_this {
            lines.push(Line::from(vec![
                Span::styled(role_icon, Style::default().fg(role_color)),
                Span::styled(status_icon, Style::default().fg(theme::text_muted())),
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

/// Render a streaming tool call preview with YAML-style parameter display.
///
/// Shows the tool name and incrementally reveals parameters as they stream in.
/// Partial JSON is best-effort parsed into `key: value` lines for readability.
pub(crate) fn render_streaming_tool(
    name: &str,
    partial_json: &str,
    viewport_width: u16,
    base_style: Style,
) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    let icon = icons::msg_tool_call();
    let prefix_width = UnicodeWidthStr::width(icon.as_str()) + 1; // icon display width + space
    let wrap_width = (viewport_width as usize).saturating_sub(prefix_width + 2).max(20);

    // Tool name header
    lines.push(Line::from(vec![
        Span::styled(icon, Style::default().fg(theme::accent())),
        Span::styled(" ".to_string(), base_style),
        Span::styled(name.to_string(), Style::default().fg(theme::text()).bold()),
        Span::styled(" …".to_string(), Style::default().fg(theme::text_muted())),
    ]));

    // Try to parse partial JSON into key-value pairs for YAML-style display.
    // The JSON may be incomplete (streaming), so we do best-effort extraction.
    let param_prefix = " ".repeat(prefix_width);
    if !partial_json.is_empty() {
        for (key, val) in extract_json_fields(partial_json) {
            render_param_lines(&mut lines, &param_prefix, &key, &val, wrap_width, base_style);
        }
    }

    lines.push(Line::from(""));
    lines
}

/// Best-effort extraction of key-value pairs from potentially incomplete JSON.
///
/// Handles the common case of `{"key": "value", "key2": ...}` even when the
/// closing brace or last value is missing (still streaming).
fn extract_json_fields(partial: &str) -> Vec<(String, String)> {
    // Try full parse first — if the JSON is complete, use serde
    if let Ok(serde_json::Value::Object(map)) = serde_json::from_str(partial) {
        return map
            .into_iter()
            .map(|(k, v)| {
                let val = match v {
                    serde_json::Value::String(s) => s,
                    serde_json::Value::Null
                    | serde_json::Value::Bool(_)
                    | serde_json::Value::Number(_)
                    | serde_json::Value::Array(_)
                    | serde_json::Value::Object(_) => v.to_string(),
                };
                (k, val)
            })
            .collect();
    }

    // Incomplete JSON — hand-parse key-value pairs
    let mut fields = Vec::new();
    let mut chars = partial.char_indices().peekable();

    // Skip opening brace
    while let Some(&(_, c)) = chars.peek() {
        if c == '{' {
            let _ = chars.next();
            break;
        }
        let _ = chars.next();
    }

    loop {
        // Skip whitespace and commas
        while let Some(&(_, c)) = chars.peek() {
            if c == ' ' || c == '\n' || c == '\r' || c == '\t' || c == ',' {
                let _ = chars.next();
            } else {
                break;
            }
        }

        // Try to read a key
        let Some(key) = read_json_string(&mut chars) else { break };

        // Skip colon
        while let Some(&(_, c)) = chars.peek() {
            if c == ':' || c == ' ' {
                let _ = chars.next();
            } else {
                break;
            }
        }

        // Read value (may be incomplete)
        let val = read_json_value(&mut chars, partial);
        fields.push((key, val));
    }

    fields
}

/// Read a JSON string literal, returning the unescaped content.
fn read_json_string(chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>) -> Option<String> {
    // Expect opening quote
    if chars.peek().map(|&(_, c)| c) != Some('"') {
        return None;
    }
    let _ = chars.next(); // consume opening quote

    let mut s = String::new();
    let mut escaped = false;
    for (_, c) in chars.by_ref() {
        if escaped {
            s.push(c);
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '"' {
            return Some(s);
        } else {
            s.push(c);
        }
    }
    // Unterminated string — return what we have
    if s.is_empty() { None } else { Some(s) }
}

/// Read a JSON value (string, number, bool, array, object) from the stream.
/// For incomplete values, returns whatever was consumed.
fn read_json_value(chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>, full: &str) -> String {
    match chars.peek().map(|&(_, c)| c) {
        Some('"') => read_json_string(chars).unwrap_or_default(),
        Some(c) if c == '{' || c == '[' => {
            // Capture from current position to end (may be incomplete)
            let start = chars.peek().map_or(full.len(), |&(i, _)| i);
            // Consume remaining chars for this nested structure
            let open = c;
            let close = if c == '{' { '}' } else { ']' };
            let mut depth = 0;
            let mut end = full.len();
            for (i, ch) in chars.by_ref() {
                if ch == open {
                    depth += 1;
                } else if ch == close {
                    depth -= 1;
                    if depth == 0 {
                        end = i + ch.len_utf8();
                        break;
                    }
                }
            }
            full.get(start..end).unwrap_or("").to_string()
        }
        Some(_) => {
            // Number, bool, null — read until delimiter
            let mut val = String::new();
            while let Some(&(_, c)) = chars.peek() {
                if c == ',' || c == '}' || c == ']' || c == '\n' {
                    break;
                }
                val.push(c);
                let _ = chars.next();
            }
            val.trim().to_string()
        }
        None => String::new(),
    }
}

/// Render a parameter key-value pair as one or more lines.
///
/// Single-line values render as `prefix key: value`. Multiline values unroll
/// line by line, each continuation indented to align under the first value char.
fn render_param_lines(
    lines: &mut Vec<Line<'static>>,
    param_prefix: &str,
    key: &str,
    val: &str,
    wrap_width: usize,
    base_style: Style,
) {
    let key_span_width = key.len() + 2; // "key: "
    let val_width = wrap_width.saturating_sub(key_span_width);
    let val_lines: Vec<&str> = val.lines().collect();

    if val_lines.len() <= 1 {
        // Single-line value — truncate if too wide
        let display_val = truncate_single_line(val, val_width);
        lines.push(Line::from(vec![
            Span::styled(param_prefix.to_string(), base_style),
            Span::styled(format!("{key}: "), Style::default().fg(theme::accent())),
            Span::styled(display_val, Style::default().fg(theme::text_secondary())),
        ]));
    } else {
        // Multiline value — unroll each line with continuation indent
        let continuation = format!("{}{}", param_prefix, " ".repeat(key_span_width));
        for (i, line) in val_lines.iter().enumerate() {
            let display_line = truncate_single_line(line, val_width);
            if i == 0 {
                lines.push(Line::from(vec![
                    Span::styled(param_prefix.to_string(), base_style),
                    Span::styled(format!("{key}: "), Style::default().fg(theme::accent())),
                    Span::styled(display_line, Style::default().fg(theme::text_secondary())),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::styled(continuation.clone(), base_style),
                    Span::styled(display_line, Style::default().fg(theme::text_secondary())),
                ]));
            }
        }
    }
}

/// Truncate a single line, adding ellipsis if it exceeds the max width.
fn truncate_single_line(val: &str, max_width: usize) -> String {
    if val.len() > max_width {
        format!("{}…", &val.get(..val.floor_char_boundary(max_width.saturating_sub(1))).unwrap_or(""))
    } else {
        val.to_string()
    }
}

pub(super) use super::render_input::render_input;
