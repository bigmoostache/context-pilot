//! Agents view — dedicated TUI layout when `ViewMode::Agents` is active.
//!
//! Renders a two-pane layout: agent list (left) + message area (right).
//! All content goes through the IR pipeline (`cp_render::Block` / `Span` →
//! `blocks_to_lines()` → ratatui). Only layout chrome (borders, scrollbars,
//! area splits) uses ratatui directly — same pattern as the sidebar adapter.
//!
//! Sub-module [`messages`] handles the right-pane message area, input, and
//! question form rendering.

/// Message area rendering: messages, input, question form.
pub(super) mod messages;
/// Name-entry prompts (new agent, rename).
mod prompt;
/// Virtualized agent-list entries.
mod visible;
/// Human-readable reason behind an agent row's ⚠ (web red square + banner).
mod warning;
use prompt::{NEW_AGENT_PROMPT, RENAME_PROMPT, render_title_prompt};
use visible::push_visible_entries;
pub(crate) use warning::agent_warning;

use ratatui::Frame;
use ratatui::prelude::{Constraint, Direction, Layout, Rect, Style};
use ratatui::widgets::{Block as RBlock, Paragraph};

use cp_render::{Block as IrBlock, Semantic, Span as S};

use crate::state::State;
use crate::ui::{ir, theme};
use cp_base::cast::Safe as _;
use cp_fleet::AgentExecState;
use cp_mod_agents::types::{AgentAuthor, AgentsState, FocusState};
use cp_mod_agents::view_state::FleetExecMirror;

/// Width of the agent list pane in columns.
///
/// Same as the panel-centric view's sidebar so the two layouts line up.
pub(crate) const AGENT_LIST_WIDTH: u16 = cp_base::config::constants::SIDEBAR_WIDTH;

/// Render the agents view: agent list + message area.
pub(crate) fn render_agents_view(frame: &mut Frame<'_>, state: &mut State, area: Rect) {
    let agents_state = AgentsState::get(state);
    let focus_state = FocusState::get(state);
    let viewing_archived = focus_state.viewing_archived;

    // The visible list is the subset matching the current view. The active
    // view has a trailing virtual "+ New Agent" entry; the archived view
    // does not.
    let visible = agents_state.visible_indices(viewing_archived);
    let show_new = !viewing_archived;
    let total_entries = visible.len().saturating_add(usize::from(show_new));
    let selected_idx = focus_state.selected_agent_idx.min(total_entries.saturating_sub(1));
    let on_virtual_new = show_new && selected_idx >= visible.len();

    // Two-pane layout: agent list | message area
    if area.width > AGENT_LIST_WIDTH.saturating_add(20) {
        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(AGENT_LIST_WIDTH), Constraint::Min(1)])
            .split(area);

        let (Some(&list_area), Some(&msg_area)) = (layout.first(), layout.get(1)) else {
            return;
        };
        {
            let _g = crate::profile!("tv_list");
            render_agent_list(frame, state, &visible, list_area);
        }
        let focus = FocusState::get(state);
        if focus.renaming_agent_id.is_some() {
            render_title_prompt(frame, &RENAME_PROMPT, &focus.rename_title.text, msg_area);
        } else if on_virtual_new {
            render_title_prompt(frame, &NEW_AGENT_PROMPT, &focus.new_agent_title.text, msg_area);
        } else if let Some(&real_idx) = visible.get(selected_idx) {
            messages::render_message_area_with_input(frame, state, real_idx, msg_area);
        } else {
            // No agent selected — leave the message area blank.
        }
    } else {
        // Narrow terminal — show agent list only
        render_agent_list(frame, state, &visible, area);
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
            S::styled("\u{2317} ARCHIVED".to_owned(), Semantic::AccentDim),
        ]));
        blocks.push(IrBlock::Empty);
    }
}

/// Per-frame row facts for [`push_visible_entries`].
struct Rows {
    /// Selected position in the visible slice.
    selected: usize,
    /// Archive confirm armed (bubble under the selected entry).
    confirming: bool,
    /// Archived view is shown.
    viewing_archived: bool,
    /// Pane height: the pane has no scroll offset and clips here.
    height: usize,
}

/// Mutable accumulator threaded through the thread-list block builders:
/// the growing IR block vec, the selected-entry range, and the pane width.
struct ListBuild<'build> {
    /// Growing IR block list for the agent list.
    blocks: &'build mut Vec<IrBlock>,
    /// Selected-entry line range, filled as entries are pushed.
    sel: &'build mut SelRange,
    /// Inner pane width (columns) for name truncation.
    inner_width: u16,
    /// Display-only per-agent exec state, rebuilt each tick by the run loop.
    /// Drives the leading working-spinner (reuses the fleet mirror; never read
    /// for scheduling).
    mirror: &'build FleetExecMirror,
    /// Per-thread message count the *human* has seen in the TUI
    /// ([`FocusState::last_read_count`]) — drives the unread marker.
    last_read: &'build std::collections::BTreeMap<String, usize>,
    /// Ids of `MyTurn` agents, built once per frame
    /// ([`cp_groups::social::index::runnable_set`]).
    runnable: std::collections::HashSet<&'build str>,
    /// Group id → its messages, built once per frame (unread marker lookup).
    msgs_of_group: std::collections::HashMap<&'build str, &'build [cp_mod_agents::types::AgentMessage]>,
    /// Tree-guide prefix of each visible agent (`├─`, `│ `…, empty for roots).
    guide_of: std::collections::HashMap<String, String>,
}

/// Agent id → tree-guide prefix, from the preorder walk of the visible agents.
fn guide_map(ts: &AgentsState, visible: &[usize]) -> std::collections::HashMap<String, String> {
    use cp_mod_agents::types::tree::{guides, preorder};
    let walk = preorder(&ts.agents, visible);
    let depths: Vec<usize> = walk.iter().map(|&(_, d)| d).collect();
    walk.iter().zip(guides(&depths)).filter_map(|(&(i, _), g)| Some((ts.agents.get(i)?.id.clone(), g))).collect()
}

/// Push the virtual "+ New Agent" entry (active view only, single-line),
/// recording its line range into `sel` when it is the selected entry.
fn push_new_agent_entry(lb: &mut ListBuild<'_>, state: &State, on_virtual: bool) {
    let new_sem = if on_virtual { Semantic::Accent } else { Semantic::Muted };
    if on_virtual {
        lb.sel.start = Some(lb.blocks.len());
    }
    let title = &FocusState::get(state).new_agent_title.text;
    let new_name = if title.is_empty() {
        "New Agent".to_owned()
    } else {
        truncate_str(title, lb.inner_width.saturating_sub(6).into())
    };
    lb.blocks.push(IrBlock::Line(vec![
        S::styled("  ".to_owned(), new_sem),
        S::styled("\u{25cf} ".to_owned(), new_sem),
        S::styled(new_name, new_sem),
    ]));
    if on_virtual {
        lb.sel.end = Some(lb.blocks.len());
    }
}

/// Raw RGB for one agent's status circle — a function of agent *state* only,
/// never of which agent the human happens to be viewing:
/// orange = `MyTurn` (the LLM owes a turn), green = `TheirTurn` (the human owes
/// a turn), yellow/amber = paused. Returned as a raw triple (via
/// [`Span::rgb`](cp_render::Span::rgb)) because the IR `Semantic` palette has no
/// distinct orange-vs-amber pair.
fn agent_circle_rgb(agent: &cp_mod_agents::types::Agent, my_turn: bool) -> (u8, u8, u8) {
    let color = if agent.paused {
        theme::warning()
    } else if my_turn {
        theme::orange()
    } else {
        theme::success()
    };
    if let ratatui::style::Color::Rgb(r, g, b) = color { (r, g, b) } else { (200, 200, 200) }
}

/// Whether an agent's exec state should show the "working" spinner.
///
/// Only [`Streaming`](AgentExecState::Streaming) counts: a stream, its tools or
/// a blocking wait is actually in flight. [`Runnable`](AgentExecState::Runnable)
/// is deliberately NOT working — it means `MyTurn` + not streaming, i.e. the LLM
/// owes a turn but nothing is running right now. That is a stall the human
/// should notice (⚠ in [`leading_slot`]), not a spinner. A refusal/abnormal stop
/// leaves the agent exactly here (still `MyTurn`, parked), so narrowing the
/// spinner to `Streaming` is what surfaces the ⚠ on refusal-stopped agents.
/// `Idle` and `Errored` are likewise void of the spinner.
const fn agent_is_working(exec: AgentExecState) -> bool {
    matches!(exec, AgentExecState::Streaming)
}

/// The web sidebar light: the row shows the working spinner OR an orange
/// circle (LLM's turn, not paused). Agents waiting on the human stay unlit.
/// Shared with the bridge emitter so the web square always matches this list.
pub(crate) fn agent_lit(state: &State, agent: &cp_mod_agents::types::Agent) -> bool {
    let working = agent_is_working(FleetExecMirror::get(state).exec_state_of(&agent.id));
    let orange = !agent.paused
        && matches!(cp_mod_agents::types::turn::status(state, &agent.id), cp_mod_agents::types::AgentStatus::MyTurn);
    working || orange
}

/// Whether `agent` holds an LLM reply the human has not yet seen in the TUI:
/// a non-auto assistant message at or past the agent's
/// [`last_read_count`](FocusState::last_read_count). An agent with no entry
/// was never tracked and counts as read, so old agents don't all light up.
fn has_unread_reply(
    history: &[cp_mod_agents::types::AgentMessage],
    agent: &cp_mod_agents::types::Agent,
    last_read: &std::collections::BTreeMap<String, usize>,
) -> bool {
    let Some(&seen) = last_read.get(&agent.id) else {
        return false;
    };
    history.get(seen..).is_some_and(|tail| tail.iter().any(|m| !m.auto && matches!(m.author, AgentAuthor::Assistant)))
}

/// Leading 2-col slot of an agent row, mirroring the footer:
/// working → the footer's square spinner; idle AND `MyTurn` → ⚠ (the LLM owes
/// a turn but is doing nothing, a stall the human should notice); unread LLM
/// reply → ◆ (cleared once the row has stayed selected 2s, see
/// `FocusState::tick_read_dwell`); otherwise void.
fn leading_slot(lb: &ListBuild<'_>, agent: &cp_mod_agents::types::Agent) -> S {
    if agent_is_working(lb.mirror.exec_state_of(&agent.id)) {
        S::styled(format!("{} ", crate::ui::helpers::spinner()), Semantic::Accent)
    } else if lb.runnable.contains(agent.id.as_str()) {
        S::styled("\u{26a0} ".to_owned(), Semantic::Error)
    } else if has_unread_reply(
        lb.msgs_of_group.get(agent.human_group_id.as_str()).copied().unwrap_or_default(),
        agent,
        lb.last_read,
    ) {
        S::styled("\u{25c6} ".to_owned(), Semantic::Accent)
    } else {
        S::styled("  ".to_owned(), Semantic::Default)
    }
}

/// Worktree folder name of an agent, plus its branch's open PR (if any).
struct WorktreeLine {
    /// Worktree folder name.
    name: String,
    /// Open PR badge (non-blocking cached lookup, see `cp_mod_github::pr_badge`).
    badge: Option<cp_mod_github::pr_badge::Badge>,
}

/// The agent's [`WorktreeLine`]; `None` at the project root. The PR badge is
/// dropped when the parent agent works in the same worktree: the parent's
/// row already shows it.
fn worktree_line(state: &State, agent: &cp_mod_agents::types::Agent) -> Option<WorktreeLine> {
    let dir = state.agent_store.get(&agent.id)?.workdir.as_deref()?;
    let name = dir.file_name()?.to_string_lossy().into_owned();
    let shares_parent = agent
        .parent_agent_id
        .as_deref()
        .and_then(|p| state.agent_store.get(p)?.workdir.as_deref())
        .is_some_and(|parent_dir| parent_dir == dir);
    let token = cp_mod_github::types::GithubState::get(state).github_token.as_deref();
    let badge = if shares_parent { None } else { token.and_then(|t| cp_mod_github::pr_badge::badge(dir, t)) };
    Some(WorktreeLine { name, badge })
}

/// Spans of the muted `#123 ● ⎇ <worktree>` line. The PR comes first so a
/// long worktree name truncates without hiding it; the `●` is the same dot as
/// the agent rows, coloured by state (blue conflicts, red CI failing, yellow
/// CI running, green).
fn worktree_spans(wt: &WorktreeLine, width: u16) -> Vec<S> {
    use cp_mod_github::pr_badge::CheckColor;
    let mut spans = vec![S::muted("    ".to_owned())];
    let mut used: usize = 4;
    if let Some(b) = wt.badge {
        let color = match b.color {
            CheckColor::Green => theme::success(),
            CheckColor::Yellow => theme::warning(),
            CheckColor::Red => theme::error(),
            CheckColor::Blue => ratatui::style::Color::Rgb(66, 135, 245),
        };
        let (r, g, bl) = if let ratatui::style::Color::Rgb(r, g, bl) = color { (r, g, bl) } else { (200, 200, 200) };
        let label = format!("{} ", b.label());
        used = used.saturating_add(label.chars().count()).saturating_add(2);
        spans.push(S::muted(label));
        spans.push(S::rgb("\u{25cf}".to_owned(), r, g, bl));
        spans.push(S::muted(" ".to_owned()));
    }
    let room = usize::from(width).saturating_sub(used.saturating_add(3));
    spans.push(S::muted(format!("\u{2387} {}", truncate_str(&wt.name, room))));
    spans
}

/// Push one agent's entry (status-colored circle + name), plus a muted
/// `⎇ <worktree>` line below when the agent works in a git worktree,
/// recording its selected line range.
fn push_agent_entry(
    lb: &mut ListBuild<'_>,
    agent: &cp_mod_agents::types::Agent,
    is_selected: bool,
    worktree: Option<&WorktreeLine>,
) {
    let leading = leading_slot(lb, agent);
    let (cr, cg, cb) = agent_circle_rgb(agent, lb.runnable.contains(agent.id.as_str()));
    // Sub-agents: dimmed tree guides, two columns per depth level (tree order
    // comes from `visible_indices`).
    let guide = lb.guide_of.get(&agent.id).cloned().unwrap_or_default();
    let room = usize::from(lb.inner_width.saturating_sub(6)).saturating_sub(guide.chars().count());
    let name = truncate_str(&agent.name, room);

    if is_selected {
        lb.sel.start = Some(lb.blocks.len());
    }
    lb.blocks.push(IrBlock::Line(vec![
        leading,
        S::styled(guide, Semantic::Muted),
        S::rgb("\u{25cf} ".to_owned(), cr, cg, cb),
        S::new(name),
    ]));
    if let Some(wt) = worktree {
        lb.blocks.push(IrBlock::Line(worktree_spans(wt, lb.inner_width)));
    }
    if is_selected {
        lb.sel.end = Some(lb.blocks.len());
    }
}

/// Push the inline archive/restore confirm bubble, rendered on its own line
/// directly below the selected agent's row.
///
/// Leads with a down-right arrow (`\u{21B3}`) hanging off the agent it
/// concerns, and is fully red ([`Semantic::Error`]) so the pending confirm
/// reads as a bubble attached to that specific agent — not the easy-to-miss
/// footer hint it replaces.
fn push_archive_confirm_bubble(blocks: &mut Vec<IrBlock>, viewing_archived: bool) {
    let verb = if viewing_archived { "restore" } else { "archive" };
    blocks.push(IrBlock::Line(vec![
        S::styled("    \u{21B3} ".to_owned(), Semantic::Error),
        S::styled("Ctrl+X".to_owned(), Semantic::Error),
        S::styled(format!(" again to confirm {verb}"), Semantic::Error),
    ]));
}

/// Push the bottom help hint line for the current mode.
fn push_help_hint(blocks: &mut Vec<IrBlock>, viewing_archived: bool) {
    if viewing_archived {
        blocks.push(IrBlock::Line(vec![
            S::styled("Up/Dn".to_owned(), Semantic::KeyHint),
            S::muted(" select  ".to_owned()),
            S::styled("\u{2192}".to_owned(), Semantic::KeyHint),
            S::muted(" view  ".to_owned()),
            S::styled("Ctrl+X".to_owned(), Semantic::KeyHint),
            S::muted(" restore  ".to_owned()),
            S::styled("Ctrl+R".to_owned(), Semantic::KeyHint),
            S::muted(" ren  ".to_owned()),
            S::styled("Ctrl+U".to_owned(), Semantic::KeyHint),
            S::muted(" active  ".to_owned()),
            S::styled("Esc".to_owned(), Semantic::KeyHint),
            S::muted(" back".to_owned()),
        ]));
    } else {
        blocks.push(IrBlock::Line(vec![
            S::styled("Up/Dn".to_owned(), Semantic::KeyHint),
            S::muted(" select  ".to_owned()),
            S::styled("\u{2192}".to_owned(), Semantic::KeyHint),
            S::muted(" view  ".to_owned()),
            S::styled("Ctrl+X".to_owned(), Semantic::KeyHint),
            S::muted(" arch  ".to_owned()),
            S::styled("Ctrl+R".to_owned(), Semantic::KeyHint),
            S::muted(" ren  ".to_owned()),
            S::styled("Ctrl+U".to_owned(), Semantic::KeyHint),
            S::muted(" arch'd  ".to_owned()),
            S::styled("Esc".to_owned(), Semantic::KeyHint),
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

/// Render the left-pane agent list via IR blocks.
///
/// Layout chrome (right border) is direct ratatui. All visible content
/// (agent entries, indicators, help hints) goes through IR → `blocks_to_lines()`.
fn render_agent_list(frame: &mut Frame<'_>, state: &State, visible: &[usize], area: Rect) {
    let ts = AgentsState::get(state);
    let focus = FocusState::get(state);
    let mirror = FleetExecMirror::get(state);
    let viewing_archived = focus.viewing_archived;
    let groups = &cp_groups::types::GroupsState::get(state).groups;
    let show_new = !viewing_archived; // virtual "+ New Agent" only in the active view
    let total_entries = visible.len().saturating_add(usize::from(show_new));
    let selected = focus.selected_agent_idx.min(total_entries.saturating_sub(1));
    // The arm is only "live" within the 2-second confirm window; a lapsed arm
    // reverts the hint to the normal state (a next Ctrl+X will re-arm).
    let confirming =
        focus.confirming_archive && cp_base::panels::now_ms().saturating_sub(focus.archive_armed_at_ms) <= 2_000;

    // No border chrome: the pane bleeds into the message area with no divider
    // line (matches the panel-centric sidebar, which has no right border).
    let background = RBlock::default().style(Style::default().bg(theme::bg_base()));
    let inner = background.inner(area);
    frame.render_widget(background, area);

    // ── Build IR blocks ──────────────────────────────────────────────
    let mut ir_blocks: Vec<IrBlock> = Vec::new();
    let mut sel = SelRange::default();

    push_archived_header(&mut ir_blocks, viewing_archived);

    let on_virtual = show_new && selected >= visible.len();
    let index_guard = crate::profile!("tv_list_index");
    let mut lb = ListBuild {
        blocks: &mut ir_blocks,
        sel: &mut sel,
        inner_width: inner.width,
        mirror,
        last_read: &focus.last_read_count,
        runnable: cp_groups::social::index::runnable_set(groups),
        msgs_of_group: groups.iter().map(|g| (g.id.as_str(), g.messages.as_slice())).collect(),
        guide_of: guide_map(ts, visible),
    };
    drop(index_guard);
    if show_new {
        push_new_agent_entry(&mut lb, state, on_virtual);
    }
    let rows = Rows { selected, confirming, viewing_archived, height: inner.height.to_usize() };
    push_visible_entries(&mut lb, state, visible, &rows);

    // Empty-state hint when the archived list has nothing in it.
    if viewing_archived && visible.is_empty() {
        ir_blocks.push(IrBlock::Line(vec![S::muted("  (no archived agents)".to_owned())]));
    }

    // Pad to push help hints to the bottom
    let content_lines = ir_blocks.len();
    let help_y = inner.height.saturating_sub(1).to_usize();
    if help_y > content_lines {
        for _ in 0..help_y.saturating_sub(content_lines) {
            ir_blocks.push(IrBlock::Empty);
        }
    }

    push_help_hint(&mut ir_blocks, viewing_archived);

    // Convert IR → ratatui and render
    let mut lines = ir::blocks_to_lines(&ir_blocks);
    apply_selection_highlight(&mut lines, &sel, inner.width);

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

/// Truncate a string to `max_len` characters, appending "…" if truncated.
pub(super) fn truncate_str(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_owned()
    } else {
        let mut result: String = s.chars().take(max_len.saturating_sub(1)).collect();
        result.push('\u{2026}');
        result
    }
}
