use ratatui::{
    prelude::{Constraint, Direction, Frame, Layout, Line, Rect, Span, Style},
    widgets::Paragraph,
};

use super::super::{
    chars,
    helpers::{format_number, spinner, truncate_string},
    theme,
};
use crate::infra::constants::SIDEBAR_HELP_HEIGHT;
use crate::state::{Kind, State};
use cp_base::cast::Safe as _;

/// Returns a count badge for fixed panels, replacing the panel ID (P1, P2, etc.)
/// with a meaningful number that reflects the panel's content.
pub(super) fn fixed_panel_badge(ctx_type: &str, state: &State) -> Option<String> {
    let count = match ctx_type {
        "todo" => {
            let ts = cp_mod_todo::types::TodoState::get(state);
            ts.todos.iter().filter(|t| !matches!(t.status, cp_mod_todo::types::TodoStatus::Done)).count()
        }
        "library" => cp_mod_prompt::types::PromptState::get(state).loaded_skill_ids.len(),
        "tree" => cp_mod_tree::types::TreeState::get(state).open_folders.len(),
        "memory" => cp_mod_memory::types::MemoryState::get(state).memories.len(),
        "spine" => cp_mod_spine::types::SpineState::unprocessed_notifications(state).len(),
        "logs" => {
            let ls = cp_mod_logs::types::LogsState::get(state);
            ls.logs.iter().filter(|l| l.is_top_level()).count()
        }
        "callback" => cp_mod_callback::types::CallbackState::get(state).active_set.len(),
        "scratchpad" => cp_mod_scratchpad::types::ScratchpadState::get(state).scratchpad_cells.len(),
        "queue" => cp_mod_queue::types::QueueState::get(state).queued_calls.len(),
        "overview" => state.context.len().saturating_add(2), // +2 for system prompt + tool definitions
        "tools" => state.tools.iter().filter(|t| t.enabled).count(),
        "chat-dashboard" => cp_mod_chat::types::ChatState::get(state).rooms.len(),
        _ => return None,
    };
    Some(count.to_string())
}

/// Maximum number of dynamic contexts (P7+) to show per page.
const MAX_DYNAMIC_PER_PAGE: usize = 10;

/// Render the full sidebar with context list, token bar, PR card, and help hints.
pub(crate) fn render_sidebar(frame: &mut Frame<'_>, state: &State, area: Rect) {
    let _guard = crate::profile!("ui::sidebar");
    let base_style = Style::default().bg(theme::bg_base());

    // Sidebar layout: context list + help hints
    let sidebar_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),                      // Context list
            Constraint::Length(SIDEBAR_HELP_HEIGHT), // Help hints
        ])
        .split(area);
    debug_assert!(sidebar_layout.len() >= 2, "sidebar layout must have at least 2 chunks");

    // Context list
    let mut lines: Vec<Line<'_>> = vec![
        Line::from(vec![
            Span::styled("  ", base_style),
            Span::styled("CONTEXT", Style::default().fg(theme::text_muted()).bold()),
        ]),
        Line::from(""),
    ];

    // Use shared token calculation (same as Statistics panel)
    let system_prompt_tokens = {
        let sp = cp_mod_prompt::seed::get_active_agent_content(state);
        crate::state::estimate_tokens(&sp).saturating_mul(2)
    };
    let tool_def_tokens = crate::modules::overview::context::estimate_tool_definitions_tokens(state);
    let panel_tokens: usize = state.context.iter().map(|c| c.token_count).sum();
    let total_tokens = system_prompt_tokens.saturating_add(tool_def_tokens).saturating_add(panel_tokens);
    let max_tokens = state.effective_context_budget();
    let threshold_tokens = state.cleaning_threshold_tokens();

    // Compute hit/miss token breakdown for progress bar
    // System prompt and tool definitions always count as "hit" (stable, always cached)
    let mut hit_tokens = system_prompt_tokens.saturating_add(tool_def_tokens);
    let mut miss_tokens = 0usize;
    for ctx in &state.context {
        if ctx.panel_cache_hit {
            hit_tokens = hit_tokens.saturating_add(ctx.token_count);
        } else {
            miss_tokens = miss_tokens.saturating_add(ctx.token_count);
        }
    }

    // Calculate ID width for alignment based on longest ID
    let id_width = state.context.iter().map(|c| c.id.len()).max().unwrap_or(2);

    let spin = spinner(state.spinner_frame);

    // Sort contexts by ID for display (P0, P1, P2, ...)
    let mut sorted_indices: Vec<usize> = (0..state.context.len()).collect();
    sorted_indices.sort_by(|&a, &b| {
        let ctx_a = state.context.get(a);
        let ctx_b = state.context.get(b);
        let id_a = ctx_a
            .and_then(|c| c.id.strip_prefix('P'))
            .and_then(|n: &str| n.parse::<usize>().ok())
            .unwrap_or(usize::MAX);
        let id_b = ctx_b
            .and_then(|c| c.id.strip_prefix('P'))
            .and_then(|n: &str| n.parse::<usize>().ok())
            .unwrap_or(usize::MAX);
        id_a.cmp(&id_b)
    });

    // Separate fixed (P1-P9) and dynamic (P10+) contexts, skipping Conversation (it's the chat feed, not a numbered panel)
    let (fixed_indices, dynamic_indices): (Vec<_>, Vec<_>) = sorted_indices
        .into_iter()
        .filter(|&i| state.context.get(i).is_some_and(|c| c.context_type != Kind::new(Kind::CONVERSATION)))
        .partition(|&i| state.context.get(i).is_some_and(|c| c.context_type.is_fixed()));

    let lctx = SidebarLineCtx { state, id_width, spin, base_style };

    // Render Conversation entry (special: no Px ID, highlights when selected)
    if let Some(conv_idx) = state.context.iter().position(|c| c.context_type == Kind::new(Kind::CONVERSATION)) {
        let is_selected = conv_idx == state.selected_context;
        let indicator = if is_selected { chars::ARROW_RIGHT } else { " " };
        let indicator_color = if is_selected { theme::accent() } else { theme::bg_base() };
        let name_color = if is_selected { theme::accent() } else { theme::text_secondary() };
        let icon = Kind::new(Kind::CONVERSATION).icon();
        let conv_ctx = state.context.get(conv_idx);
        let conv_tokens = format_number(conv_ctx.map_or(0, |c| c.token_count));

        lines.push(Line::from(vec![
            Span::styled(format!(" {indicator}"), Style::default().fg(indicator_color)),
            Span::styled(format!(" {:>width$} ", "", width = id_width), Style::default().fg(theme::text_muted())),
            Span::styled(icon, Style::default().fg(if is_selected { theme::accent() } else { theme::text_muted() })),
            Span::styled(format!("{:<18}", "Conversation"), Style::default().fg(name_color)),
            Span::styled(format!("{conv_tokens:>6}"), Style::default().fg(theme::accent_dim())),
            Span::styled(" ", base_style),
        ]));
    }

    // Render fixed contexts (always visible)
    for &i in &fixed_indices {
        if let Some(ctx) = state.context.get(i) {
            render_context_line(&mut lines, ctx, i, &lctx);
        }
    }

    // Calculate pagination for dynamic contexts
    let total_dynamic = dynamic_indices.len();
    let total_pages = if total_dynamic == 0 { 1 } else { total_dynamic.div_ceil(MAX_DYNAMIC_PER_PAGE) };

    // Determine current page based on selected context
    let current_page = dynamic_indices
        .iter()
        .position(|&i| i == state.selected_context)
        .map_or(0, |selected_pos| selected_pos.checked_div(MAX_DYNAMIC_PER_PAGE).unwrap_or(0));

    // Get dynamic contexts for current page
    let page_start = current_page.saturating_mul(MAX_DYNAMIC_PER_PAGE);
    let page_end = page_start.saturating_add(MAX_DYNAMIC_PER_PAGE).min(total_dynamic);
    let page_indices: Vec<usize> = dynamic_indices.get(page_start..page_end).unwrap_or(&[]).to_vec();

    // Add separator if there are dynamic contexts
    if total_dynamic > 0 {
        lines
            .push(Line::from(vec![Span::styled(format!("  {:─<32}", ""), Style::default().fg(theme::border_muted()))]));

        // Render dynamic contexts for current page
        for &i in &page_indices {
            if let Some(ctx) = state.context.get(i) {
                render_context_line(&mut lines, ctx, i, &lctx);
            }
        }

        // Page indicator (only if more than one page)
        if total_pages > 1 {
            lines.push(Line::from(vec![Span::styled(
                format!("  page {}/{}", current_page.saturating_add(1), total_pages),
                Style::default().fg(theme::text_muted()),
            )]));
        }
    }

    // Separator
    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        format!(" {}", chars::HORIZONTAL.repeat(34)),
        Style::default().fg(theme::border()),
    )]));

    // Token usage bar - full width with hit/miss coloring
    let bar_width = 34usize;
    let threshold_pct = state.cleaning_threshold;

    // Format: "12.5K / 140K threshold / 200K budget"
    let current = format_number(total_tokens);
    let threshold = format_number(threshold_tokens);
    let budget = format_number(max_tokens);

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(" ", base_style),
        Span::styled(current, Style::default().fg(theme::text()).bold()),
        Span::styled(" / ", Style::default().fg(theme::text_muted())),
        Span::styled(threshold, Style::default().fg(theme::warning())),
        Span::styled(" / ", Style::default().fg(theme::text_muted())),
        Span::styled(budget, Style::default().fg(theme::accent())),
    ]));

    // Calculate bar segment positions
    let hit_pct = if max_tokens > 0 { hit_tokens.to_f64() / max_tokens.to_f64() } else { 0.0 };
    let miss_pct = if max_tokens > 0 { miss_tokens.to_f64() / max_tokens.to_f64() } else { 0.0 };
    let hit_filled = (hit_pct * bar_width.to_f64()).to_usize();
    let miss_filled = (miss_pct * bar_width.to_f64()).to_usize();
    let total_filled = hit_filled.saturating_add(miss_filled).min(bar_width);
    let threshold_pos = (threshold_pct.to_f64() * bar_width.to_f64()).to_usize();

    // Build bar: [green hit][orange miss][empty]
    let mut bar_spans = vec![Span::styled(" ", base_style)];
    for i in 0..bar_width {
        let is_threshold = i == threshold_pos && threshold_pos < bar_width;
        let char = if is_threshold {
            "|"
        } else if i < total_filled {
            chars::BLOCK_FULL
        } else {
            chars::BLOCK_LIGHT
        };

        let color = if is_threshold {
            theme::warning()
        } else if i < hit_filled {
            theme::success() // green = cache hit
        } else if i < total_filled {
            theme::warning() // orange = cache miss
        } else {
            theme::bg_elevated()
        };

        bar_spans.push(Span::styled(char, Style::default().fg(color)));
    }
    lines.push(Line::from(bar_spans));

    // Separator before token stats
    // PR card (if current branch has an active PR)
    if let Some(pr) = &cp_mod_github::types::GithubState::get(state).branch_pr {
        let state_color = match pr.state.as_str() {
            "OPEN" => theme::success(),
            "MERGED" => theme::accent(),
            "CLOSED" => theme::error(),
            _ => theme::text_secondary(),
        };

        // PR number + state
        lines.push(Line::from(vec![
            Span::styled(" ", base_style),
            Span::styled(format!("PR#{}", pr.number), Style::default().fg(theme::accent()).bold()),
            Span::styled(" ", base_style),
            Span::styled(pr.state.to_lowercase(), Style::default().fg(state_color)),
        ]));

        // Title (truncated)
        let title = truncate_string(&pr.title, 32);
        lines.push(Line::from(vec![
            Span::styled(" ", base_style),
            Span::styled(title, Style::default().fg(theme::text_secondary())),
        ]));

        // +/- stats and review/checks on one line
        let mut detail_spans = vec![Span::styled(" ", base_style)];
        if let (Some(add), Some(del)) = (pr.additions, pr.deletions) {
            detail_spans.push(Span::styled(format!("+{add}"), Style::default().fg(theme::success())));
            detail_spans.push(Span::styled(format!(" -{del}"), Style::default().fg(theme::error())));
        }
        if let Some(ref review) = pr.review_decision {
            let (review_icon, review_color) = match review.as_str() {
                "APPROVED" => (" ✓", theme::success()),
                "CHANGES_REQUESTED" => (" ✗", theme::error()),
                "REVIEW_REQUIRED" => (" ●", theme::warning()),
                _ => (" ?", theme::text_muted()),
            };
            detail_spans.push(Span::styled(review_icon, Style::default().fg(review_color)));
        }
        if let Some(ref checks) = pr.checks_status {
            let (check_icon, check_color) = match checks.as_str() {
                "passing" => (" ●", theme::success()),
                "failing" => (" ●", theme::error()),
                "pending" => (" ●", theme::warning()),
                _ => (" ●", theme::text_muted()),
            };
            detail_spans.push(Span::styled(check_icon, Style::default().fg(check_color)));
        }
        if detail_spans.len() > 1 {
            lines.push(Line::from(detail_spans));
        }

        lines.push(Line::from(vec![Span::styled(
            format!(" {}", chars::HORIZONTAL.repeat(34)),
            Style::default().fg(theme::border()),
        )]));
        lines.push(Line::from(""));
    }

    // Token stats (cache hit / cache miss / output table + total cost)
    lines.extend(super::token_stats::render_token_stats(state));

    let paragraph = Paragraph::new(lines).style(base_style);
    let Some(&context_area) = sidebar_layout.first() else { return };
    frame.render_widget(paragraph, context_area);

    // Help hints at bottom of sidebar
    let help_entries = [
        ("Tab", "next panel"),
        ("↑↓", "scroll"),
        ("Ctrl+P", "commands"),
        ("Ctrl+H", "config"),
        ("Ctrl+V", "view"),
        ("Ctrl+Q", "quit"),
    ];
    let help_lines: Vec<Line<'_>> = help_entries
        .into_iter()
        .map(|(key, desc)| {
            Line::from(vec![
                Span::styled("  ", base_style),
                Span::styled(key, Style::default().fg(theme::accent())),
                Span::styled(format!(" {desc}"), Style::default().fg(theme::text_muted())),
            ])
        })
        .collect();

    let help_paragraph = Paragraph::new(help_lines).style(base_style);
    let Some(&help_area) = sidebar_layout.get(1) else { return };
    frame.render_widget(help_paragraph, help_area);
}

/// Layout context for rendering a single sidebar context line.
struct SidebarLineCtx<'ctx> {
    /// Application state for selection and module lookups.
    state: &'ctx State,
    /// Alignment width for panel IDs.
    id_width: usize,
    /// Current spinner frame character.
    spin: &'ctx str,
    /// Base style for background spans.
    base_style: Style,
}

/// Render a single context line in the sidebar panel list.
fn render_context_line(
    lines: &mut Vec<Line<'static>>,
    ctx: &crate::state::Entry,
    array_index: usize,
    lctx: &SidebarLineCtx<'_>,
) {
    let state = lctx.state;
    let is_selected = array_index == state.selected_context;
    let icon = ctx.context_type.icon();

    // Check if this context is loading (has no cached content but needs it)
    let is_loading = ctx.cached_content.is_none() && ctx.context_type.needs_cache();

    // Build the line — fixed panels show a count badge instead of Px ID
    let shortcut = if ctx.context_type.is_fixed() {
        let badge = fixed_panel_badge(ctx.context_type.as_str(), lctx.state).unwrap_or_default();
        format!("{badge:>id_width$}", id_width = lctx.id_width)
    } else {
        format!("{:>width$}", &ctx.id, width = lctx.id_width)
    };
    let name = truncate_string(&ctx.name, 18);

    // Show spinner instead of token count when loading
    // Show page indicator for paginated panels
    let tokens_or_spinner = if is_loading {
        format!("{:>6}", lctx.spin)
    } else if ctx.total_pages > 1 {
        format!("{}/{}", ctx.current_page.saturating_add(1), ctx.total_pages)
    } else {
        format_number(ctx.token_count)
    };

    let indicator = if is_selected { chars::ARROW_RIGHT } else { " " };

    // Selected element: orange text, no background change
    // Loading elements: dimmed
    let name_color = if is_loading {
        theme::text_muted()
    } else if is_selected {
        theme::accent()
    } else {
        theme::text_secondary()
    };
    let indicator_color = if is_selected { theme::accent() } else { theme::bg_base() };
    let tokens_color = if is_loading { theme::warning() } else { theme::accent_dim() };

    lines.push(Line::from(vec![
        Span::styled(format!(" {indicator}"), Style::default().fg(indicator_color)),
        Span::styled(format!(" {shortcut} "), Style::default().fg(theme::text_muted())),
        Span::styled(icon, Style::default().fg(if is_selected { theme::accent() } else { theme::text_muted() })),
        Span::styled(format!("{name:<18}"), Style::default().fg(name_color)),
        Span::styled(format!("{tokens_or_spinner:>6}"), Style::default().fg(tokens_color)),
        Span::styled(" ", lctx.base_style),
    ]));
}
