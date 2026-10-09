//! Thread message area rendering — right pane of the Threads view.
//!
//! Handles message display, input area, question form overlay, and
//! conversion helpers. All content goes through the IR pipeline.

use ratatui::Frame;
use ratatui::prelude::{Constraint, Direction, Layout, Rect, Style};
use ratatui::widgets::{Block as RBlock, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};

use cp_render::{Block as IrBlock, Semantic, Span as S};

use crate::modules::conversation::render_blocks::{MessageBlockOpts, render_message_blocks};
use crate::modules::conversation::render_input_blocks::{InputBlockCtx, render_input_blocks};
use crate::state::{Message, State};
use crate::ui::{ir, theme};
use cp_base::cast::Safe as _;
use cp_base::cast::float_math;
use cp_mod_threads::types::{FocusState, ThreadAuthor, ThreadStatus, ThreadsState};
use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash as _, Hasher as _};
use std::rc::Rc;

/// Rendered message lines for one thread, shared between frames.
type CachedLines = Rc<Vec<ratatui::text::Line<'static>>>;

thread_local! {
    /// Last built message lines and the fingerprint they were built from.
    /// Single slot: the threads view shows one thread at a time.
    static LINE_CACHE: RefCell<Option<(u64, CachedLines)>> = const { RefCell::new(None) };
    /// Rendered lines per message, keyed by [`message_key`]. When the thread
    /// changes (new message, switch back to a thread), only messages missing
    /// here are rendered again; the rest are copied.
    static MSG_CACHE: RefCell<HashMap<u64, CachedLines>> = RefCell::new(HashMap::new());
}

/// Extra entries kept in [`MSG_CACHE`] beyond the current build before pruning
/// to the current build's messages.
const MSG_CACHE_SLACK: usize = 4096;

/// Key of one message's rendered lines: everything `render_message_blocks`
/// reads from it, plus width and active theme.
fn message_key(msg: &cp_mod_threads::types::ThreadMessage, viewport_width: u16) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    viewport_width.hash(&mut h);
    std::ptr::from_ref(cp_base::config::accessors::active_theme()).addr().hash(&mut h);
    matches!(msg.author, ThreadAuthor::Assistant).hash(&mut h);
    msg.timestamp.hash(&mut h);
    msg.content.hash(&mut h);
    h.finish()
}

/// Fingerprint of everything [`build_thread_message_lines`] reads: thread id,
/// width, active theme (colors are resolved at line build time) and each
/// message's role/auto flag/content. Runs every frame, so content is NOT hashed
/// byte by byte (that was O(thread text), ~0.3 ms on long threads): each
/// message contributes its content length + heap address, which any append or
/// replacement changes. The last message (where streaming edits land) is
/// hashed in full as a guard against same-length in-place edits.
fn lines_fingerprint(thread: &cp_mod_threads::types::Thread, viewport_width: u16) -> u64 {
    let _g = crate::profile!("tv_fingerprint");
    let mut h = std::collections::hash_map::DefaultHasher::new();
    thread.id.hash(&mut h);
    viewport_width.hash(&mut h);
    std::ptr::from_ref(cp_base::config::accessors::active_theme()).addr().hash(&mut h);
    thread.messages.len().hash(&mut h);
    for msg in &thread.messages {
        msg.auto.hash(&mut h);
        matches!(msg.author, ThreadAuthor::Assistant).hash(&mut h);
        msg.timestamp.hash(&mut h);
        msg.content.as_ref().map(|c| (c.len(), c.as_ptr().addr())).hash(&mut h);
    }
    if let Some(last) = thread.messages.last() {
        last.content.hash(&mut h);
    }
    h.finish()
}

/// Message lines for `thread`, rebuilt only when its fingerprint changes.
fn cached_thread_message_lines(thread: &cp_mod_threads::types::Thread, viewport_width: u16) -> CachedLines {
    let key = lines_fingerprint(thread, viewport_width);
    LINE_CACHE.with_borrow_mut(|slot| {
        if let Some(entry) = slot.as_ref()
            && entry.0 == key
        {
            return Rc::clone(&entry.1);
        }
        let lines = {
            let _g = crate::profile!("tv_lines_build");
            Rc::new(build_thread_message_lines(thread, viewport_width))
        };
        *slot = Some((key, Rc::clone(&lines)));
        lines
    })
}

/// Render the right-pane message area with input box for the selected thread.
///
/// Messages and input render through the IR pipeline (same `render_message_blocks`
/// and `render_input_blocks` as the main conversation). Border title uses
/// `semantic_to_style` for color mapping.
pub(super) fn render_message_area_with_input(frame: &mut Frame<'_>, state: &mut State, selected: usize, area: Rect) {
    // ── Phase 1: immutable borrow of `state` — render the chrome + input, and
    // build the message lines into an owned Vec (so no borrow of `state`
    // survives into the mutable scroll phase below). ───────────────────────
    let msg_area: Rect;
    let lines: Rc<Vec<ratatui::text::Line<'static>>>;
    {
        let ts = ThreadsState::get(state);
        let Some(thread) = ts.threads.get(selected) else {
            return;
        };

        // Title: thread name + status — colors via semantic mapping
        let focus = FocusState::get(state);
        let is_focused = focus.focused_thread_id.as_deref() == Some(thread.id.as_str());
        let (status_label, status_sem) = if is_focused {
            (" [FOCUSED]", Semantic::Accent)
        } else if matches!(thread.status, ThreadStatus::MyTurn) {
            (" [MY_TURN]", Semantic::Warning)
        } else {
            (" [THEIR_TURN]", Semantic::Success)
        };

        let title = ratatui::text::Line::from(vec![
            ratatui::text::Span::styled(format!(" {} ", thread.name), ir::semantic_to_style(Semantic::Default)),
            ratatui::text::Span::styled(status_label, ir::semantic_to_style(status_sem)),
            ratatui::text::Span::raw(" "),
        ]);

        let border = RBlock::default()
            .borders(Borders::ALL)
            .border_type(ratatui::widgets::BorderType::Rounded)
            .border_style(ir::semantic_to_style(Semantic::Border))
            .title(title)
            .style(Style::default().bg(theme::bg_surface()));

        let inner = border.inner(area);
        frame.render_widget(border, area);

        // Calculate input area height based on input content (capped at 50% of area)
        let input_height = calculate_input_height(state, inner.width, inner.height);
        let messages_height = inner.height.saturating_sub(input_height);

        if messages_height == 0 {
            return;
        }

        // Split inner area: messages on top, input at bottom
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(messages_height), Constraint::Length(input_height)])
            .split(inner);

        let (Some(&m_area), Some(&input_area)) = (layout.first(), layout.get(1)) else {
            return;
        };

        lines = cached_thread_message_lines(thread, m_area.width);
        msg_area = m_area;
        let _g = crate::profile!("tv_input");
        render_thread_input(frame, state, input_area);
    }

    // ── Phase 2: mutable borrow of `state` — scroll management + paint. ─────
    let _g = crate::profile!("tv_paint");
    paint_thread_messages(frame, state, &lines, msg_area);
}

/// Build the message lines for a thread (owned, `'static`) via the conversation
/// IR renderer.
///
/// Converts `ThreadMessage` → `Message`, feeds to `render_message_blocks()`
/// (same IR path as the main conversation), converts via `blocks_to_lines()`.
/// Returned lines hold no borrow of `State`, so the caller can take a mutable
/// borrow afterwards for scroll management.
fn build_thread_message_lines(
    thread: &cp_mod_threads::types::Thread,
    viewport_width: u16,
) -> Vec<ratatui::text::Line<'static>> {
    if thread.messages.is_empty() {
        let ir_blocks =
            vec![IrBlock::Line(vec![S::muted("No messages yet. Type below to start the conversation.".to_owned())])];
        return ir::blocks_to_lines(&ir_blocks);
    }

    let opts = MessageBlockOpts { viewport_width, is_streaming: false, dev_mode: false };

    // Real messages come from MSG_CACHE (rendered once per content/width/theme);
    // only messages not seen before go through the IR renderer.
    //
    // Auto tool-activity traces (`msg.auto`) are NOT rendered as full message
    // bubbles — that would drown the real conversation in a wall of one-line
    // tool breadcrumbs. Instead each collapses to a single fully-grey line
    // (see `auto_trace_spans`). Consecutive traces stack with NO blank line
    // between them; a single blank line is emitted after each maximal *run* of
    // traces (mirroring the one real messages leave after their bubble), so a
    // block of tool calls is visually separated from the next message but its
    // internal rows stay tight.
    let mut out: Vec<ratatui::text::Line<'static>> = Vec::new();
    let mut used: Vec<u64> = Vec::new();
    let mut in_auto_run = false;
    MSG_CACHE.with_borrow_mut(|cache| {
        for msg in &thread.messages {
            if msg.auto {
                out.extend(ir::blocks_to_lines(&[IrBlock::Line(auto_trace_spans(msg))]));
                in_auto_run = true;
                continue;
            }
            if in_auto_run {
                out.extend(ir::blocks_to_lines(&[IrBlock::Empty]));
                in_auto_run = false;
            }
            let key = message_key(msg, viewport_width);
            used.push(key);
            let lines = cache.entry(key).or_insert_with(|| {
                let _g = crate::profile!("tv_msg_render");
                Rc::new(ir::blocks_to_lines(&render_message_blocks(&thread_message_to_message(msg), &opts)))
            });
            out.extend(lines.iter().cloned());
        }
        if cache.len() > used.len().saturating_add(MSG_CACHE_SLACK) {
            let keep: std::collections::HashSet<u64> = used.iter().copied().collect();
            cache.retain(|k, _| keep.contains(k));
        }
    });
    // Trailing run of traces (conversation ends on tool calls) still gets its
    // separating blank line.
    if in_auto_run {
        out.extend(ir::blocks_to_lines(&[IrBlock::Empty]));
    }
    out
}

/// Paint the pre-built message `lines` with scroll management that mirrors the
/// main conversation renderer (`render_conversation_from_ir`).
///
/// This is the fix for the threads-view scroll bugs: the pane's scroll uses the
/// shared `state.scroll_offset`, an absolute-from-top offset. While the user has
/// not grabbed the scroll (`!user_scrolled`) we keep `scroll_offset` pinned to
/// `max_scroll` (the bottom) every frame — so the first wheel-up subtracts from
/// the bottom position instead of from `0` (which read as the top → the
/// "teleports to top" bug). We also re-stick to the bottom when the user scrolls
/// back down to within half a line, and clamp `scroll_offset` into
/// `[0, max_scroll]` so over-scrolling past the bottom can no longer inflate the
/// offset and make a later scroll-up feel dead.
fn paint_thread_messages(frame: &mut Frame<'_>, state: &mut State, lines: &[ratatui::text::Line<'static>], area: Rect) {
    let viewport_height = area.height.to_usize();
    let content_height = lines.len();
    let max_scroll = content_height.saturating_sub(viewport_height).to_f32();
    state.thread_mut().max_scroll = max_scroll;

    // Reached the bottom again → resume auto-stick so new content follows.
    if state.thread().stream.user_scrolled
        && state.thread().scroll_offset.to_f64() >= float_math::sub(max_scroll.to_f64(), 0.5)
    {
        state.thread_mut().stream.user_scrolled = false;
    }
    // Auto-pinned → keep the offset synced to the bottom (prevents the
    // first-scroll teleport by starting any manual scroll from max_scroll).
    if !state.thread().stream.user_scrolled {
        state.thread_mut().scroll_offset = max_scroll;
    }
    state.thread_mut().scroll_offset = state.thread_mut().scroll_offset.clamp(0.0, max_scroll);

    let offset = state.thread().scroll_offset;
    // Clone only the visible window (no wrap on this Paragraph, so slicing is
    // identical to `.scroll()` over the full vec, minus the O(total) copy).
    let start = offset.round().to_usize().min(content_height);
    let end = start.saturating_add(viewport_height).min(content_height);
    let paragraph = Paragraph::new(lines.get(start..end).unwrap_or_default().to_vec());
    frame.render_widget(paragraph, area);

    // Scrollbar — colors via semantic mapping
    if content_height > viewport_height {
        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .style(ir::semantic_to_style(Semantic::Border))
            .thumb_style(ir::semantic_to_style(Semantic::AccentDim));
        let mut scrollbar_state = ScrollbarState::new(max_scroll.to_usize()).position(offset.round().to_usize());
        frame.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
    }
}

/// Render the input area at the bottom of the thread message area.
///
/// Separator line and input content both go through the IR pipeline.
fn render_thread_input(frame: &mut Frame<'_>, state: &State, area: Rect) {
    // Separator line via IR (border-colored, dimmed)
    let sep_area = Rect { height: 1, ..area };
    let sep_blocks = vec![IrBlock::Line(vec![S::styled("\u{2500}".repeat(area.width.into()), Semantic::Border).dim()])];
    let sep_lines = ir::blocks_to_lines(&sep_blocks);
    let sep = Paragraph::new(sep_lines);
    frame.render_widget(sep, sep_area);

    // Input content below separator — via IR pipeline
    let input_area = Rect { y: area.y.saturating_add(1), height: area.height.saturating_sub(1), ..area };

    let command_ids: Vec<String> = cp_mod_prompt::storage::load_prompts_for(cp_mod_prompt::types::PromptType::Command)
        .iter()
        .map(|p| p.id.clone())
        .collect();

    let ctx = InputBlockCtx {
        command_ids: &command_ids,
        paste_buffers: &state.thread().paste_buffers,
        paste_buffer_labels: &state.thread().paste_buffer_labels,
        viewport_width: input_area.width,
    };

    let input_blocks = render_input_blocks(
        &state.thread().composer.text,
        state.thread().composer.cursor,
        state.thread().composer.anchor,
        &ctx,
    );

    let lines = ir::blocks_to_lines(&input_blocks);
    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, input_area);
}

/// Strip the leading auto-trace marker from an auto message's content for the
/// compact dim line (the structured `auto` flag already identifies the trace,
/// so the inline `/⁠* auto *⁠/` marker is redundant in the rendered line).
fn auto_line(msg: &cp_mod_threads::types::ThreadMessage) -> String {
    const MARKER: &str = "/* auto */ ";
    let content = msg.content.as_deref().unwrap_or("");
    content.strip_prefix(MARKER).unwrap_or(content).to_owned()
}

/// Style an auto tool-activity trace as a single bubble-aligned line.
///
/// The trace body has the shape `{verb} · {tool} — {intent}` (produced by
/// `maybe_append_tool_activity`). It renders as one fully-grey, dimmed line so
/// it recedes behind the real conversation rather than competing with it:
/// - a 🔥 gutter icon followed by two spaces, so the text lines up one column
///   past where real messages place their body (bubble alignment);
/// - **verb**, **tool name** and *intent* all in muted grey (no hue, no bold —
///   the tool name used to carry an accent tint that pulled the eye; dropped);
/// - the `·` / `—` separators dimmed, the intent additionally italic as the
///   only (very subtle) structural cue.
///
/// Parsing is defensive: a missing separator degrades gracefully (the whole
/// remainder collapses into the tool-name token) rather than dropping content.
fn auto_trace_spans(msg: &cp_mod_threads::types::ThreadMessage) -> Vec<S> {
    let body = auto_line(msg);
    let (verb, rest) = body.split_once(" \u{b7} ").unwrap_or(("", body.as_str()));
    let (tool, intent) = rest.split_once(" \u{2014} ").unwrap_or((rest, ""));

    let mut spans = vec![S::styled("\u{1f525}  ".to_owned(), Semantic::Muted).dim()];
    if !verb.is_empty() {
        spans.push(S::styled(verb.to_owned(), Semantic::Muted).dim());
        spans.push(S::styled(" \u{b7} ".to_owned(), Semantic::Muted).dim());
    }
    spans.push(S::styled(tool.to_owned(), Semantic::Muted).dim());
    if !intent.is_empty() {
        spans.push(S::styled(" \u{2014} ".to_owned(), Semantic::Muted).dim());
        spans.push(S::styled(intent.to_owned(), Semantic::Muted).dim().italic());
    }
    spans
}

/// Convert a `ThreadMessage` to a `Message` for the conversation IR renderer.
fn thread_message_to_message(msg: &cp_mod_threads::types::ThreadMessage) -> Message {
    let role = if matches!(msg.author, ThreadAuthor::Assistant) { "assistant" } else { "user" };
    let content = msg.content.clone().unwrap_or_default();

    Message::new_text(String::new(), role, content).at(msg.timestamp)
}

/// Calculate input area height based on current input content.
///
/// Caps at 50% of the available height so messages remain visible.
fn calculate_input_height(state: &State, width: u16, available_height: u16) -> u16 {
    let max_input = available_height.saturating_div(2).max(3);
    if state.thread().composer.text.is_empty() {
        // Separator (1) + one line for empty input prompt
        return 3;
    }
    let line_count = state.thread().composer.text.lines().count().max(1);
    // Account for wrapping
    let wrap_width = usize::from(width).saturating_sub(10).max(20);
    let wrapped_lines: usize = state
        .thread()
        .composer
        .text
        .lines()
        .map(|l| if l.is_empty() { 1 } else { l.len().div_ceil(wrap_width).max(1) })
        .sum();
    let total = wrapped_lines.max(line_count);
    // Separator (1) + content + hint line (1), capped at 50% of available height
    (total.saturating_add(3)).min(max_input.into()).to_u16()
}
