/// IR-based message renderer: emits `Vec<Block>` instead of ratatui `Vec<Line>`.
///
/// Mirrors the logic in `render.rs` but outputs platform-agnostic IR blocks.
/// The TUI adapter converts these to ratatui via `blocks_to_lines()`.
use super::markdown_ir;

use std::collections::HashMap;
use std::sync::OnceLock;

use cp_render::{Block, Semantic, Span};

use crate::infra::constants::icons;
use crate::modules::{ToolVisualizer, build_visualizer_registry};
use crate::state::{Message, MsgKind, MsgStatus};
use crate::ui::helpers::wrap_text;

use super::render_json::extract_json_fields;

/// Lazily built registry of `tool_name` → visualizer function.
static VISUALIZER_REGISTRY: OnceLock<HashMap<String, ToolVisualizer>> = OnceLock::new();

/// Retrieve or initialize the global visualizer registry.
fn get_visualizer_registry() -> &'static HashMap<String, ToolVisualizer> {
    VISUALIZER_REGISTRY.get_or_init(build_visualizer_registry)
}

/// Display options for rendering a single conversation message.
pub(crate) struct MessageBlockOpts {
    /// Available viewport width for text wrapping.
    pub viewport_width: u16,
    /// Whether this message is currently being streamed.
    pub is_streaming: bool,
    /// Whether to show developer-mode token counts.
    pub dev_mode: bool,
}

/// Render a single message to IR blocks.
pub(crate) fn render_message_blocks(msg: &Message, opts: &MessageBlockOpts) -> Vec<Block> {
    let viewport_width = opts.viewport_width;
    let mut blocks: Vec<Block> = Vec::new();

    // Handle tool call messages — YAML-style parameter display
    if msg.msg_type == MsgKind::ToolCall {
        let icon = icons::msg_tool_call();
        let prefix_width = unicode_width::UnicodeWidthStr::width(icon.as_str()).saturating_add(1);
        let wrap_width = (viewport_width as usize).saturating_sub(prefix_width.saturating_add(2)).max(20);

        for tool_use in &msg.tool_uses {
            blocks.push(Block::line(vec![
                Span::styled(icon.clone(), Semantic::Success),
                Span::new(" ".to_owned()),
                Span::new(tool_use.name.clone()).bold(),
            ]));

            let param_prefix = " ".repeat(prefix_width);
            let param_ctx = ParamCtx { prefix: &param_prefix, wrap_width };
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
                    render_param_blocks(&mut blocks, &param_ctx, key, &val_str);
                }
            }
        }
        blocks.push(Block::empty());
        return blocks;
    }

    // Handle tool result messages
    if msg.msg_type == MsgKind::ToolResult {
        for result in &msg.tool_results {
            let (status_icon, status_semantic) = if result.is_error {
                (icons::msg_error(), Semantic::Warning)
            } else {
                (icons::msg_tool_result(), Semantic::Success)
            };

            let prefix_width: usize = 4;
            let wrap_width = (viewport_width as usize).saturating_sub(prefix_width.saturating_add(1)).max(20);

            // Check for custom module visualizer
            let registry = get_visualizer_registry();
            // Prefer display (user-facing) over content (LLM-facing) for rendering
            let render_source = result.display.as_deref().unwrap_or(&result.content);
            let custom_blocks = if result.tool_name.is_empty() {
                None
            } else {
                registry.get(&result.tool_name).map(|visualizer| visualizer(render_source, wrap_width))
            };

            let mut is_first = true;

            if let Some(vis_blocks) = custom_blocks {
                // Module-provided visualisation — flatten blocks into prefixed lines
                for vis_block in &vis_blocks {
                    match vis_block {
                        Block::Line(spans) => {
                            if is_first {
                                let mut full =
                                    vec![Span::styled(status_icon.clone(), status_semantic), Span::new(" ".to_owned())];
                                full.extend(spans.clone());
                                blocks.push(Block::line(full));
                                is_first = false;
                            } else {
                                let mut full = vec![Span::new(" ".repeat(prefix_width))];
                                full.extend(spans.clone());
                                blocks.push(Block::line(full));
                            }
                        }
                        Block::Empty => {
                            if is_first {
                                blocks.push(Block::line(vec![
                                    Span::styled(status_icon.clone(), status_semantic),
                                    Span::new(" ".to_owned()),
                                ]));
                                is_first = false;
                            } else {
                                blocks.push(Block::line(vec![Span::new(" ".repeat(prefix_width))]));
                            }
                        }
                        Block::Header(_)
                        | Block::Table { .. }
                        | Block::ProgressBar { .. }
                        | Block::Tree(_)
                        | Block::Separator
                        | Block::KeyValue(_)
                        | _ => {
                            // For complex blocks (tables etc), convert to lines first
                            let lines = crate::ui::ir::blocks_to_lines(std::slice::from_ref(vis_block));
                            for line in lines {
                                let spans: Vec<Span> = line.spans.iter().map(ratatui_span_to_ir).collect();
                                if is_first {
                                    let mut full = vec![
                                        Span::styled(status_icon.clone(), status_semantic),
                                        Span::new(" ".to_owned()),
                                    ];
                                    full.extend(spans);
                                    blocks.push(Block::line(full));
                                    is_first = false;
                                } else {
                                    let mut full = vec![Span::new(" ".repeat(prefix_width))];
                                    full.extend(spans);
                                    blocks.push(Block::line(full));
                                }
                            }
                        }
                    }
                }
            } else {
                // Fallback: plain text rendering with wrapping
                for line in render_source.lines() {
                    if line.is_empty() {
                        blocks.push(Block::line(vec![Span::new(" ".repeat(prefix_width))]));
                        continue;
                    }
                    let wrapped = wrap_text(line, wrap_width);
                    for wrapped_line in wrapped {
                        if is_first {
                            let full = vec![
                                Span::styled(status_icon.clone(), status_semantic),
                                Span::new(" ".to_owned()),
                                Span::styled(wrapped_line, Semantic::Code),
                            ];
                            blocks.push(Block::line(full));
                            is_first = false;
                        } else {
                            let full =
                                vec![Span::new(" ".repeat(prefix_width)), Span::styled(wrapped_line, Semantic::Code)];
                            blocks.push(Block::line(full));
                        }
                    }
                }
            }
        }
        blocks.push(Block::empty());
        return blocks;
    }

    // Regular text message
    let (role_icon, role_semantic) = if msg.role == "user" {
        (icons::msg_user(), Semantic::Accent)
    } else {
        (icons::msg_assistant(), Semantic::AccentDim)
    };

    let status_icon = match msg.status {
        MsgStatus::Full => icons::status_full(),
        MsgStatus::Deleted | MsgStatus::Detached => icons::status_deleted(),
    };

    let content = &msg.content;
    let prefix = format!("{role_icon}{status_icon}");
    let prefix_width = unicode_width::UnicodeWidthStr::width(prefix.as_str());
    let wrap_width = (viewport_width as usize).saturating_sub(prefix_width.saturating_add(2)).max(20);

    if content.trim().is_empty() {
        if msg.role == "assistant" && opts.is_streaming {
            blocks.push(Block::line(vec![
                Span::styled(role_icon, role_semantic),
                Span::styled(status_icon, Semantic::Muted),
                Span::styled("...".to_owned(), Semantic::Muted).italic(),
            ]));
        } else {
            blocks.push(Block::line(vec![
                Span::styled(role_icon, role_semantic),
                Span::styled(status_icon, Semantic::Muted),
            ]));
        }
    } else {
        let mut is_first_line = true;
        let is_assistant = msg.role == "assistant";
        let content_lines: Vec<&str> = content.lines().collect();
        let mut i = 0;

        while i < content_lines.len() {
            let Some(&line) = content_lines.get(i) else { break };

            if line.is_empty() {
                blocks.push(Block::line(vec![Span::new(" ".repeat(prefix_width))]));
                i = i.saturating_add(1);
                continue;
            }

            if is_assistant {
                // Check for markdown table
                if line.trim().starts_with('|') && line.trim().ends_with('|') {
                    let mut table_lines: Vec<&str> = vec![line];
                    let mut j = i.saturating_add(1);
                    while j < content_lines.len() {
                        let Some(&next) = content_lines.get(j) else { break };
                        let next_trimmed = next.trim();
                        if next_trimmed.starts_with('|') && next_trimmed.ends_with('|') {
                            table_lines.push(next);
                            j = j.saturating_add(1);
                        } else {
                            break;
                        }
                    }

                    let table_span_rows = render_markdown_table_ir(&table_lines, wrap_width);
                    for (idx, row_spans) in table_span_rows.into_iter().enumerate() {
                        if is_first_line && idx == 0 {
                            let mut line_spans = vec![
                                Span::styled(role_icon.clone(), role_semantic),
                                Span::styled(status_icon.clone(), Semantic::Muted),
                            ];
                            line_spans.extend(row_spans);
                            blocks.push(Block::line(line_spans));
                            is_first_line = false;
                        } else {
                            let mut line_spans = vec![Span::new(" ".repeat(prefix_width))];
                            line_spans.extend(row_spans);
                            blocks.push(Block::line(line_spans));
                        }
                    }

                    i = j;
                    continue;
                }

                // Regular markdown line — pre-wrap then parse
                let wrapped = wrap_text(line, wrap_width);
                for wrapped_line in &wrapped {
                    let md_spans = markdown_ir::parse_markdown_line_ir(wrapped_line);

                    if is_first_line {
                        let mut line_spans = vec![
                            Span::styled(role_icon.clone(), role_semantic),
                            Span::styled(status_icon.clone(), Semantic::Muted),
                        ];
                        line_spans.extend(md_spans);
                        blocks.push(Block::line(line_spans));
                        is_first_line = false;
                    } else {
                        let mut line_spans = vec![Span::new(" ".repeat(prefix_width))];
                        line_spans.extend(md_spans);
                        blocks.push(Block::line(line_spans));
                    }
                }
            } else {
                // User message — wrap without markdown
                let wrapped = wrap_text(line, wrap_width);

                for line_text in &wrapped {
                    if is_first_line {
                        blocks.push(Block::line(vec![
                            Span::styled(role_icon.clone(), role_semantic),
                            Span::styled(status_icon.clone(), Semantic::Muted),
                            Span::new(line_text.clone()),
                        ]));
                        is_first_line = false;
                    } else {
                        blocks
                            .push(Block::line(vec![Span::new(" ".repeat(prefix_width)), Span::new(line_text.clone())]));
                    }
                }
            }
            i = i.saturating_add(1);
        }
    }

    // Dev mode: show token counts
    if opts.dev_mode && msg.role == "assistant" && (msg.input_tokens > 0 || msg.content_token_count > 0) {
        blocks.push(Block::line(vec![
            Span::new(" ".repeat(prefix_width)),
            Span::styled(format!("[in:{} out:{}]", msg.input_tokens, msg.content_token_count), Semantic::Muted)
                .italic(),
        ]));
    }

    blocks.push(Block::empty());
    blocks
}

/// Render a streaming tool call preview as IR blocks.
pub(crate) fn render_streaming_tool_blocks(name: &str, partial_json: &str, viewport_width: u16) -> Vec<Block> {
    let mut blocks: Vec<Block> = Vec::new();

    let icon = icons::msg_tool_call();
    let prefix_width = unicode_width::UnicodeWidthStr::width(icon.as_str()).saturating_add(1);
    let wrap_width = (viewport_width as usize).saturating_sub(prefix_width.saturating_add(2)).max(20);

    // Tool name header
    blocks.push(Block::line(vec![
        Span::styled(icon, Semantic::Accent),
        Span::new(" ".to_owned()),
        Span::new(name.to_owned()).bold(),
        Span::styled(" …".to_owned(), Semantic::Muted),
    ]));

    // Parse partial JSON into key-value pairs
    let param_prefix = " ".repeat(prefix_width);
    let param_ctx = ParamCtx { prefix: &param_prefix, wrap_width };
    if !partial_json.is_empty() {
        for (key, val) in extract_json_fields(partial_json) {
            render_param_blocks(&mut blocks, &param_ctx, &key, &val);
        }
    }

    blocks.push(Block::empty());
    blocks
}

/// Context for rendering parameter key-value pairs.
struct ParamCtx<'prefix> {
    /// Indentation prefix.
    prefix: &'prefix str,
    /// Max wrap width for values.
    wrap_width: usize,
}

/// Render a parameter key-value pair as one or more blocks.
fn render_param_blocks(blocks: &mut Vec<Block>, ctx: &ParamCtx<'_>, key: &str, val: &str) {
    let key_span_width = key.len().saturating_add(2); // "key: "
    let val_width = ctx.wrap_width.saturating_sub(key_span_width);
    let val_lines: Vec<&str> = val.lines().collect();

    if val_lines.len() <= 1 {
        let display_val = truncate_single_line(val, val_width);
        blocks.push(Block::line(vec![
            Span::new(ctx.prefix.to_owned()),
            Span::styled(format!("{key}: "), Semantic::Accent),
            Span::styled(display_val, Semantic::Code),
        ]));
    } else {
        let continuation = format!("{}{}", ctx.prefix, " ".repeat(key_span_width));
        for (idx, line) in val_lines.iter().enumerate() {
            let display_line = truncate_single_line(line, val_width);
            if idx == 0 {
                blocks.push(Block::line(vec![
                    Span::new(ctx.prefix.to_owned()),
                    Span::styled(format!("{key}: "), Semantic::Accent),
                    Span::styled(display_line, Semantic::Code),
                ]));
            } else {
                blocks.push(Block::line(vec![
                    Span::new(continuation.clone()),
                    Span::styled(display_line, Semantic::Code),
                ]));
            }
        }
    }
}

/// Truncate a single line, adding ellipsis if it exceeds the max width.
fn truncate_single_line(val: &str, max_width: usize) -> String {
    if val.len() > max_width {
        format!("{}…", val.get(..val.floor_char_boundary(max_width.saturating_sub(1))).unwrap_or(""))
    } else {
        val.to_string()
    }
}

/// Convert a ratatui `Span` back to an IR `Span` (for visualizer block fallback).
fn ratatui_span_to_ir(span: &ratatui::prelude::Span<'_>) -> Span {
    let text = span.content.to_string();

    // Try to extract RGB colour from the ratatui style
    if let Some(ratatui::style::Color::Rgb(r, g, b)) = span.style.fg {
        return Span::rgb(text, r, g, b);
    }

    // Fallback: map common ratatui colors to semantics
    Span::new(text)
}

// ── Markdown table → IR spans ────────────────────────────────────────

/// Render a markdown table to IR span rows.
///
/// Delegates to the existing `markdown::render_markdown_table()` and converts
/// the `ratatui::Span` output to `cp_render::Span`. This is a bridge until
/// the markdown module is fully ported to IR.
fn render_markdown_table_ir(table_lines: &[&str], max_width: usize) -> Vec<Vec<Span>> {
    let base_style = ratatui::prelude::Style::default();
    let ratatui_rows = crate::ui::markdown::render_markdown_table(table_lines, base_style, max_width);

    ratatui_rows.into_iter().map(|row| row.iter().map(ratatui_span_to_ir).collect()).collect()
}
