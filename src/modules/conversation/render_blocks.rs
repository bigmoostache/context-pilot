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
    match msg.msg_type {
        MsgKind::ToolCall => render_tool_call_blocks(msg, opts.viewport_width),
        MsgKind::ToolResult => render_tool_result_blocks(msg, opts.viewport_width),
        MsgKind::TextMessage => render_text_message_blocks(msg, opts),
    }
}

/// Render a `ToolCall` message: icon + bold tool name, then YAML-style key/value
/// parameter lines (values wrap, never truncate).
fn render_tool_call_blocks(msg: &Message, viewport_width: u16) -> Vec<Block> {
    let mut blocks: Vec<Block> = Vec::new();
    let icon = icons::msg_tool_call();
    let prefix_width = unicode_width::UnicodeWidthStr::width(icon.as_str()).saturating_add(1);
    let wrap_width = usize::from(viewport_width).saturating_sub(prefix_width.saturating_add(2)).max(20);

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
                let val_str = val.as_str().map_or_else(|| val.to_string(), str::to_owned);
                render_param_blocks(&mut blocks, &param_ctx, key, &val_str);
            }
        }
    }
    blocks.push(Block::empty());
    blocks
}

/// Render a `ToolResult` message: status icon + either a module visualizer's
/// blocks (flattened, prefixed) or a plain wrapped-text fallback.
fn render_tool_result_blocks(msg: &Message, viewport_width: u16) -> Vec<Block> {
    let mut blocks: Vec<Block> = Vec::new();
    for result in &msg.tool_results {
        let (status_icon, status_semantic) = if result.is_error {
            (icons::msg_error(), Semantic::Warning)
        } else {
            (icons::msg_tool_result(), Semantic::Success)
        };

        let prefix_width: usize = 4;
        let wrap_width = usize::from(viewport_width).saturating_sub(prefix_width.saturating_add(1)).max(20);

        // Check for custom module visualizer
        let registry = get_visualizer_registry();
        // Prefer display (user-facing) over content (LLM-facing) for rendering
        let render_source = result.display.as_deref().unwrap_or(&result.content);
        let custom_blocks = if result.tool_name.is_empty() {
            None
        } else {
            registry.get(&result.tool_name).map(|visualizer| visualizer(render_source, wrap_width))
        };

        let ctx = ResultLineCtx { status_icon: &status_icon, status_semantic, prefix_width };
        if let Some(vis_blocks) = custom_blocks {
            flatten_visualizer_blocks(&mut blocks, &vis_blocks, &ctx);
        } else {
            render_plain_result_text(&mut blocks, render_source, wrap_width, &ctx);
        }
    }
    blocks.push(Block::empty());
    blocks
}

/// Shared prefix context for tool-result line emission.
struct ResultLineCtx<'ctx> {
    /// Status glyph shown on the first result line.
    status_icon: &'ctx str,
    /// Semantic color for the status glyph (success / warning).
    status_semantic: Semantic,
    /// Blank-indent width for continuation lines.
    prefix_width: usize,
}

impl ResultLineCtx<'_> {
    /// Leading spans for a result line: status icon on the first line, blank
    /// indent afterward. Flips `*is_first` to false after the first call.
    fn lead(&self, is_first: &mut bool) -> Vec<Span> {
        if *is_first {
            *is_first = false;
            vec![Span::styled(self.status_icon.to_owned(), self.status_semantic), Span::new(" ".to_owned())]
        } else {
            vec![Span::new(" ".repeat(self.prefix_width))]
        }
    }
}

/// Flatten a module visualizer's blocks into prefixed result lines. `Line`/`Empty`
/// blocks get the status-icon lead; complex blocks pass through after an icon line.
fn flatten_visualizer_blocks(blocks: &mut Vec<Block>, vis_blocks: &[Block], ctx: &ResultLineCtx<'_>) {
    let mut is_first = true;
    for vis_block in vis_blocks {
        match vis_block.clone() {
            Block::Line(spans) => {
                let mut full = ctx.lead(&mut is_first);
                full.extend(spans);
                blocks.push(Block::line(full));
            }
            Block::Empty => blocks.push(Block::line(ctx.lead(&mut is_first))),
            Block::Header(_)
            | Block::Table { .. }
            | Block::ProgressBar { .. }
            | Block::Tree(_)
            | Block::Separator
            | Block::KeyValue(_)
            | _ => {
                // Complex blocks (Table, Tree, …) — emit a bare icon line first
                // (no current visualizer produces these), then pass through.
                if is_first {
                    blocks.push(Block::line(ctx.lead(&mut is_first)));
                }
                blocks.push(vis_block.clone());
            }
        }
    }
}

/// Fallback: render tool-result text as plain wrapped `Code`-styled lines.
fn render_plain_result_text(blocks: &mut Vec<Block>, source: &str, wrap_width: usize, ctx: &ResultLineCtx<'_>) {
    let mut is_first = true;
    for line in source.lines() {
        if line.is_empty() {
            blocks.push(Block::line(vec![Span::new(" ".repeat(ctx.prefix_width))]));
            continue;
        }
        for wrapped_line in wrap_text(line, wrap_width) {
            let mut full = ctx.lead(&mut is_first);
            full.push(Span::styled(wrapped_line, Semantic::Code));
            blocks.push(Block::line(full));
        }
    }
}

/// Render a regular text message: role/status icon prefix, then body (markdown
/// for assistant with fenced-code + table handling, plain wrap for user).
fn render_text_message_blocks(msg: &Message, opts: &MessageBlockOpts) -> Vec<Block> {
    let viewport_width = opts.viewport_width;
    let mut blocks: Vec<Block> = Vec::new();

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
    let wrap_width = usize::from(viewport_width).saturating_sub(prefix_width.saturating_add(2)).max(20);

    if content.trim().is_empty() {
        let mut spans = vec![Span::styled(role_icon, role_semantic), Span::styled(status_icon, Semantic::Muted)];
        if msg.role == "assistant" && opts.is_streaming {
            spans.push(Span::styled("...".to_owned(), Semantic::Muted).italic());
        }
        blocks.push(Block::line(spans));
    } else {
        let ctx =
            TextBodyCtx { role_icon: &role_icon, status_icon: &status_icon, role_semantic, prefix_width, wrap_width };
        render_text_body(&mut blocks, content, msg.role == "assistant", &ctx);
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

/// Shared prefix context for text-body line emission.
struct TextBodyCtx<'ctx> {
    /// Role glyph (user / assistant) shown on the first body line.
    role_icon: &'ctx str,
    /// Status glyph shown beside the role glyph on the first line.
    status_icon: &'ctx str,
    /// Semantic color for the role glyph.
    role_semantic: Semantic,
    /// Blank-indent width for continuation lines.
    prefix_width: usize,
    /// Max wrap width for body text.
    wrap_width: usize,
}

impl TextBodyCtx<'_> {
    /// Leading spans for a body line: role icon + muted status icon on the first
    /// line, blank indent afterward. Flips `*is_first` to false after first call.
    fn lead(&self, is_first: &mut bool) -> Vec<Span> {
        if *is_first {
            *is_first = false;
            vec![
                Span::styled(self.role_icon.to_owned(), self.role_semantic),
                Span::styled(self.status_icon.to_owned(), Semantic::Muted),
            ]
        } else {
            vec![Span::new(" ".repeat(self.prefix_width))]
        }
    }
}

/// Position + first-line state threaded through the text-body render loop.
struct MdScan {
    /// Index of the next source line to process.
    idx: usize,
    /// Whether the next emitted line is the first (gets the role-icon lead).
    is_first: bool,
}

/// Consume a contiguous markdown table starting at `scan.idx` (all `|…|` rows),
/// emit its rendered rows, and advance `scan.idx` past the table.
fn consume_markdown_table(blocks: &mut Vec<Block>, ctx: &TextBodyCtx<'_>, lines: &[&str], scan: &mut MdScan) {
    let Some(&first) = lines.get(scan.idx) else { return };
    let mut table_lines: Vec<&str> = vec![first];
    let mut j = scan.idx.saturating_add(1);
    while let Some(&next) = lines.get(j) {
        let t = next.trim();
        if t.starts_with('|') && t.ends_with('|') {
            table_lines.push(next);
            j = j.saturating_add(1);
        } else {
            break;
        }
    }
    for row_spans in render_markdown_table_ir(&table_lines, ctx.wrap_width) {
        push_prefixed(blocks, ctx, &mut scan.is_first, row_spans);
    }
    scan.idx = j;
}

/// Render one assistant markdown line: wrap, then parse each wrapped segment.
fn render_markdown_line(blocks: &mut Vec<Block>, ctx: &TextBodyCtx<'_>, line: &str, is_first: &mut bool) {
    for wrapped_line in &wrap_text(line, ctx.wrap_width) {
        push_prefixed(blocks, ctx, is_first, markdown_ir::parse_markdown_line_ir(wrapped_line));
    }
}

/// Render one user line: wrap verbatim, no markdown parsing.
fn render_user_line(blocks: &mut Vec<Block>, ctx: &TextBodyCtx<'_>, line: &str, is_first: &mut bool) {
    for line_text in &wrap_text(line, ctx.wrap_width) {
        push_prefixed(blocks, ctx, is_first, vec![Span::new(line_text.clone())]);
    }
}

/// Render the non-empty body of a text message. Assistant bodies get fenced-code,
/// markdown-table, and inline-markdown handling; user bodies wrap verbatim.
fn render_text_body(blocks: &mut Vec<Block>, content: &str, is_assistant: bool, ctx: &TextBodyCtx<'_>) {
    let mut scan = MdScan { idx: 0, is_first: true };
    let content_lines: Vec<&str> = content.lines().collect();
    let mut in_code_block = false;

    while scan.idx < content_lines.len() {
        let Some(&line) = content_lines.get(scan.idx) else { break };

        if is_assistant && line.trim().starts_with("```") {
            in_code_block = !in_code_block;
            push_prefixed(blocks, ctx, &mut scan.is_first, vec![Span::styled(line.to_owned(), Semantic::Muted)]);
            scan.idx = scan.idx.saturating_add(1);
            continue;
        }
        if in_code_block {
            push_prefixed(blocks, ctx, &mut scan.is_first, vec![Span::styled(line.to_owned(), Semantic::Code)]);
            scan.idx = scan.idx.saturating_add(1);
            continue;
        }
        if line.is_empty() {
            blocks.push(Block::line(vec![Span::new(" ".repeat(ctx.prefix_width))]));
            scan.idx = scan.idx.saturating_add(1);
            continue;
        }

        if is_assistant {
            if line.trim().starts_with('|') && line.trim().ends_with('|') {
                consume_markdown_table(blocks, ctx, &content_lines, &mut scan);
                continue;
            }
            render_markdown_line(blocks, ctx, line, &mut scan.is_first);
        } else {
            render_user_line(blocks, ctx, line, &mut scan.is_first);
        }
        scan.idx = scan.idx.saturating_add(1);
    }
}

/// Push one body line: `ctx.lead()` prefix spans followed by `content_spans`.
fn push_prefixed(blocks: &mut Vec<Block>, ctx: &TextBodyCtx<'_>, is_first: &mut bool, content_spans: Vec<Span>) {
    let mut line_spans = ctx.lead(is_first);
    line_spans.extend(content_spans);
    blocks.push(Block::line(line_spans));
}

/// Render a streaming tool call preview as IR blocks.
pub(crate) fn render_streaming_tool_blocks(name: &str, partial_json: &str, viewport_width: u16) -> Vec<Block> {
    let mut blocks: Vec<Block> = Vec::new();

    let icon = icons::msg_tool_call();
    let prefix_width = unicode_width::UnicodeWidthStr::width(icon.as_str()).saturating_add(1);
    let wrap_width = usize::from(viewport_width).saturating_sub(prefix_width.saturating_add(2)).max(20);

    // Tool name header
    blocks.push(Block::line(vec![
        Span::styled(icon, Semantic::Accent),
        Span::new(" ".to_owned()),
        Span::new(name.to_owned()).bold(),
        Span::styled(" \u{2026}".to_owned(), Semantic::Muted),
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

/// Render a parameter key-value pair as one or more blocks (wraps instead of truncating).
fn render_param_blocks(blocks: &mut Vec<Block>, ctx: &ParamCtx<'_>, key: &str, val: &str) {
    let key_span_width = key.len().saturating_add(2); // "key: "
    let val_width = ctx.wrap_width.saturating_sub(key_span_width).max(10);
    let continuation = format!("{}{}", ctx.prefix, " ".repeat(key_span_width));
    let mut is_first = true;

    for source_line in val.lines() {
        let wrapped = wrap_text(source_line, val_width);
        // wrap_text returns empty vec for empty lines
        let wrapped = if wrapped.is_empty() { vec![String::new()] } else { wrapped };
        for wrapped_line in &wrapped {
            if is_first {
                blocks.push(Block::line(vec![
                    Span::new(ctx.prefix.to_owned()),
                    Span::styled(format!("{key}: "), Semantic::Accent),
                    Span::styled(wrapped_line.clone(), Semantic::Code),
                ]));
                is_first = false;
            } else {
                blocks.push(Block::line(vec![
                    Span::new(continuation.clone()),
                    Span::styled(wrapped_line.clone(), Semantic::Code),
                ]));
            }
        }
    }

    // Handle empty val (no lines at all)
    if is_first {
        blocks.push(Block::line(vec![
            Span::new(ctx.prefix.to_owned()),
            Span::styled(format!("{key}: "), Semantic::Accent),
        ]));
    }
}

// ── Markdown table → IR spans ────────────────────────────────────────

/// Render a markdown table to IR span rows.
fn render_markdown_table_ir(table_lines: &[&str], max_width: usize) -> Vec<Vec<Span>> {
    crate::ui::markdown::render_markdown_table(table_lines, max_width)
}
