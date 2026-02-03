use ratatui::{
    prelude::*,
    widgets::Paragraph,
};

use crate::constants::SIDEBAR_HELP_HEIGHT;
use crate::state::State;
use super::{theme, chars, spinner, helpers::*};

pub fn render_sidebar(frame: &mut Frame, state: &State, area: Rect) {
    let _guard = crate::profile!("ui::sidebar");
    let base_style = Style::default().bg(theme::BG_BASE);

    // Sidebar layout: context list + help hints
    let sidebar_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),                        // Context list
            Constraint::Length(SIDEBAR_HELP_HEIGHT),   // Help hints
        ])
        .split(area);

    // Context list
    let mut lines: Vec<Line> = vec![
        Line::from(vec![
            Span::styled("  ", base_style),
            Span::styled("CONTEXT", Style::default().fg(theme::TEXT_MUTED).bold()),
        ]),
        Line::from(""),
    ];

    let total_tokens: usize = state.context.iter().map(|c| c.token_count).sum();
    let max_tokens = state.effective_context_budget();
    let threshold_tokens = state.cleaning_threshold_tokens();

    // Calculate ID width for alignment based on longest ID
    let id_width = state.context.iter().map(|c| c.id.len()).max().unwrap_or(2);

    let spin = spinner::spinner(state.spinner_frame);

    // Sort contexts by ID for display (P1, P2, P3, ...)
    let mut sorted_indices: Vec<usize> = (0..state.context.len()).collect();
    sorted_indices.sort_by(|&a, &b| {
        let id_a = state.context[a].id.strip_prefix('P').and_then(|n| n.parse::<usize>().ok()).unwrap_or(usize::MAX);
        let id_b = state.context[b].id.strip_prefix('P').and_then(|n| n.parse::<usize>().ok()).unwrap_or(usize::MAX);
        id_a.cmp(&id_b)
    });

    let mut prev_was_fixed = true;
    for &i in &sorted_indices {
        let ctx = &state.context[i];
        let is_fixed = ctx.context_type.is_fixed();

        // Add separator when transitioning from fixed to dynamic contexts
        if prev_was_fixed && !is_fixed {
            lines.push(Line::from(vec![
                Span::styled(format!("  {:─<32}", ""), Style::default().fg(theme::BORDER_MUTED)),
            ]));
        }
        prev_was_fixed = is_fixed;

        let is_selected = i == state.selected_context;
        let icon = ctx.context_type.icon();

        // Check if this context is loading (has no cached content but needs it)
        let is_loading = ctx.cached_content.is_none() && ctx.context_type.needs_cache();

        // Build the line with right-aligned ID
        let shortcut = format!("{:>width$}", &ctx.id, width = id_width);
        let name = truncate_string(&ctx.name, 18);

        // Show spinner instead of token count when loading
        let tokens_or_spinner = if is_loading {
            format!("{:>6}", spin)
        } else {
            format_number(ctx.token_count)
        };

        let indicator = if is_selected { chars::ARROW_RIGHT } else { " " };

        // Selected element: orange text, no background change
        // Loading elements: dimmed
        let name_color = if is_loading {
            theme::TEXT_MUTED
        } else if is_selected {
            theme::ACCENT
        } else {
            theme::TEXT_SECONDARY
        };
        let indicator_color = if is_selected { theme::ACCENT } else { theme::BG_BASE };
        let tokens_color = if is_loading { theme::WARNING } else { theme::ACCENT_DIM };

        lines.push(Line::from(vec![
            Span::styled(format!(" {}", indicator), Style::default().fg(indicator_color)),
            Span::styled(format!(" {} ", shortcut), Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(format!("{} ", icon), Style::default().fg(if is_selected { theme::ACCENT } else { theme::TEXT_MUTED })),
            Span::styled(format!("{:<18}", name), Style::default().fg(name_color)),
            Span::styled(format!("{:>6}", tokens_or_spinner), Style::default().fg(tokens_color)),
            Span::styled(" ", base_style),
        ]));
    }

    // Separator
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(format!(" {}", chars::HORIZONTAL.repeat(34)), Style::default().fg(theme::BORDER)),
    ]));

    // Token usage bar - full width
    let bar_width = 34usize;
    let threshold_pct = state.cleaning_threshold;
    let usage_pct = (total_tokens as f64 / max_tokens as f64).min(1.0);

    // Calculate bar positions
    let filled = (usage_pct * bar_width as f64) as usize;
    let threshold_pos = (threshold_pct as f64 * bar_width as f64) as usize;

    // Color based on threshold
    let bar_color = if total_tokens >= threshold_tokens {
        theme::ERROR
    } else if total_tokens as f64 >= threshold_tokens as f64 * 0.9 {
        theme::WARNING
    } else {
        theme::ACCENT
    };

    // Format: "12.5K / 140K threshold / 200K budget"
    let current = format_number(total_tokens);
    let threshold = format_number(threshold_tokens);
    let budget = format_number(max_tokens);

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(" ", base_style),
        Span::styled(&current, Style::default().fg(theme::TEXT).bold()),
        Span::styled(" / ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(&threshold, Style::default().fg(theme::WARNING)),
        Span::styled(" / ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(&budget, Style::default().fg(theme::ACCENT)),
    ]));

    // Build bar with threshold marker
    let mut bar_spans = vec![Span::styled(" ", base_style)];
    for i in 0..bar_width {
        let char = if i == threshold_pos && threshold_pos < bar_width {
            "|" // Threshold marker
        } else if i < filled {
            chars::BLOCK_FULL
        } else {
            chars::BLOCK_LIGHT
        };

        let color = if i == threshold_pos {
            theme::WARNING
        } else if i < filled {
            bar_color
        } else {
            theme::BG_ELEVATED
        };

        bar_spans.push(Span::styled(char, Style::default().fg(color)));
    }
    lines.push(Line::from(bar_spans));

    let paragraph = Paragraph::new(lines)
        .style(base_style);
    frame.render_widget(paragraph, sidebar_layout[0]);

    // Help hints at bottom of sidebar
    let help_lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  ", base_style),
            Span::styled("Enter", Style::default().fg(theme::ACCENT)),
            Span::styled(" send", Style::default().fg(theme::TEXT_MUTED)),
        ]),
        Line::from(vec![
            Span::styled("  ", base_style),
            Span::styled("Tab", Style::default().fg(theme::ACCENT)),
            Span::styled(" next panel", Style::default().fg(theme::TEXT_MUTED)),
        ]),
        Line::from(vec![
            Span::styled("  ", base_style),
            Span::styled("↑↓", Style::default().fg(theme::ACCENT)),
            Span::styled(" scroll", Style::default().fg(theme::TEXT_MUTED)),
        ]),
        Line::from(vec![
            Span::styled("  ", base_style),
            Span::styled("Ctrl+P", Style::default().fg(theme::ACCENT)),
            Span::styled(" commands", Style::default().fg(theme::TEXT_MUTED)),
        ]),
        Line::from(vec![
            Span::styled("  ", base_style),
            Span::styled("Ctrl+K", Style::default().fg(theme::ACCENT)),
            Span::styled(" clean", Style::default().fg(theme::TEXT_MUTED)),
        ]),
        Line::from(vec![
            Span::styled("  ", base_style),
            Span::styled("Ctrl+Q", Style::default().fg(theme::ACCENT)),
            Span::styled(" quit", Style::default().fg(theme::TEXT_MUTED)),
        ]),
    ];

    let help_paragraph = Paragraph::new(help_lines)
        .style(base_style);
    frame.render_widget(help_paragraph, sidebar_layout[1]);
}
