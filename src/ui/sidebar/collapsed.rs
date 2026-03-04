use ratatui::prelude::*;

use super::super::{helpers::*, theme};
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

    let mut lines: Vec<Line<'_>> = Vec::new();
    lines.push(Line::from(""));

    // Sort contexts by panel ID order
    let mut sorted_indices: Vec<usize> = (0..state.context.len()).collect();
    sorted_indices.sort_by(|&a, &b| {
        let id_a = state.context[a].id.strip_prefix('P').and_then(|n| n.parse::<usize>().ok()).unwrap_or(usize::MAX);
        let id_b = state.context[b].id.strip_prefix('P').and_then(|n| n.parse::<usize>().ok()).unwrap_or(usize::MAX);
        id_a.cmp(&id_b)
    });

    // Separate fixed and dynamic, skip conversation (rendered separately)
    let (fixed_indices, dynamic_indices): (Vec<_>, Vec<_>) = sorted_indices
        .into_iter()
        .filter(|&i| state.context[i].context_type != ContextType::new(ContextType::CONVERSATION))
        .partition(|&i| state.context[i].context_type.is_fixed());

    // Conversation entry
    if let Some(conv_idx) =
        state.context.iter().position(|c| c.context_type == ContextType::new(ContextType::CONVERSATION))
    {
        render_collapsed_line(&mut lines, conv_idx, state, base_style, None);
    }

    // Fixed panels
    for &i in &fixed_indices {
        let badge = fixed_panel_badge(state.context[i].context_type.as_str(), state);
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
    frame.render_widget(paragraph, layout[0]);

    // Token summary at bottom (3 lines: current / threshold / budget)
    let system_prompt_tokens = {
        let sp = cp_mod_prompt::seed::get_active_agent_content(state);
        crate::state::estimate_tokens(&sp) * 2
    };
    let tool_def_tokens = crate::modules::overview::context::estimate_tool_definitions_tokens(state);
    let panel_tokens: usize = state.context.iter().map(|c| c.token_count).sum();
    let total_tokens = system_prompt_tokens + tool_def_tokens + panel_tokens;
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
    frame.render_widget(token_paragraph, layout[1]);
}

// Ye olde compass needle — points to the active panel
fn render_collapsed_line(
    lines: &mut Vec<Line<'static>>,
    idx: usize,
    state: &State,
    base_style: Style,
    badge: Option<&str>,
) {
    let ctx = &state.context[idx];
    let is_selected = idx == state.selected_context;
    let icon = ctx.context_type.icon();

    let arrow = if is_selected { "▸" } else { " " };
    let arrow_color = if is_selected { theme::accent() } else { theme::bg_base() };
    let icon_color = if is_selected { theme::accent() } else { theme::text_muted() };

    // Badge or short ID for dynamic panels
    let label = if let Some(b) = badge {
        format!("{:>3}", b)
    } else if ctx.context_type.is_fixed() {
        "   ".to_string()
    } else {
        format!("{:>3}", &ctx.id.strip_prefix('P').unwrap_or(&ctx.id))
    };
    let label_color = if is_selected { theme::accent() } else { theme::text_muted() };

    // Token count (compact)
    let tokens = format_number(ctx.token_count);
    let tokens_color = theme::accent_dim();

    lines.push(Line::from(vec![
        Span::styled(format!(" {}", arrow), Style::default().fg(arrow_color)),
        Span::styled(icon, Style::default().fg(icon_color)),
        Span::styled(label, Style::default().fg(label_color)),
        Span::styled(format!("{:>5}", tokens), Style::default().fg(tokens_color)),
        Span::styled(" ", base_style),
    ]));
}
