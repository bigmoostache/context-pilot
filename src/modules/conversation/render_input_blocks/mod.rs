/// IR-based input area renderer: emits `Vec<Block>` instead of ratatui `Vec<Line>`.
///
/// Mirrors the logic in `render_input.rs` but outputs IR blocks.
/// Handles cursor rendering, paste placeholder expansion, command highlighting,
/// selection highlighting, and command hint display.
use cp_render::{Block, Semantic, Span};

use crate::infra::constants::icons;
use crate::ui::helpers::wrap_text;

/// Paste-sentinel expansion (split out for the 500-line structure limit).
mod sentinels;
use sentinels::expand_paste_sentinels;

/// Sentinel marker used to represent paste placeholders in the input string.
pub(super) const SENTINEL_CHAR: char = '\x00';

/// Placeholder prefix used in display text for paste placeholders.
pub(super) const PASTE_PLACEHOLDER_START: char = '\u{E000}';
/// Placeholder suffix used in display text for paste placeholders.
pub(super) const PASTE_PLACEHOLDER_END: char = '\u{E001}';

/// Contextual data needed to render the input area.
pub(crate) struct InputBlockCtx<'ctx> {
    /// Known command IDs for `/command` highlighting and hints.
    pub command_ids: &'ctx [String],
    /// Paste buffer contents (indexed by sentinel markers).
    pub paste_buffers: &'ctx [String],
    /// Optional labels for paste buffers (command names, etc.).
    pub paste_buffer_labels: &'ctx [Option<String>],
    /// Available viewport width in columns.
    pub viewport_width: u16,
}

/// Build the content spans for one wrapped input line, honoring an active
/// paste-block (rendered as a single accent run with cursor split) or normal
/// command/cursor highlighting.
fn build_line_content_spans(
    line_text: &str,
    in_paste_block: bool,
    cursor_char: &str,
    command_ids: &[String],
) -> Vec<Span> {
    if !in_paste_block {
        return build_input_spans_ir(line_text, cursor_char, command_ids);
    }
    let clean = line_text.replace([PASTE_PLACEHOLDER_START, PASTE_PLACEHOLDER_END], "");
    if clean.contains(cursor_char) {
        let parts: Vec<&str> = clean.splitn(2, cursor_char).collect();
        let first_part = parts.first().copied().unwrap_or("");
        vec![
            Span::styled(first_part.to_owned(), Semantic::Accent),
            Span::styled(cursor_char.to_owned(), Semantic::Accent).bold(),
            Span::styled(parts.get(1).unwrap_or(&"").to_string(), Semantic::Accent),
        ]
    } else {
        vec![Span::styled(clean, Semantic::Accent)]
    }
}

/// Push one input line: role-icon lead on the first line, blank indent after.
/// Flips `*is_first_line` false after the first call.
fn emit_input_line(
    blocks: &mut Vec<Block>,
    ctx: &LineRenderCtx<'_>,
    is_first_line: &mut bool,
    content_spans: Vec<Span>,
) {
    let mut line_spans = if *is_first_line {
        *is_first_line = false;
        vec![
            Span::styled(ctx.role_icon.to_owned(), Semantic::Accent),
            Span::styled("... ".to_owned(), Semantic::Accent).dim(),
            Span::new(" ".to_owned()),
        ]
    } else {
        vec![Span::new(" ".repeat(ctx.prefix_width))]
    };
    line_spans.extend(content_spans);
    blocks.push(Block::line(line_spans));
}

/// Mutable cursor state threaded across the wrapped-line emit loop.
struct LineEmitState {
    /// Whether we are currently inside a paste-placeholder block.
    in_paste_block: bool,
    /// Byte position within `input_with_cursor` (for selection mapping).
    byte_pos: usize,
    /// Whether the next emitted line is the first (gets the role-icon lead).
    is_first_line: bool,
}

/// Read-only rendering context for the wrapped-line emit loop.
struct LineRenderCtx<'ctx> {
    /// Cursor glyph inserted at the caret position.
    cursor_char: &'ctx str,
    /// Known command IDs for `/command` highlighting.
    command_ids: &'ctx [String],
    /// Role icon (user glyph) for the first line's lead.
    role_icon: &'ctx str,
    /// Blank-indent width for continuation lines.
    prefix_width: usize,
    /// Selection start byte (post-cursor-insertion coords).
    sel_start: usize,
    /// Selection end byte (post-cursor-insertion coords).
    sel_end: usize,
}

/// Process one wrapped line: toggle paste-block state, build content spans,
/// apply selection highlighting + command hints, and emit the prefixed line.
fn process_wrapped_line(blocks: &mut Vec<Block>, st: &mut LineEmitState, ctx: &LineRenderCtx<'_>, line_text: &str) {
    let has_start = line_text.contains(PASTE_PLACEHOLDER_START);
    let has_end = line_text.contains(PASTE_PLACEHOLDER_END);
    if has_start {
        st.in_paste_block = true;
    }

    let line_byte_start = st.byte_pos;
    let mut spans = build_line_content_spans(line_text, st.in_paste_block, ctx.cursor_char, ctx.command_ids);

    if has_end {
        st.in_paste_block = false;
    }

    if ctx.sel_start < ctx.sel_end {
        spans = apply_selection_to_spans(spans, line_byte_start, ctx.sel_start, ctx.sel_end);
    }

    if line_text.contains(ctx.cursor_char) && !st.in_paste_block {
        let clean_line = line_text.replace(ctx.cursor_char, "");
        spans.extend(build_command_hints_ir(&clean_line, ctx.command_ids));
    }

    emit_input_line(blocks, ctx, &mut st.is_first_line, spans);
    st.byte_pos = st.byte_pos.saturating_add(line_text.len());
}

/// Emit the non-empty input body: iterate source lines, wrap each, and emit
/// every wrapped line (blank-indent for empty lines, trailing blank on `\n`).
fn emit_input_body(blocks: &mut Vec<Block>, input_with_cursor: &str, wrap_width: usize, ctx: &LineRenderCtx<'_>) {
    let mut st = LineEmitState { in_paste_block: false, byte_pos: 0, is_first_line: true };
    for line in input_with_cursor.lines() {
        if line.is_empty() {
            blocks.push(Block::line(vec![Span::new(" ".repeat(ctx.prefix_width))]));
            st.byte_pos = st.byte_pos.saturating_add(1); // skip \n
            continue;
        }
        for line_text in &wrap_text(line, wrap_width) {
            process_wrapped_line(blocks, &mut st, ctx, line_text);
        }
        st.byte_pos = st.byte_pos.saturating_add(1); // account for \n between lines
    }
    if input_with_cursor.ends_with('\n') {
        blocks.push(Block::line(vec![Span::new(" ".repeat(ctx.prefix_width))]));
    }
}

/// Render input area to IR blocks.
pub(crate) fn render_input_blocks(
    raw_input: &str,
    raw_cursor: usize,
    raw_anchor: Option<usize>,
    ctx: &InputBlockCtx<'_>,
) -> Vec<Block> {
    let mut blocks: Vec<Block> = Vec::new();
    let role_icon = icons::msg_user();
    let prefix_width: usize = 8;
    let wrap_width = usize::from(ctx.viewport_width).saturating_sub(prefix_width.saturating_add(2)).max(20);
    let cursor_char = "\u{258e}";
    let cursor_char_len = cursor_char.len();

    // Keep originals before reassignment (needed for send-hint condition)
    let original_input = raw_input;
    let original_cursor = raw_cursor;

    // Pre-process: expand paste sentinels to display placeholders
    let (display_input, display_cursor, display_anchor) =
        expand_paste_sentinels(raw_input, raw_cursor, raw_anchor, ctx);
    let input = &display_input;
    let cursor_pos = display_cursor;

    // Insert cursor character at cursor position
    let input_with_cursor = if cursor_pos >= input.len() {
        format!("{input}{cursor_char}")
    } else {
        format!("{}{}{}", input.get(..cursor_pos).unwrap_or(""), cursor_char, input.get(cursor_pos..).unwrap_or(""))
    };

    // Compute post-cursor-insertion selection range
    let (sel_start, sel_end) = compute_post_insertion_selection(display_cursor, display_anchor, cursor_char_len);

    if input.is_empty() {
        blocks.push(Block::line(vec![
            Span::styled(role_icon, Semantic::Accent),
            Span::styled("... ".to_owned(), Semantic::Accent).dim(),
            Span::new(" ".to_owned()),
            Span::styled(cursor_char.to_owned(), Semantic::Accent),
        ]));
    } else {
        let render_ctx = LineRenderCtx {
            cursor_char,
            command_ids: ctx.command_ids,
            role_icon: &role_icon,
            prefix_width,
            sel_start,
            sel_end,
        };
        emit_input_body(&mut blocks, &input_with_cursor, wrap_width, &render_ctx);
    }

    // Show hint when next Enter will send
    let at_end = original_cursor >= original_input.len();
    let ends_with_empty_line =
        original_input.ends_with('\n') || original_input.lines().last().is_some_and(|l| l.trim().is_empty());
    if !original_input.is_empty() && at_end && ends_with_empty_line {
        blocks.push(Block::line(vec![Span::styled("  ↵ Enter to send".to_owned(), Semantic::Muted)]));
    }

    blocks.push(Block::line(vec![Span::new(String::new())]));
    blocks
}

// ── Selection helpers ────────────────────────────────────────────────

/// Compute selection range in post-cursor-insertion coordinates.
/// Returns `(sel_start, sel_end)` with `sel_start < sel_end`, or `(0, 0)` if no selection.
const fn compute_post_insertion_selection(
    display_cursor: usize,
    display_anchor: Option<usize>,
    cursor_char_len: usize,
) -> (usize, usize) {
    let Some(da) = display_anchor else { return (0, 0) };
    if display_cursor == da {
        return (0, 0);
    }
    if display_cursor < da {
        // Cursor at left edge — skip past ▎, anchor shifted right
        (display_cursor.saturating_add(cursor_char_len), da.saturating_add(cursor_char_len))
    } else {
        // Cursor at right edge — anchor unshifted, end at cursor (where ▎ starts)
        (da, display_cursor)
    }
}

/// Split a span straddling a selection boundary into up-to-three pieces
/// (before / selected-reversed / after), pushing each non-empty piece.
fn split_span_at_selection(result: &mut Vec<Span>, span: &Span, clip_start: usize, clip_end: usize) {
    // Before selection
    if clip_start > 0
        && let Some(before_text) = span.text.get(..clip_start)
        && !before_text.is_empty()
    {
        result.push(Span { text: before_text.to_owned(), ..span.clone() });
    }
    // Selected part
    if let Some(sel_text) = span.text.get(clip_start..clip_end)
        && !sel_text.is_empty()
    {
        result.push(Span { text: sel_text.to_owned(), reversed: true, ..span.clone() });
    }
    // After selection
    if let Some(after_text) = span.text.get(clip_end..)
        && !after_text.is_empty()
    {
        result.push(Span { text: after_text.to_owned(), ..span.clone() });
    }
}

/// Apply selection highlighting (reversed style) to spans within a selection range.
/// `line_offset` is the byte position of this line's text within the full `input_with_cursor`.
fn apply_selection_to_spans(spans: Vec<Span>, line_offset: usize, sel_start: usize, sel_end: usize) -> Vec<Span> {
    let mut result = Vec::new();
    let mut offset = line_offset;

    for span in spans {
        let span_start = offset;
        let span_len = span.text.len();
        let span_end = offset.saturating_add(span_len);

        if span_end <= sel_start || span_start >= sel_end || span_len == 0 {
            // Entirely outside selection or empty
            result.push(span);
        } else if span_start >= sel_start && span_end <= sel_end {
            // Entirely inside selection — set reversed
            result.push(Span { reversed: true, ..span });
        } else {
            // Partially overlapping — split at selection boundaries
            let clip_start = sel_start.saturating_sub(span_start).min(span_len);
            let clip_end = sel_end.saturating_sub(span_start).min(span_len);
            split_span_at_selection(&mut result, &span, clip_start, clip_end);
        }

        offset = span_end;
    }

    result
}

// ── Input span building ──────────────────────────────────────────────

/// Build IR spans for a single input line, with cursor and command highlighting.
fn build_input_spans_ir(line_text: &str, cursor_char: &str, command_ids: &[String]) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();

    let segments = split_paste_placeholders(line_text);
    for segment in segments {
        match segment {
            InputSegment::Text(text) => {
                spans.extend(build_text_spans_ir(&text, cursor_char, command_ids));
            }
            InputSegment::PastePlaceholder(text) => {
                if text.contains(cursor_char) {
                    let clean = text.replace(cursor_char, "");
                    spans.push(Span::styled(clean, Semantic::Active));
                    spans.push(Span::styled(cursor_char.to_owned(), Semantic::Accent).bold());
                } else {
                    spans.push(Span::styled(text, Semantic::Active));
                }
            }
        }
    }

    spans
}

/// Input segment type for splitting paste placeholders.
enum InputSegment {
    /// Normal text content.
    Text(String),
    /// Content of a paste placeholder.
    PastePlaceholder(String),
}

/// Split a line into text segments and paste placeholder segments.
fn split_paste_placeholders(line: &str) -> Vec<InputSegment> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut char_iter = line.chars();

    while let Some(next_ch) = char_iter.next() {
        if next_ch == PASTE_PLACEHOLDER_START {
            if !current.is_empty() {
                segments.push(InputSegment::Text(std::mem::take(&mut current)));
            }
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

/// Split a command line into its command part (first `matched_cmd_len` visible
/// chars) and the rest, keeping the cursor char attached to whichever side it
/// falls on.
fn split_cmd_rest(text: &str, matched_cmd_len: usize, cursor_char: &str) -> (String, String) {
    let mut cmd_part = String::new();
    let mut rest_part = String::new();
    let mut chars_consumed: usize = 0;
    let mut in_cmd = true;

    for text_ch in text.chars() {
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
    (cmd_part, rest_part)
}

/// Build IR spans for a plain text segment (no paste placeholders).
fn build_text_spans_ir(text: &str, cursor_char: &str, command_ids: &[String]) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();

    let clean_text = text.replace(cursor_char, "");
    let trimmed = clean_text.trim_start();
    let leading_spaces = clean_text.len().saturating_sub(trimmed.len());

    // Check for command
    let (matched_cmd_len, is_command) = if trimmed.starts_with('/') && !command_ids.is_empty() {
        let after_slash = trimmed.get(1..).unwrap_or("");
        let cmd_end = after_slash.find(|c: char| c.is_whitespace()).unwrap_or(after_slash.len());
        let cmd_id = after_slash.get(..cmd_end).unwrap_or("");
        if command_ids.iter().any(|id| id == cmd_id) {
            (leading_spaces.saturating_add(1).saturating_add(cmd_end), true)
        } else {
            (0, false)
        }
    } else {
        (0, false)
    };

    if is_command {
        let (cmd_part, rest_part) = split_cmd_rest(text, matched_cmd_len, cursor_char);
        push_with_cursor_ir(&mut spans, &cmd_part, cursor_char, Semantic::Accent);
        push_with_cursor_ir(&mut spans, &rest_part, cursor_char, Semantic::Default);
    } else {
        push_with_cursor_ir(&mut spans, text, cursor_char, Semantic::Default);
    }

    spans
}

/// Push text with cursor highlighting into IR spans.
fn push_with_cursor_ir(spans: &mut Vec<Span>, text: &str, cursor_char: &str, semantic: Semantic) {
    if text.contains(cursor_char) {
        let parts: Vec<&str> = text.splitn(2, cursor_char).collect();
        let first_part = parts.first().copied().unwrap_or("");
        if !first_part.is_empty() {
            spans.push(Span::styled(first_part.to_owned(), semantic));
        }
        spans.push(Span::styled(cursor_char.to_owned(), Semantic::Accent).bold());
        let second_part = parts.get(1).copied().unwrap_or("");
        if !second_part.is_empty() {
            spans.push(Span::styled(second_part.to_owned(), semantic));
        }
    } else if !text.is_empty() {
        spans.push(Span::styled(text.to_owned(), semantic));
    } else {
        // Empty text with no cursor — nothing to push.
    }
}

/// Show available command hints when user types `/` at start of a line.
fn build_command_hints_ir(clean_line: &str, command_ids: &[String]) -> Vec<Span> {
    let trimmed = clean_line.trim_start();
    if !trimmed.starts_with('/') || command_ids.is_empty() {
        return vec![];
    }
    let partial = trimmed.get(1..).unwrap_or("");
    if partial.contains(' ') {
        return vec![];
    }
    let matches: Vec<&String> = if partial.is_empty() {
        command_ids.iter().collect()
    } else {
        command_ids.iter().filter(|id| id.starts_with(partial)).collect()
    };
    if matches.len() == 1 && matches.first().map_or("", |s| s.as_str()) == partial {
        return vec![];
    }
    if matches.is_empty() {
        return vec![];
    }
    let hint_text = matches.iter().map(|id| format!("/{id}")).collect::<Vec<_>>().join("  ");
    vec![Span::new("  ".to_owned()), Span::styled(hint_text, Semantic::Muted).italic()]
}
