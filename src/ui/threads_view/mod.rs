//! Threads view — dedicated TUI layout when `ViewMode::Threads` is active.
//!
//! Renders a two-pane layout: thread list (left) + message area (right).
//! All content goes through the IR pipeline (`cp_render::Block` / `Span` →
//! `blocks_to_lines()` → ratatui). Only layout chrome (borders, scrollbars,
//! area splits) uses ratatui directly — same pattern as the sidebar adapter.
//!
//! Sub-module [`messages`] handles the right-pane message area, input, and
//! question form rendering.

/// Message area rendering: messages, input, question form.
pub(super) mod messages;

use ratatui::Frame;
use ratatui::prelude::{Constraint, Direction, Layout, Rect, Style};
use ratatui::widgets::{Block as RBlock, Borders, Paragraph};

use cp_render::{Block as IrBlock, Semantic, Span as S};

use crate::state::State;
use crate::ui::{ir, theme};
use cp_base::cast::Safe as _;
use cp_mod_threads::types::{FocusState, ThreadStatus, ThreadsState};

/// Width of the thread list pane in columns.
pub(crate) const THREAD_LIST_WIDTH: u16 = 28;

/// Render the threads view: thread list + message area.
pub(crate) fn render_threads_view(frame: &mut Frame<'_>, state: &State, area: Rect) {
    let threads_state = ThreadsState::get(state);
    let focus_state = FocusState::get(state);
    let viewing_archived = focus_state.viewing_archived;

    // The visible list is the subset matching the current view. The active
    // view has a trailing virtual "+ New Thread" entry; the archived view
    // does not.
    let visible = threads_state.visible_indices(viewing_archived);
    let show_new = !viewing_archived;
    let total_entries = visible.len().saturating_add(usize::from(show_new));
    let selected_idx = focus_state.selected_thread_idx.min(total_entries.saturating_sub(1));
    let on_virtual_new = show_new && selected_idx >= visible.len();

    // Two-pane layout: thread list | message area
    if area.width > THREAD_LIST_WIDTH.saturating_add(20) {
        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(THREAD_LIST_WIDTH), Constraint::Min(1)])
            .split(area);

        let (Some(&list_area), Some(&msg_area)) = (layout.first(), layout.get(1)) else {
            return;
        };
        render_thread_list(frame, state, list_area);
        if on_virtual_new {
            render_new_thread_prompt(frame, state, msg_area);
        } else if let Some(&real_idx) = visible.get(selected_idx) {
            messages::render_message_area_with_input(frame, state, real_idx, msg_area);
        } else {
            // No thread selected — leave the message area blank.
        }
    } else {
        // Narrow terminal — show thread list only
        render_thread_list(frame, state, area);
    }
}

/// Selected-entry line range within the thread-list IR blocks.
#[derive(Default)]
struct SelRange {
    /// First block index of the selected entry.
    start: Option<usize>,
    /// One-past-last block index of the selected entry.
    end: Option<usize>,
}

/// Push the archived-mode header tag. No-op in the active view.
fn push_archived_header(blocks: &mut Vec<IrBlock>, viewing_archived: bool) {
    if viewing_archived {
        blocks.push(IrBlock::Line(vec![
            S::styled("  ".to_owned(), Semantic::Muted),
            S::styled("⌗ ARCHIVED".to_owned(), Semantic::AccentDim),
        ]));
        blocks.push(IrBlock::Empty);
    }
}

/// Mutable accumulator threaded through the thread-list block builders:
/// the growing IR block vec, the selected-entry range, and the pane width.
struct ListBuild<'build> {
    /// Growing IR block list for the thread list.
    blocks: &'build mut Vec<IrBlock>,
    /// Selected-entry line range, filled as entries are pushed.
    sel: &'build mut SelRange,
    /// Inner pane width (columns) for name truncation.
    inner_width: u16,
}

/// Push the virtual "+ New Thread" entry (active view only, 2-line format),
/// recording its line range into `sel` when it is the selected entry.
fn push_new_thread_entry(lb: &mut ListBuild<'_>, state: &State, on_virtual: bool) {
    let new_sem = if on_virtual { Semantic::Accent } else { Semantic::Muted };
    if on_virtual {
        lb.sel.start = Some(lb.blocks.len());
    }
    let new_name = if on_virtual && !state.input.is_empty() {
        truncate_str(&state.input, lb.inner_width.saturating_sub(6).into())
    } else {
        "New Thread".to_owned()
    };
    lb.blocks.push(IrBlock::Line(vec![
        S::styled("  ".to_owned(), new_sem),
        S::styled("● ".to_owned(), new_sem),
        S::styled(new_name, new_sem),
    ]));
    lb.blocks.push(IrBlock::Line(vec![S::new("  ".to_owned()), S::styled("[NEW THREAD]".to_owned(), new_sem)]));
    if on_virtual {
        lb.sel.end = Some(lb.blocks.len());
    }
}

/// Semantic + badge for one thread's status (focused wins over turn state).
const fn thread_status_style(thread: &cp_mod_threads::types::Thread, is_focused: bool) -> (Semantic, &'static str) {
    if is_focused {
        return (Semantic::Accent, "[FOCUSED]");
    }
    match thread.status {
        ThreadStatus::MyTurn => (Semantic::Warning, "[MY_TURN]"),
        ThreadStatus::TheirTurn => (Semantic::Success, "[THEIR_TURN]"),
    }
}

/// Push one thread's 2-line entry, recording its selected line range.
fn push_thread_entry(
    lb: &mut ListBuild<'_>,
    thread: &cp_mod_threads::types::Thread,
    focus: &FocusState,
    is_selected: bool,
) {
    let last_read = focus.last_read_count.get(&thread.id).copied().unwrap_or(0);
    let has_unread = thread.messages.len() > last_read;
    let is_focused = focus.focused_thread_id.as_deref() == Some(thread.id.as_str());
    let (status_sem, badge) = thread_status_style(thread, is_focused);

    let indicator = if has_unread && !is_selected { "● " } else { "  " };
    let indicator_sem = if has_unread { Semantic::Warning } else { Semantic::Default };
    let name = truncate_str(&thread.name, lb.inner_width.saturating_sub(6).into());

    if is_selected {
        lb.sel.start = Some(lb.blocks.len());
    }
    lb.blocks.push(IrBlock::Line(vec![
        S::styled(indicator.to_owned(), indicator_sem),
        S::styled("● ".to_owned(), status_sem),
        S::new(name),
    ]));
    lb.blocks.push(IrBlock::Line(vec![
        S::new("  ".to_owned()),
        S::styled(badge.to_owned(), status_sem),
        S::muted(format!("  {} msg", thread.messages.len())),
    ]));
    if is_selected {
        lb.sel.end = Some(lb.blocks.len());
    }
}

/// Push the bottom help / confirm hint line for the current mode.
fn push_help_hint(blocks: &mut Vec<IrBlock>, viewing_archived: bool, confirming: bool) {
    if confirming {
        let (verb, key_sem) =
            if viewing_archived { (" Restore? ", Semantic::KeyHint) } else { (" Archive? ", Semantic::KeyHint) };
        blocks.push(IrBlock::Line(vec![
            S::warning(verb.to_owned()),
            S::styled("y".to_owned(), key_sem),
            S::muted("/any to cancel".to_owned()),
        ]));
    } else if viewing_archived {
        blocks.push(IrBlock::Line(vec![
            S::styled(" Ctrl+A".to_owned(), Semantic::KeyHint),
            S::muted(" restore  ".to_owned()),
            S::styled("Ctrl+U".to_owned(), Semantic::KeyHint),
            S::muted(" active".to_owned()),
        ]));
    } else {
        blocks.push(IrBlock::Line(vec![
            S::styled(" Ctrl+A".to_owned(), Semantic::KeyHint),
            S::muted(" arch  ".to_owned()),
            S::styled("Ctrl+U".to_owned(), Semantic::KeyHint),
            S::muted(" arch'd  ".to_owned()),
            S::styled("Ctrl+V".to_owned(), Semantic::KeyHint),
            S::muted(" back".to_owned()),
        ]));
    }
}

/// Apply the surface-bg highlight (full width) to the selected entry's lines.
fn apply_selection_highlight(lines: &mut [ratatui::text::Line<'static>], sel: &SelRange, inner_width: u16) {
    let (Some(start), Some(end)) = (sel.start, sel.end) else {
        return;
    };
    let full_width = inner_width.to_usize();
    for line in lines.get_mut(start..end).into_iter().flatten() {
        line.style = line.style.bg(theme::bg_surface());
        let current_width: usize = line.spans.iter().map(ratatui::text::Span::width).sum();
        if current_width < full_width {
            line.spans.push(ratatui::text::Span::styled(
                " ".repeat(full_width.saturating_sub(current_width)),
                Style::default().bg(theme::bg_surface()),
            ));
        }
    }
}

/// Render the left-pane thread list via IR blocks.
///
/// Layout chrome (right border) is direct ratatui. All visible content
/// (thread entries, indicators, help hints) goes through IR → `blocks_to_lines()`.
fn render_thread_list(frame: &mut Frame<'_>, state: &State, area: Rect) {
    let ts = ThreadsState::get(state);
    let focus = FocusState::get(state);
    let viewing_archived = focus.viewing_archived;
    let visible = ts.visible_indices(viewing_archived);
    let show_new = !viewing_archived; // virtual "+ New Thread" only in the active view
    let total_entries = visible.len().saturating_add(usize::from(show_new));
    let selected = focus.selected_thread_idx.min(total_entries.saturating_sub(1));
    let confirming = focus.confirming_archive;

    // Layout chrome: border on right side
    let border = RBlock::default()
        .borders(Borders::RIGHT)
        .border_style(ir::semantic_to_style(Semantic::Border))
        .style(Style::default().bg(theme::bg_base()));
    let inner = border.inner(area);
    frame.render_widget(border, area);

    // ── Build IR blocks ──────────────────────────────────────────────
    let mut ir_blocks: Vec<IrBlock> = Vec::new();
    let mut sel = SelRange::default();

    push_archived_header(&mut ir_blocks, viewing_archived);

    let on_virtual = show_new && selected >= visible.len();
    let mut lb = ListBuild { blocks: &mut ir_blocks, sel: &mut sel, inner_width: inner.width };
    if show_new {
        push_new_thread_entry(&mut lb, state, on_virtual);
    }

    // `i` is the position in the visible slice; `real` indexes `ts.threads`.
    for (i, &real) in visible.iter().enumerate() {
        let Some(thread) = ts.threads.get(real) else {
            continue;
        };
        push_thread_entry(&mut lb, thread, focus, i == selected);
    }

    // Empty-state hint when the archived list has nothing in it.
    if viewing_archived && visible.is_empty() {
        ir_blocks.push(IrBlock::Line(vec![S::muted("  (no archived threads)".to_owned())]));
    }

    // Pad to push help hints to the bottom
    let content_lines = ir_blocks.len();
    let help_y = inner.height.saturating_sub(1).to_usize();
    if help_y > content_lines {
        for _ in 0..help_y.saturating_sub(content_lines) {
            ir_blocks.push(IrBlock::Empty);
        }
    }

    push_help_hint(&mut ir_blocks, viewing_archived, confirming);

    // Convert IR → ratatui and render
    let mut lines = ir::blocks_to_lines(&ir_blocks);
    apply_selection_highlight(&mut lines, &sel, inner.width);

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

/// Render the right pane when the virtual "New Thread" entry is selected.
///
/// Shows a prompt inviting the user to type a thread name in the input area.
/// Border + title are layout chrome; content goes through IR pipeline.
fn render_new_thread_prompt(frame: &mut Frame<'_>, state: &State, area: Rect) {
    let border = RBlock::default()
        .borders(Borders::ALL)
        .border_style(ir::semantic_to_style(Semantic::Border))
        .title(ratatui::text::Span::styled(" New Thread ", ir::semantic_to_style(Semantic::Accent)))
        .style(Style::default().bg(theme::bg_base()));

    let inner = border.inner(area);
    frame.render_widget(border, area);

    let input_preview = if state.input.is_empty() { "…".to_owned() } else { state.input.clone() };

    let ir_blocks = vec![
        IrBlock::Empty,
        IrBlock::Line(vec![S::muted("Type a name for the new thread below,".to_owned())]),
        IrBlock::Line(vec![S::muted("then press Enter to create it.".to_owned())]),
        IrBlock::Empty,
        IrBlock::Line(vec![S::accent("  ➜ ".to_owned()), S::new(input_preview)]),
    ];

    let lines = ir::blocks_to_lines(&ir_blocks);
    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

/// Truncate a string to `max_len` characters, appending "…" if truncated.
pub(super) fn truncate_str(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_owned()
    } else {
        let mut result: String = s.chars().take(max_len.saturating_sub(1)).collect();
        result.push('…');
        result
    }
}
