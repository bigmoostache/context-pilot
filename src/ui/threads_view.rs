//! Threads view — dedicated TUI layout when `ViewMode::Threads` is active.
//!
//! Renders a two-pane layout: thread list (left) + message area (right).
//! All content goes through the IR pipeline (`cp_render::Block` / `Span` →
//! `blocks_to_lines()` → ratatui). Only layout chrome (borders, scrollbars,
//! area splits) uses ratatui directly — same pattern as the sidebar adapter.

use ratatui::Frame;
use ratatui::prelude::{Constraint, Direction, Layout, Rect, Style};
use ratatui::widgets::{
    Block as RBlock, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};

use cp_render::{Block as IrBlock, Semantic, Span as S};

use crate::modules::conversation::render_blocks::{MessageBlockOpts, render_message_blocks};
use crate::modules::conversation::render_input_blocks::{InputBlockCtx, render_input_blocks};
use crate::state::{Message, MsgKind, MsgStatus, State};
use crate::ui::{ir, theme};
use cp_base::cast::Safe as _;
use cp_mod_threads::types::{FocusState, ThreadAuthor, ThreadStatus, ThreadsState};

/// Width of the thread list pane in columns.
pub(crate) const THREAD_LIST_WIDTH: u16 = 28;

/// Render the threads view: thread list + message area.
pub(crate) fn render_threads_view(frame: &mut Frame<'_>, state: &State, area: Rect) {
    let threads_state = ThreadsState::get(state);
    let focus_state = FocusState::get(state);

    // Clamp selected index — allow one past end for virtual "New Thread" entry
    let total_entries = threads_state.threads.len().saturating_add(1);
    let selected_idx = focus_state
        .selected_thread_idx
        .min(total_entries.saturating_sub(1));
    let on_virtual_new = selected_idx >= threads_state.threads.len();

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
        } else {
            render_message_area_with_input(frame, state, selected_idx, msg_area);
        }
    } else {
        // Narrow terminal — show thread list only
        render_thread_list(frame, state, area);
    }
}

/// Render the left-pane thread list via IR blocks.
///
/// Layout chrome (right border) is direct ratatui. All visible content
/// (thread entries, indicators, help hints) goes through IR → `blocks_to_lines()`.
fn render_thread_list(frame: &mut Frame<'_>, state: &State, area: Rect) {
    let ts = ThreadsState::get(state);
    let focus = FocusState::get(state);
    let total_entries = ts.threads.len().saturating_add(1); // +1 for virtual entry
    let selected = focus
        .selected_thread_idx
        .min(total_entries.saturating_sub(1));
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

    for (i, thread) in ts.threads.iter().enumerate() {
        let is_selected = i == selected;

        // Unread detection: messages beyond last-read count
        let last_read = focus
            .last_read_count
            .get(&thread.id)
            .copied()
            .unwrap_or(0);
        let has_unread = thread.messages.len() > last_read;

        // Semantic for status: accent=focused, warning=MY_TURN, success=THEIR_TURN
        let is_focused = focus.focused_thread_id.as_deref() == Some(thread.id.as_str());
        let status_sem = if is_focused {
            Semantic::Accent
        } else {
            match thread.status {
                ThreadStatus::MyTurn => Semantic::Warning,
                ThreadStatus::TheirTurn => Semantic::Success,
            }
        };

        // Line 1: indicator + status dot + thread name
        let indicator = if is_selected {
            "▸ "
        } else if has_unread {
            "● "
        } else {
            "  "
        };
        let indicator_sem = if has_unread && !is_selected {
            Semantic::Warning
        } else {
            Semantic::Accent
        };
        let name = truncate_str(&thread.name, inner.width.saturating_sub(6).into());

        let mut line1 = vec![
            S::styled(indicator.to_owned(), indicator_sem),
            S::styled("● ".to_owned(), status_sem),
            S::new(name),
        ];
        if is_selected {
            for span in &mut line1 {
                span.reversed = true;
            }
        }
        ir_blocks.push(IrBlock::Line(line1));

        // Line 2: status badge + message count
        let (badge, badge_sem) = if is_focused {
            ("[FOCUSED]", Semantic::Accent)
        } else {
            match thread.status {
                ThreadStatus::MyTurn => ("[MY_TURN]", Semantic::Warning),
                ThreadStatus::TheirTurn => ("[THEIR_TURN]", Semantic::Success),
            }
        };
        ir_blocks.push(IrBlock::Line(vec![
            S::new("  ".to_owned()),
            S::styled(badge.to_owned(), badge_sem),
            S::muted(format!("  {} msg", thread.messages.len())),
        ]));

        // Empty line between threads
        ir_blocks.push(IrBlock::Empty);
    }

    // Virtual "+ New Thread" entry
    let on_virtual = selected >= ts.threads.len();
    let new_sem = if on_virtual {
        Semantic::Accent
    } else {
        Semantic::Muted
    };
    let new_indicator = if on_virtual { "▸ " } else { "  " };
    let mut new_spans = vec![
        S::styled(new_indicator.to_owned(), new_sem),
        S::styled("+ New Thread".to_owned(), new_sem),
    ];
    if on_virtual {
        for span in &mut new_spans {
            span.reversed = true;
        }
    }
    ir_blocks.push(IrBlock::Line(new_spans));

    // Pad to push help hints to the bottom
    let content_lines = ir_blocks.len();
    let help_y = inner.height.saturating_sub(1).to_usize();
    if help_y > content_lines {
        for _ in 0..help_y.saturating_sub(content_lines) {
            ir_blocks.push(IrBlock::Empty);
        }
    }

    // Help / confirmation hint
    if confirming {
        ir_blocks.push(IrBlock::Line(vec![
            S::warning(" Archive? ".to_owned()),
            S::styled("y".to_owned(), Semantic::KeyHint),
            S::muted("/any to cancel".to_owned()),
        ]));
    } else {
        ir_blocks.push(IrBlock::Line(vec![
            S::styled(" Ctrl+A".to_owned(), Semantic::KeyHint),
            S::muted(" del  ".to_owned()),
            S::styled("Ctrl+V".to_owned(), Semantic::KeyHint),
            S::muted(" back".to_owned()),
        ]));
    }

    // Convert IR → ratatui and render
    let lines = ir::blocks_to_lines(&ir_blocks);
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
        .title(ratatui::text::Span::styled(
            " New Thread ",
            ir::semantic_to_style(Semantic::Accent),
        ))
        .style(Style::default().bg(theme::bg_base()));

    let inner = border.inner(area);
    frame.render_widget(border, area);

    let input_preview = if state.input.is_empty() {
        "…".to_owned()
    } else {
        state.input.clone()
    };

    let ir_blocks = vec![
        IrBlock::Empty,
        IrBlock::Line(vec![S::muted(
            "Type a name for the new thread below,".to_owned(),
        )]),
        IrBlock::Line(vec![S::muted(
            "then press Enter to create it.".to_owned(),
        )]),
        IrBlock::Empty,
        IrBlock::Line(vec![
            S::accent("  ➜ ".to_owned()),
            S::new(input_preview),
        ]),
    ];

    let lines = ir::blocks_to_lines(&ir_blocks);
    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

/// Render the right-pane message area with input box for the selected thread.
///
/// Messages and input render through the IR pipeline (same `render_message_blocks`
/// and `render_input_blocks` as the main conversation). Border title uses
/// `semantic_to_style` for color mapping.
fn render_message_area_with_input(
    frame: &mut Frame<'_>,
    state: &State,
    selected: usize,
    area: Rect,
) {
    let ts = ThreadsState::get(state);
    let Some(thread) = ts.threads.get(selected) else {
        return;
    };

    // Title: thread name + status — colors via semantic mapping
    let focus = FocusState::get(state);
    let is_focused = focus.focused_thread_id.as_deref() == Some(thread.id.as_str());
    let (status_label, status_sem) = if is_focused {
        (" [FOCUSED]", Semantic::Accent)
    } else {
        match thread.status {
            ThreadStatus::MyTurn => (" [MY_TURN]", Semantic::Warning),
            ThreadStatus::TheirTurn => (" [THEIR_TURN]", Semantic::Success),
        }
    };

    let title = ratatui::text::Line::from(vec![
        ratatui::text::Span::styled(
            format!(" {} ", thread.name),
            ir::semantic_to_style(Semantic::Default),
        ),
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

    // Calculate input area height based on input content
    let input_height = calculate_input_height(state, inner.width);
    let messages_height = inner.height.saturating_sub(input_height);

    if messages_height == 0 {
        return;
    }

    // Split inner area: messages on top, input at bottom
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(messages_height),
            Constraint::Length(input_height),
        ])
        .split(inner);

    let (Some(&msg_area), Some(&input_area)) = (layout.first(), layout.get(1)) else {
        return;
    };

    render_thread_messages(frame, state, thread, msg_area);
    render_thread_input(frame, state, input_area);
}

/// Render thread messages using the conversation IR renderer.
///
/// Converts `ThreadMessage` → `Message`, feeds to `render_message_blocks()`
/// (same IR path as the main conversation), converts via `blocks_to_lines()`.
fn render_thread_messages(
    frame: &mut Frame<'_>,
    state: &State,
    thread: &cp_mod_threads::types::Thread,
    area: Rect,
) {
    if thread.messages.is_empty() {
        let ir_blocks = vec![IrBlock::Line(vec![S::muted(
            "No messages yet. Type below to start the conversation.".to_owned(),
        )])];
        let lines = ir::blocks_to_lines(&ir_blocks);
        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, area);
        return;
    }

    let opts = MessageBlockOpts {
        viewport_width: area.width,
        is_streaming: false,
        dev_mode: false,
    };

    // Convert ThreadMessages → Messages → IR blocks → ratatui Lines
    let mut all_blocks: Vec<cp_render::Block> = Vec::new();
    for msg in &thread.messages {
        let conv_msg = thread_message_to_message(msg);
        let msg_blocks = render_message_blocks(&conv_msg, &opts);
        all_blocks.extend(msg_blocks);
    }

    let lines = ir::blocks_to_lines(&all_blocks);

    // Scroll: use global scroll_offset; pin to bottom when user hasn't scrolled
    let content_height = lines.len();
    let viewport_height = area.height.to_usize();
    let max_scroll = content_height.saturating_sub(viewport_height);
    let scroll_offset = if state.flags.stream.user_scrolled {
        // User manually scrolled — respect their position, clamped
        (state.scroll_offset.to_usize()).min(max_scroll)
    } else {
        // Auto-scroll to bottom
        max_scroll
    };

    let paragraph = Paragraph::new(lines).scroll((scroll_offset.to_u16(), 0));
    frame.render_widget(paragraph, area);

    // Scrollbar — colors via semantic mapping
    if content_height > viewport_height {
        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .style(ir::semantic_to_style(Semantic::Border))
            .thumb_style(ir::semantic_to_style(Semantic::AccentDim));
        let mut scrollbar_state = ScrollbarState::new(max_scroll).position(scroll_offset);
        frame.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
    }
}

/// Render the input area at the bottom of the thread message area.
///
/// Separator line and input content both go through the IR pipeline.
fn render_thread_input(frame: &mut Frame<'_>, state: &State, area: Rect) {
    // Separator line via IR (border-colored, dimmed)
    let sep_area = Rect { height: 1, ..area };
    let sep_blocks = vec![IrBlock::Line(vec![S::styled(
        "─".repeat(area.width.into()),
        Semantic::Border,
    )
    .dim()])];
    let sep_lines = ir::blocks_to_lines(&sep_blocks);
    let sep = Paragraph::new(sep_lines);
    frame.render_widget(sep, sep_area);

    // Input content below separator — via IR pipeline
    let input_area = Rect {
        y: area.y.saturating_add(1),
        height: area.height.saturating_sub(1),
        ..area
    };

    let command_ids: Vec<String> =
        cp_mod_prompt::storage::load_prompts_for(cp_mod_prompt::types::PromptType::Command)
            .iter()
            .map(|p| p.id.clone())
            .collect();

    let ctx = InputBlockCtx {
        command_ids: &command_ids,
        paste_buffers: &state.paste_buffers,
        paste_buffer_labels: &state.paste_buffer_labels,
        viewport_width: input_area.width,
    };

    let input_blocks = render_input_blocks(
        &state.input,
        state.input_cursor,
        state.input_selection_anchor,
        &ctx,
    );

    let lines = ir::blocks_to_lines(&input_blocks);
    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, input_area);
}

/// Convert a `ThreadMessage` to a `Message` for the conversation IR renderer.
fn thread_message_to_message(msg: &cp_mod_threads::types::ThreadMessage) -> Message {
    let role = match msg.author {
        ThreadAuthor::User => "user",
        ThreadAuthor::Assistant => "assistant",
    };
    Message {
        id: String::new(),
        uid: None,
        role: role.to_owned(),
        content: msg.content.clone().unwrap_or_default(),
        msg_type: MsgKind::TextMessage,
        status: MsgStatus::Full,
        tool_uses: vec![],
        tool_results: vec![],
        input_tokens: 0,
        content_token_count: 0,
        timestamp_ms: msg.timestamp,
    }
}

/// Calculate input area height based on current input content.
fn calculate_input_height(state: &State, width: u16) -> u16 {
    if state.input.is_empty() {
        // Separator (1) + one line for empty input prompt
        return 3;
    }
    let line_count = state.input.lines().count().max(1);
    // Account for wrapping
    let wrap_width = (width as usize).saturating_sub(10).max(20);
    let wrapped_lines: usize = state
        .input
        .lines()
        .map(|l| {
            if l.is_empty() {
                1
            } else {
                l.len().div_ceil(wrap_width).max(1)
            }
        })
        .sum();
    let total = wrapped_lines.max(line_count);
    // Separator (1) + content + hint line (1), capped at 10 lines
    (total.saturating_add(3)).min(10).to_u16()
}

/// Truncate a string to `max_len` characters, appending "…" if truncated.
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let mut result: String = s.chars().take(max_len.saturating_sub(1)).collect();
        result.push('…');
        result
    }
}
