use ratatui::prelude::{Constraint, Direction, Frame, Layout, Line, Rect, Span, Style};

use super::super::{helpers::format_number, theme};
use crate::state::{Kind, State};

use super::full::fixed_panel_badge;

/// Context needed to render a single collapsed sidebar line.
struct CollapsedLineCtx<'ctx> {
    /// Index of the context panel in `state.context`.
    idx: usize,
    /// Application state used to look up panel data and selection.
    state: &'ctx State,
    /// Base background style for the sidebar.
    base_style: Style,
    /// Optional badge text (e.g. unread count) displayed next to the icon.
    badge: Option<&'ctx str>,
}

/// Render a collapsed sidebar (icons + badges only, ~14 columns wide).
/// Shows: selection arrow, icon, badge count. Token bar at the bottom.
pub(crate) fn render_sidebar_collapsed(frame: &mut Frame<'_>, state: &State, area: Rect) {
    let _guard = crate::profile!("ui::sidebar_collapsed");
    let base_style = Style::default().bg(theme::bg_base());

    // Layout: panel list + token summary at bottom
    let token_area_height = 5u16;
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(token_area_height)])
        .split(area);
    debug_assert!(layout.len() >= 2, "collapsed sidebar layout must have at least 2 chunks");

    let mut lines: Vec<Line<'_>> = Vec::new();
    lines.push(Line::from(""));

    // Sort contexts by panel ID order
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

    // Separate fixed and dynamic, skip conversation (rendered separately)
    let (fixed_indices, dynamic_indices): (Vec<_>, Vec<_>) = sorted_indices
        .into_iter()
        .filter(|&i| state.context.get(i).is_some_and(|c| c.context_type != Kind::new(Kind::CONVERSATION)))
        .partition(|&i| state.context.get(i).is_some_and(|c| c.context_type.is_fixed()));

    // Conversation entry
    if let Some(conv_idx) = state.context.iter().position(|c| c.context_type == Kind::new(Kind::CONVERSATION)) {
        render_collapsed_line(&mut lines, &CollapsedLineCtx { idx: conv_idx, state, base_style, badge: None });
    }

    // Fixed panels
    for &i in &fixed_indices {
        let badge = state.context.get(i).and_then(|c| fixed_panel_badge(c.context_type.as_str(), state));
        render_collapsed_line(&mut lines, &CollapsedLineCtx { idx: i, state, base_style, badge: badge.as_deref() });
    }

    // Dynamic panels separator + entries
    if !dynamic_indices.is_empty() {
        lines.push(Line::from(vec![Span::styled("  ──────────", Style::default().fg(theme::border_muted()))]));
        for &i in &dynamic_indices {
            render_collapsed_line(&mut lines, &CollapsedLineCtx { idx: i, state, base_style, badge: None });
        }
    }

    let paragraph = ratatui::widgets::Paragraph::new(lines).style(base_style);
    let Some(&panel_area) = layout.first() else { return };
    frame.render_widget(paragraph, panel_area);

    // Token summary at bottom (3 lines: current / threshold / budget)
    let system_prompt_tokens = {
        let sp = cp_mod_prompt::seed::get_active_agent_content(state);
        crate::state::estimate_tokens(&sp).saturating_mul(2)
    };
    let tool_def_tokens = crate::modules::overview::context::estimate_tool_definitions_tokens(state);
    let panel_tokens: usize = state.context.iter().map(|c| c.token_count).sum();
    let total_tokens = system_prompt_tokens.saturating_add(tool_def_tokens).saturating_add(panel_tokens);
    let max_tokens = state.effective_context_budget();
    let threshold_tokens = state.cleaning_threshold_tokens();

    let token_lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(" ", base_style),
            Span::styled(format_number(total_tokens), Style::default().fg(theme::text()).bold()),
        ]),
        Line::from(vec![
            Span::styled(" ", base_style),
            Span::styled(format_number(threshold_tokens), Style::default().fg(theme::warning())),
        ]),
        Line::from(vec![
            Span::styled(" ", base_style),
            Span::styled(format_number(max_tokens), Style::default().fg(theme::accent())),
        ]),
    ];

    let token_paragraph = ratatui::widgets::Paragraph::new(token_lines).style(base_style);
    let Some(&token_area) = layout.get(1) else { return };
    frame.render_widget(token_paragraph, token_area);
}

/// Render a single collapsed sidebar line for a context panel.
fn render_collapsed_line(lines: &mut Vec<Line<'static>>, ctx: &CollapsedLineCtx<'_>) {
    let Some(panel) = ctx.state.context.get(ctx.idx) else { return };
    let is_selected = ctx.idx == ctx.state.selected_context;
    let icon = panel.context_type.icon();

    let arrow = if is_selected { "▸" } else { " " };
    let arrow_color = if is_selected { theme::accent() } else { theme::bg_base() };
    let icon_color = if is_selected { theme::accent() } else { theme::text_muted() };

    // Badge or short ID for dynamic panels
    let label = ctx.badge.map_or_else(
        || {
            if panel.context_type.is_fixed() {
                "   ".to_string()
            } else {
                format!("{:>3}", &panel.id.strip_prefix('P').unwrap_or(&panel.id))
            }
        },
        |b| format!("{b:>3}"),
    );
    let label_color = if is_selected { theme::accent() } else { theme::text_muted() };

    // Token count (compact)
    let tokens = format_number(panel.token_count);
    let tokens_color = theme::accent_dim();

    lines.push(Line::from(vec![
        Span::styled(format!(" {arrow}"), Style::default().fg(arrow_color)),
        Span::styled(icon, Style::default().fg(icon_color)),
        Span::styled(label, Style::default().fg(label_color)),
        Span::styled(format!("{tokens:>5}"), Style::default().fg(tokens_color)),
        Span::styled(" ", ctx.base_style),
    ]));
}
