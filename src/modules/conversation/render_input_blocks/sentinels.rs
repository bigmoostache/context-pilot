//! Paste-sentinel expansion for the input renderer.
//!
//! Replaces `\x00N\x00` sentinel runs in the raw input with human-readable
//! display placeholders (a `📋 Paste #N` summary or a labeled `⚡/command`
//! block), remapping the cursor + selection anchor through each substitution.
//!
//! Extracted from [`super`] to keep `render_input_blocks.rs` under the
//! 500-line structure limit.

use super::{InputBlockCtx, PASTE_PLACEHOLDER_END, PASTE_PLACEHOLDER_START, SENTINEL_CHAR};

/// Build the display placeholder text for paste buffer `idx`: a labeled command
/// block when the buffer has a label, else a `📋 Paste #N (lines, tok)` summary.
fn paste_display_text(idx: usize, paste_buffers: &[String], paste_buffer_labels: &[Option<String>]) -> String {
    let label = paste_buffer_labels.get(idx).and_then(|l| l.as_ref());
    label.map_or_else(
        || {
            let (token_count, line_count) =
                paste_buffers.get(idx).map_or((0, 0), |s| (crate::state::estimate_tokens(s), s.lines().count().max(1)));
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
    )
}

/// Byte offsets describing one sentinel-to-placeholder substitution.
struct SentinelSpan {
    /// Byte index where the sentinel starts in the raw input.
    start: usize,
    /// Length of the raw sentinel run (`\x00N\x00`).
    sentinel_len: usize,
    /// Length of the expanded display placeholder.
    placeholder_len: usize,
    /// Length of `result` so far (start of the placeholder in the output).
    result_len: usize,
}

/// Remap one cursor/anchor position across a sentinel substitution: shift past
/// the length delta when after the sentinel, or clamp to the placeholder start
/// when it fell inside.
const fn remap_position(pos: usize, span: &SentinelSpan) -> usize {
    if pos <= span.start {
        return pos;
    }
    if pos >= span.start.saturating_add(span.sentinel_len) {
        pos.saturating_add(span.placeholder_len).saturating_sub(span.sentinel_len)
    } else {
        span.result_len.saturating_add(span.placeholder_len)
    }
}

/// Accumulator threaded through sentinel expansion: the output string and the
/// remapped cursor/anchor positions.
struct ExpandState {
    /// Expanded output string built so far.
    result: String,
    /// Cursor position remapped into `result` coordinates.
    new_cursor: usize,
    /// Optional anchor position remapped into `result` coordinates.
    new_anchor: Option<usize>,
}

/// Immutable inputs for one sentinel-expansion pass.
struct SentinelInput<'ctx> {
    /// The raw input string being expanded.
    raw: &'ctx str,
    /// UTF-8 bytes of `raw` (for sentinel-run scanning).
    bytes: &'ctx [u8],
    /// Optional selection anchor to remap through the substitution.
    anchor: Option<usize>,
}

/// Consume one sentinel run starting at the current `*i` (`bytes[*i] == 0`).
/// Advances `*i` past it, appending the expanded placeholder (or the raw text on
/// a parse failure / truncation) and remapping the cursor + anchor through the
/// span.
fn consume_sentinel(input: &SentinelInput<'_>, i: &mut usize, ctx: &InputBlockCtx<'_>, st: &mut ExpandState) {
    let start = *i;
    let (raw_input, bytes, raw_anchor) = (input.raw, input.bytes, input.anchor);
    *i = start.saturating_add(1);
    let idx_start = *i;
    while *i < bytes.len() {
        let Some(&inner_byte) = bytes.get(*i) else { break };
        if inner_byte == 0 {
            break;
        }
        *i = i.saturating_add(1);
    }
    if *i >= bytes.len() {
        st.result.push_str(raw_input.get(start..).unwrap_or(""));
        return;
    }
    let idx_str = raw_input.get(idx_start..*i).unwrap_or("");
    *i = i.saturating_add(1);
    let sentinel_len = i.saturating_sub(start);

    let Ok(idx) = idx_str.parse::<usize>() else {
        st.result.push_str(raw_input.get(start..*i).unwrap_or(""));
        return;
    };
    let display_text = paste_display_text(idx, ctx.paste_buffers, ctx.paste_buffer_labels);
    let span = SentinelSpan { start, sentinel_len, placeholder_len: display_text.len(), result_len: st.result.len() };
    st.new_cursor = remap_position(st.new_cursor, &span);
    if let Some(ra) = raw_anchor {
        st.new_anchor = Some(remap_position(st.new_anchor.unwrap_or(ra), &span));
    }
    st.result.push_str(&display_text);
}

/// Pre-process input string: replace sentinel markers with display placeholders.
/// Maps cursor and optional anchor positions through the expansion.
pub(super) fn expand_paste_sentinels(
    raw_input: &str,
    raw_cursor: usize,
    raw_anchor: Option<usize>,
    ctx: &InputBlockCtx<'_>,
) -> (String, usize, Option<usize>) {
    if !raw_input.contains(SENTINEL_CHAR) {
        return (raw_input.to_owned(), raw_cursor, raw_anchor);
    }

    let mut st = ExpandState { result: String::new(), new_cursor: raw_cursor, new_anchor: raw_anchor };
    let mut i = 0;
    let bytes = raw_input.as_bytes();
    let input = SentinelInput { raw: raw_input, bytes, anchor: raw_anchor };

    while i < bytes.len() {
        let Some(&byte_val) = bytes.get(i) else { break };
        if byte_val == 0 {
            consume_sentinel(&input, &mut i, ctx, &mut st);
        } else {
            let remainder_ch = raw_input.get(i..).unwrap_or("").chars().next().unwrap_or('\0');
            st.result.push(remainder_ch);
            i = i.saturating_add(remainder_ch.len_utf8());
        }
    }

    (st.result, st.new_cursor, st.new_anchor)
}
