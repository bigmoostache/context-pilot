//! Threads view — dedicated TUI layout when `ViewMode::Threads` is active.
//!
//! Renders a two-pane layout: thread list (left) + message area (right).
//! Messages render identically to the main conversation panel (user/AI only).
//! Completely replaces the standard sidebar + panel view.

use ratatui::Frame;
use ratatui::prelude::{Constraint, Direction, Layout, Rect, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};

use crate::modules::conversation::render_blocks::{MessageBlockOpts, render_message_blocks};
use crate::modules::conversation::render_input_blocks::{InputBlockCtx, render_input_blocks};
use crate::state::{Message, MsgKind, MsgStatus, State};
use crate::ui::theme;
use cp_base::cast::Safe as _;
use cp_mod_threads::types::{FocusState, ThreadAuthor, ThreadStatus, ThreadsState};

/// Width of the thread list pane in columns.
const THREAD_LIST_WIDTH: u16 = 28;

/// Render the threads view: thread list + message area.
pub(crate) fn render_threads_view(frame: &mut Frame<'_>, state: &State, area: Rect) {
    let threads_state = ThreadsState::get(state);
    let focus_state = FocusState::get(state);

    // Clamp selected index — allow one past end for virtual "New Thread" entry
    let total_entries = threads_state.threads.len().saturating_add(1);
    let selected_idx = focus_state.selected_thread_idx.min(total_entries.saturating_sub(1));
    let on_virtual_new = selected_idx >= threads_state.threads.len();

    // Two-pane layout: thread list | message area
    if area.width > THREAD_LIST_WIDTH.saturating_add(20) {
        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(THREAD_LIST_WIDTH), Constraint::Min(1)])
            .split(area);

        let (Some(&list_area), Some(&msg_area)) = (layout.first(), layout.get(1)) else { return };
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

/// Render the left-pane thread list with selection indicator and virtual "New Thread" entry.
fn render_thread_list(frame: &mut Frame<'_>, state: &State, area: Rect) {
    let ts = ThreadsState::get(state);
    let focus = FocusState::get(state);
    let total_entries = ts.threads.len().saturating_add(1); // +1 for virtual entry
    let selected = focus.selected_thread_idx.min(total_entries.saturating_sub(1));
    let confirming = focus.confirming_archive;
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(theme::border_muted()))
        .style(Style::default().bg(theme::bg_base()));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Build lines for each thread
    let mut lines: Vec<Line<'_>> = Vec::new();

    for (i, thread) in ts.threads.iter().enumerate() {
        let is_selected = i == selected;

        // Unread detection: messages exist beyond what user last saw
        let last_read = focus.last_read_count.get(&thread.id).copied().unwrap_or(0);
        let has_unread = thread.messages.len() > last_read;

        // Indicator + thread name
        let indicator = if is_selected {
            "▸ "
        } else if has_unread {
            "● "
        } else {
            "  "
        };
        let indicator_color = if has_unread && !is_selected { theme::warning() } else { theme::accent() };
        let name_color = if is_selected { theme::accent() } else { theme::text() };
        let name = truncate_str(&thread.name, inner.width.saturating_sub(4).into());

        lines.push(Line::from(vec![
            Span::styled(indicator, Style::default().fg(indicator_color)),
            Span::styled(name, Style::default().fg(name_color)),
        ]));

        // Status badge
        let (badge, badge_color) = match thread.status {
            ThreadStatus::MyTurn => ("[MY_TURN]", theme::accent()),
            ThreadStatus::TheirTurn => ("[THEIR_TURN]", theme::text_muted()),
        };
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(badge, Style::default().fg(badge_color)),
            Span::styled(
                format!("  {} msg", thread.messages.len()),
                Style::default().fg(theme::text_muted()),
            ),
        ]));

        // Separator between threads
        lines.push(Line::from(""));
    }

    // Virtual "New Thread" entry — always at the bottom of the list
    let on_virtual = selected >= ts.threads.len();
    let new_indicator = if on_virtual { "▸ " } else { "  " };
    let new_color = if on_virtual { theme::accent() } else { theme::text_muted() };
    lines.push(Line::from(vec![
        Span::styled(new_indicator, Style::default().fg(new_color)),
        Span::styled("+ New Thread", Style::default().fg(new_color)),
    ]));

    // Help hints at the bottom
    let help_y = inner.height.saturating_sub(1);
    if help_y > 0 && inner.height > lines.len().to_u16() {
        // Pad to push help to bottom
        let needed = (help_y as usize).saturating_sub(lines.len());
        for _ in 0..needed {
            lines.push(Line::from(""));
        }
        if confirming {
            lines.push(Line::from(vec![
                Span::styled(" Archive? ", Style::default().fg(theme::warning())),
                Span::styled("y", Style::default().fg(theme::accent())),
                Span::styled("/any to cancel", Style::default().fg(theme::text_muted())),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled(" Ctrl+A", Style::default().fg(theme::accent())),
                Span::styled(" del  ", Style::default().fg(theme::text_muted())),
                Span::styled("Ctrl+V", Style::default().fg(theme::accent())),
                Span::styled(" back", Style::default().fg(theme::text_muted())),
            ]));
        }
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

/// Render the right pane when the virtual "New Thread" entry is selected.
///
/// Shows a prompt inviting the user to type a thread name in the input area.
fn render_new_thread_prompt(frame: &mut Frame<'_>, state: &State, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::border_muted()))
        .title(Span::styled(" New Thread ", Style::default().fg(theme::accent())))
        .style(Style::default().bg(theme::bg_base()));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let input_preview = if state.input.is_empty() {
        "…".to_string()
    } else {
        state.input.clone()
    };

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "Type a name for the new thread below,",
            Style::default().fg(theme::text_muted()),
        )),
        Line::from(Span::styled(
            "then press Enter to create it.",
            Style::default().fg(theme::text_muted()),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  ➜ ", Style::default().fg(theme::accent())),
            Span::styled(input_preview, Style::default().fg(theme::text())),
        ]),
    ];

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

/// Render the right-pane message area with input box for the selected thread.
///
/// Messages render identically to the main conversation panel: same icons,
/// markdown rendering, and styling. Only user and assistant messages appear
/// (threads never contain tool calls or tool results).
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

    // Title: thread name + status
    let (status_label, status_color) = match thread.status {
        ThreadStatus::MyTurn => (" [MY_TURN]", theme::accent()),
        ThreadStatus::TheirTurn => (" [THEIR_TURN]", theme::text_muted()),
    };

    let title = Line::from(vec![
        Span::styled(format!(" {} ", thread.name), Style::default().fg(theme::text())),
        Span::styled(status_label, Style::default().fg(status_color)),
        Span::raw(" "),
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(theme::border()))
        .title(title)
        .style(Style::default().bg(theme::bg_surface()));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Calculate input area height based on input content
    let input_height = calculate_input_height(state, inner.width);
    let messages_height = inner.height.saturating_sub(input_height);

    if messages_height == 0 {
        return;
    }

    // Split inner area: messages on top, input at bottom
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(messages_height), Constraint::Length(input_height)])
        .split(inner);

    let (Some(&msg_area), Some(&input_area)) = (layout.first(), layout.get(1)) else { return };

    render_thread_messages(frame, thread, msg_area);
    render_thread_input(frame, state, input_area);
}

/// Render thread messages using the same IR renderer as the main conversation.
fn render_thread_messages(
    frame: &mut Frame<'_>,
    thread: &cp_mod_threads::types::Thread,
    area: Rect,
) {
    if thread.messages.is_empty() {
        let empty = Paragraph::new(Span::styled(
            "No messages yet. Type below to start the conversation.",
            Style::default().fg(theme::text_muted()),
        ));
        frame.render_widget(empty, area);
        return;
    }

    let opts = MessageBlockOpts {
        viewport_width: area.width,
        is_streaming: false,
        dev_mode: false,
    };

    // Convert ThreadMessages to Message structs and render via conversation IR
    let mut all_blocks: Vec<cp_render::Block> = Vec::new();
    for msg in &thread.messages {
        let conv_msg = thread_message_to_message(msg);
        let msg_blocks = render_message_blocks(&conv_msg, &opts);
        all_blocks.extend(msg_blocks);
    }

    let lines = crate::ui::ir::blocks_to_lines(&all_blocks);

    // Scroll: auto-scroll to bottom
    let content_height = lines.len();
    let viewport_height = area.height.to_usize();
    let max_scroll = content_height.saturating_sub(viewport_height);
    let scroll_offset = max_scroll; // Always at bottom for now

    let paragraph = Paragraph::new(lines).scroll((scroll_offset.to_u16(), 0));
    frame.render_widget(paragraph, area);

    // Scrollbar
    if content_height > viewport_height {
        let scrollbar = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .style(Style::default().fg(theme::bg_elevated()))
            .thumb_style(Style::default().fg(theme::accent_dim()));
        let mut scrollbar_state = ScrollbarState::new(max_scroll).position(scroll_offset);
        frame.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
    }
}

/// Render the input area at the bottom of the thread message area.
fn render_thread_input(frame: &mut Frame<'_>, state: &State, area: Rect) {
    // Separator line at the top of the input area
    let sep_area = Rect { height: 1, ..area };
    let sep = Paragraph::new(Line::from(Span::styled(
        "─".repeat(area.width.into()),
        Style::default().fg(theme::border_muted()),
    )));
    frame.render_widget(sep, sep_area);

    // Input content below separator
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

    let lines = crate::ui::ir::blocks_to_lines(&input_blocks);
    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, input_area);
}

/// Convert a `ThreadMessage` to a `Message` for rendering via the conversation IR.
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
