use ratatui::prelude::{Constraint, Direction, Frame, Layout, Line, Rect, Span, Style};

use super::super::{helpers::format_number, theme};
use crate::state::{ContextType, State};

use super::full::fixed_panel_badge;

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
        .filter(|&i| {
            state.context.get(i).is_some_and(|c| c.context_type != ContextType::new(ContextType::CONVERSATION))
        })
        .partition(|&i| state.context.get(i).is_some_and(|c| c.context_type.is_fixed()));

    // Conversation entry
    if let Some(conv_idx) =
        state.context.iter().position(|c| c.context_type == ContextType::new(ContextType::CONVERSATION))
    {
        render_collapsed_line(&mut lines, conv_idx, state, base_style, None);
    }

    // Fixed panels
    for &i in &fixed_indices {
        let badge = state.context.get(i).and_then(|c| fixed_panel_badge(c.context_type.as_str(), state));
        render_collapsed_line(&mut lines, i, state, base_style, badge.as_deref());
    }

    // Dynamic panels separator + entries
    if !dynamic_indices.is_empty() {
        lines.push(Line::from(vec![Span::styled("  ──────────", Style::default().fg(theme::border_muted()))]));
        for &i in &dynamic_indices {
            render_collapsed_line(&mut lines, i, state, base_style, None);
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
#[expect(
    clippy::too_many_arguments,
    reason = "render_collapsed_line needs idx, state, style, and badge for collapsed sidebar display"
)]
fn render_collapsed_line(
    lines: &mut Vec<Line<'static>>,
    idx: usize,
    state: &State,
    base_style: Style,
    badge: Option<&str>,
) {
    let Some(ctx) = state.context.get(idx) else { return };
    let is_selected = idx == state.selected_context;
    let icon = ctx.context_type.icon();

    let arrow = if is_selected { "▸" } else { " " };
    let arrow_color = if is_selected { theme::accent() } else { theme::bg_base() };
    let icon_color = if is_selected { theme::accent() } else { theme::text_muted() };

    // Badge or short ID for dynamic panels
    let label = badge.map_or_else(
        || {
            if ctx.context_type.is_fixed() {
                "   ".to_string()
            } else {
                format!("{:>3}", &ctx.id.strip_prefix('P').unwrap_or(&ctx.id))
            }
        },
        |b| format!("{b:>3}"),
    );
    let label_color = if is_selected { theme::accent() } else { theme::text_muted() };

    // Token count (compact)
    let tokens = format_number(ctx.token_count);
    let tokens_color = theme::accent_dim();

    lines.push(Line::from(vec![
        Span::styled(format!(" {arrow}"), Style::default().fg(arrow_color)),
        Span::styled(icon, Style::default().fg(icon_color)),
        Span::styled(label, Style::default().fg(label_color)),
        Span::styled(format!("{tokens:>5}"), Style::default().fg(tokens_color)),
        Span::styled(" ", base_style),
    ]));
}
