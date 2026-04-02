/// IR-based input area renderer: emits `Vec<Block>` instead of ratatui `Vec<Line>`.
///
/// Mirrors the logic in `render_input.rs` but outputs IR blocks.
/// Handles cursor rendering, paste placeholder expansion, command highlighting,
/// and command hint display.
use cp_render::{Block, Semantic, Span};

use crate::infra::constants::icons;
use crate::ui::helpers::wrap_text;

/// Sentinel marker used to represent paste placeholders in the input string.
const SENTINEL_CHAR: char = '\x00';

/// Placeholder prefix used in display text for paste placeholders.
const PASTE_PLACEHOLDER_START: char = '\u{E000}';
/// Placeholder suffix used in display text for paste placeholders.
const PASTE_PLACEHOLDER_END: char = '\u{E001}';

/// Contextual data needed to render the input area.
pub(crate) struct InputBlockCtx<'ctx> {
    /// Known command IDs for `/command` highlighting and hints.
    pub command_ids: &'ctx [String],
    /// Paste buffer contents (indexed by sentinel markers).
    pub paste_buffers: &'ctx [String],
    /// Optional labels for paste buffers (command names, etc.).
    pub paste_buffer_labels: &'ctx [Option<String>],
}

/// Render input area to IR blocks.
pub(super) fn render_input_blocks(
    raw_input: &str,
    raw_cursor: usize,
    viewport_width: u16,
    ctx: &InputBlockCtx<'_>,
) -> Vec<Block> {
    let mut blocks: Vec<Block> = Vec::new();
    let role_icon = icons::msg_user();
    let prefix_width: usize = 8;
    let wrap_width = (viewport_width as usize).saturating_sub(prefix_width.saturating_add(2)).max(20);
    let cursor_char = "\u{258e}";

    // Keep originals before reassignment (needed for send-hint condition)
    let original_input = raw_input;
    let original_cursor = raw_cursor;

    // Pre-process: expand paste sentinels to display placeholders
    let (display_input, display_cursor) =
        expand_paste_sentinels(raw_input, raw_cursor, ctx.paste_buffers, ctx.paste_buffer_labels);
    let input = &display_input;
    let cursor_pos = display_cursor;

    // Insert cursor character at cursor position
    let input_with_cursor = if cursor_pos >= input.len() {
        format!("{input}{cursor_char}")
    } else {
        format!("{}{}{}", input.get(..cursor_pos).unwrap_or(""), cursor_char, input.get(cursor_pos..).unwrap_or(""))
    };

    if input.is_empty() {
        blocks.push(Block::line(vec![
            Span::styled(role_icon, Semantic::Accent),
            Span::styled("... ".to_owned(), Semantic::Accent).dim(),
            Span::new(" ".to_owned()),
            Span::styled(cursor_char.to_owned(), Semantic::Accent),
        ]));
    } else {
        let mut is_first_line = true;
        let mut in_paste_block = false;

        for line in input_with_cursor.lines() {
            if line.is_empty() {
                blocks.push(Block::line(vec![Span::new(" ".repeat(prefix_width))]));
                continue;
            }

            let wrapped = wrap_text(line, wrap_width);
            for line_text in &wrapped {
                let has_start = line_text.contains(PASTE_PLACEHOLDER_START);
                let has_end = line_text.contains(PASTE_PLACEHOLDER_END);
                if has_start {
                    in_paste_block = true;
                }

                let mut spans = if in_paste_block {
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
                } else {
                    build_input_spans_ir(line_text, cursor_char, ctx.command_ids)
                };

                if has_end {
                    in_paste_block = false;
                }

                // Add command hints if this line contains the cursor and starts with /
                if line_text.contains(cursor_char) && !in_paste_block {
                    let clean_line = line_text.replace(cursor_char, "");
                    let hints = build_command_hints_ir(&clean_line, ctx.command_ids);
                    spans.extend(hints);
                }

                if is_first_line {
                    let mut line_spans = vec![
                        Span::styled(role_icon.clone(), Semantic::Accent),
                        Span::styled("... ".to_owned(), Semantic::Accent).dim(),
                        Span::new(" ".to_owned()),
                    ];
                    line_spans.extend(spans);
                    blocks.push(Block::line(line_spans));
                    is_first_line = false;
                } else {
                    let mut line_spans = vec![Span::new(" ".repeat(prefix_width))];
                    line_spans.extend(spans);
                    blocks.push(Block::line(line_spans));
                }
            }
        }
        if input_with_cursor.ends_with('\n') {
            blocks.push(Block::line(vec![Span::new(" ".repeat(prefix_width))]));
        }
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

// ── Paste sentinel expansion ─────────────────────────────────────────

/// Pre-process input string: replace sentinel markers with display placeholders.
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
                let idx_str = raw_input.get(idx_start..i).unwrap_or("");
                i = i.saturating_add(1);
                let sentinel_len = i.saturating_sub(start);

                if let Ok(idx) = idx_str.parse::<usize>() {
                    let label = paste_buffer_labels.get(idx).and_then(|l| l.as_ref());
                    let display_text = label.map_or_else(
                        || {
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
                            let content = paste_buffers.get(idx).map_or("", |s| s.as_str());
                            format!("{PASTE_PLACEHOLDER_START}⚡/{cmd_name}\n{content}{PASTE_PLACEHOLDER_END}")
                        },
                    );
                    let placeholder_len = display_text.len();

                    if raw_cursor > start {
                        if raw_cursor >= start.saturating_add(sentinel_len) {
                            new_cursor = new_cursor.saturating_add(placeholder_len).saturating_sub(sentinel_len);
                        } else {
                            new_cursor = result.len().saturating_add(placeholder_len);
                        }
                    }

                    result.push_str(&display_text);
                } else {
                    result.push_str(raw_input.get(start..i).unwrap_or(""));
                }
            } else {
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
    vec![Span::new("  ".to_owned()), Span::styled(hint_text, Semantic::Muted).italic()]
}
