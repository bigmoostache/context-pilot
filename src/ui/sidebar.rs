use ratatui::{
    prelude::*,
    widgets::Paragraph,
};

use crate::constants::{MAX_CONTEXT_TOKENS, SIDEBAR_HELP_HEIGHT};
use crate::state::State;
use super::{theme, chars, helpers::*};

pub fn render_sidebar(frame: &mut Frame, state: &State, area: Rect) {
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
    let max_tokens = MAX_CONTEXT_TOKENS;

    // Calculate ID width for alignment based on longest ID
    let id_width = state.context.iter().map(|c| c.id.len()).max().unwrap_or(2);

    let mut prev_was_fixed = true;
    for (i, ctx) in state.context.iter().enumerate() {
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

        // Build the line with right-aligned ID
        let shortcut = format!("{:>width$}", &ctx.id, width = id_width);
        let name = truncate_string(&ctx.name, 18);
        let tokens = format_number(ctx.token_count);

        let indicator = if is_selected { chars::ARROW_RIGHT } else { " " };

        // Selected element: orange text, no background change
        let name_color = if is_selected { theme::ACCENT } else { theme::TEXT_SECONDARY };
        let indicator_color = if is_selected { theme::ACCENT } else { theme::BG_BASE };

        lines.push(Line::from(vec![
            Span::styled(format!(" {}", indicator), Style::default().fg(indicator_color)),
            Span::styled(format!(" {} ", shortcut), Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(format!("{} ", icon), Style::default().fg(if is_selected { theme::ACCENT } else { theme::TEXT_MUTED })),
            Span::styled(format!("{:<18}", name), Style::default().fg(name_color)),
            Span::styled(format!("{:>6}", tokens), Style::default().fg(theme::ACCENT_DIM)),
            Span::styled(" ", base_style),
        ]));
    }

    // Separator
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(format!(" {}", chars::HORIZONTAL.repeat(34)), Style::default().fg(theme::BORDER)),
    ]));

    // Token usage bar - full width
    let usage_pct = (total_tokens as f64 / max_tokens as f64 * 100.0).min(100.0);
    let bar_width = 34; // Full sidebar width minus margins
    let filled = ((usage_pct / 100.0) * bar_width as f64) as usize;
    let empty = bar_width - filled;

    let bar_color = if usage_pct > 80.0 {
        theme::WARNING
    } else {
        theme::ACCENT
    };

    // Format: "12.5K/100K (45%)"
    let current = format_number(total_tokens);
    let max = format_number(max_tokens);
    let pct = format!("{:.0}%", usage_pct);

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(" ", base_style),
        Span::styled(&current, Style::default().fg(theme::TEXT).bold()),
        Span::styled("/", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(&max, Style::default().fg(theme::ACCENT).bold()),
        Span::styled(format!(" ({})", pct), Style::default().fg(theme::TEXT_MUTED)),
    ]));
    lines.push(Line::from(vec![
        Span::styled(" ", base_style),
        Span::styled(chars::BLOCK_FULL.repeat(filled), Style::default().fg(bar_color)),
        Span::styled(chars::BLOCK_LIGHT.repeat(empty), Style::default().fg(theme::BG_ELEVATED)),
    ]));

    let paragraph = Paragraph::new(lines)
        .style(base_style);
    frame.render_widget(paragraph, sidebar_layout[0]);

    // Help hints at bottom of sidebar
    let help_lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  ", base_style),
            Span::styled("Shift+Enter", Style::default().fg(theme::ACCENT)),
            Span::styled(" send", Style::default().fg(theme::TEXT_MUTED)),
        ]),
        Line::from(vec![
            Span::styled("  ", base_style),
            Span::styled("Ctrl+L", Style::default().fg(theme::ACCENT)),
            Span::styled(" clear", Style::default().fg(theme::TEXT_MUTED)),
        ]),
        Line::from(vec![
            Span::styled("  ", base_style),
            Span::styled("Ctrl+Y", Style::default().fg(theme::ACCENT)),
            Span::styled(" copy mode", Style::default().fg(theme::TEXT_MUTED)),
        ]),
        Line::from(vec![
            Span::styled("  ", base_style),
            Span::styled("Ctrl+K", Style::default().fg(theme::ACCENT)),
            Span::styled(" clean context", Style::default().fg(theme::TEXT_MUTED)),
        ]),
        Line::from(vec![
            Span::styled("  ", base_style),
            Span::styled("Ctrl+↑↓", Style::default().fg(theme::ACCENT)),
            Span::styled(" scroll", Style::default().fg(theme::TEXT_MUTED)),
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
