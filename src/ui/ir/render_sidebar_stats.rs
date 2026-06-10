//! Token statistics rendering for the sidebar.
//!
//! Extracted from `render_sidebar.rs` to stay within the 500-line limit.
//! Renders the hit/miss/output table, cache breakpoint gauge, and total
//! cost — wrapped in rounded borders (╭╮╰╯).

use cp_render::frame::TokenStats;
use ratatui::prelude::{Line, Span, Style};
use unicode_width::UnicodeWidthStr as _;

use crate::ui::{chars, helpers::format_number, theme};
use cp_base::cast::Safe as _;

use super::render_sidebar::padded;

/// Render the token statistics table from IR, wrapped in rounded borders.
pub(super) fn render_token_stats(lines: &mut Vec<Line<'static>>, stats: &TokenStats, cw: usize) {
    let border_style = Style::default().fg(theme::border_muted());
    let inner_width = cw.saturating_sub(2); // space between │ and │

    let format_cost = |cost: Option<f64>| -> String {
        cost.map_or(String::new(), |c| {
            if c >= 1000.0 {
                format!("${:.1}K", c / 1000.0)
            } else if c < 0.01 {
                format!("${c:.3}")
            } else if c < 1.0 {
                format!("${c:.2}")
            } else {
                format!("${c:.1}")
            }
        })
    };

    // ── Build content lines (no indent — borders handle alignment) ───

    let mut content: Vec<Line<'static>> = Vec::new();

    let hit_icon = chars::ARROW_UP.to_string();
    let miss_icon = chars::CROSS.to_string();
    let out_icon = chars::ARROW_DOWN.to_string();

    // Render table manually with border_muted separators
    // Column widths: label(4), hit(6), miss(6), out(6)
    let col_label_w = 4usize;
    let col_hit_w = 6usize;
    let col_miss_w = 6usize;
    let col_out_w = 6usize;

    // Header row
    content.push(Line::from(vec![
        Span::styled(format!("{:<col_label_w$}", ""), Style::default()),
        Span::styled(" │ ", border_style),
        Span::styled(format!("{:>col_hit_w$}", format!("{hit_icon} hit")), Style::default().fg(theme::success())),
        Span::styled(" │ ", border_style),
        Span::styled(format!("{:>col_miss_w$}", format!("{miss_icon} miss")), Style::default().fg(theme::warning())),
        Span::styled(" │ ", border_style),
        Span::styled(format!("{:>col_out_w$}", format!("{out_icon} out")), Style::default().fg(theme::accent_dim())),
    ]));

    // Header separator
    content.push(Line::from(vec![
        Span::styled("─".repeat(col_label_w), border_style),
        Span::styled("─┼─", border_style),
        Span::styled("─".repeat(col_hit_w), border_style),
        Span::styled("─┼─", border_style),
        Span::styled("─".repeat(col_miss_w), border_style),
        Span::styled("─┼─", border_style),
        Span::styled("─".repeat(col_out_w), border_style),
    ]));

    for row in &stats.rows {
        // Data row
        content.push(Line::from(vec![
            Span::styled(format!("{:<col_label_w$}", row.label), Style::default().fg(theme::text_muted())),
            Span::styled(" │ ", border_style),
            Span::styled(
                format!("{:>col_hit_w$}", format_number(row.hit.to_usize())),
                Style::default().fg(theme::success()),
            ),
            Span::styled(" │ ", border_style),
            Span::styled(
                format!("{:>col_miss_w$}", format_number(row.miss.to_usize())),
                Style::default().fg(theme::warning()),
            ),
            Span::styled(" │ ", border_style),
            Span::styled(
                format!("{:>col_out_w$}", format_number(row.output.to_usize())),
                Style::default().fg(theme::accent_dim()),
            ),
        ]));

        // Cost row (if any cost is non-empty)
        let hit_cost = format_cost(row.hit_cost);
        let miss_cost = format_cost(row.miss_cost);
        let out_cost = format_cost(row.output_cost);

        if !hit_cost.is_empty() || !miss_cost.is_empty() || !out_cost.is_empty() {
            content.push(Line::from(vec![
                Span::styled(format!("{:<col_label_w$}", ""), Style::default()),
                Span::styled(" │ ", border_style),
                Span::styled(format!("{hit_cost:>col_hit_w$}"), Style::default().fg(theme::text_muted())),
                Span::styled(" │ ", border_style),
                Span::styled(format!("{miss_cost:>col_miss_w$}"), Style::default().fg(theme::text_muted())),
                Span::styled(" │ ", border_style),
                Span::styled(format!("{out_cost:>col_out_w$}"), Style::default().fg(theme::text_muted())),
            ]));
        }
    }

    // Uncached input tokens
    if stats.uncached_input > 0 {
        content.push(Line::from(vec![Span::styled(
            format!("uncached: {}", format_number(stats.uncached_input.to_usize())),
            Style::default().fg(theme::error()),
        )]));
    }

    // Alive cache breakpoints
    if stats.alive_breakpoints > 0 {
        content.push(Line::from(vec![Span::styled(
            format!("alive BPs: {}", stats.alive_breakpoints),
            Style::default().fg(theme::success()),
        )]));

        if !stats.alive_bp_positions.is_empty() {
            let gauge_width = inner_width;
            let mut gauge_spans = Vec::new();
            for i in 0..gauge_width {
                let col_permille_start = i.saturating_mul(1000).checked_div(gauge_width).unwrap_or(0);
                let col_permille_end = (i.saturating_add(1)).saturating_mul(1000).checked_div(gauge_width).unwrap_or(0);
                let has_bp = stats
                    .alive_bp_positions
                    .iter()
                    .any(|&p| usize::from(p) >= col_permille_start && usize::from(p) < col_permille_end);
                if has_bp {
                    gauge_spans.push(Span::styled("|", Style::default().fg(theme::success())));
                } else {
                    gauge_spans.push(Span::styled(chars::BLOCK_LIGHT, Style::default().fg(theme::bg_elevated())));
                }
            }
            content.push(Line::from(gauge_spans));
        }
    }

    // Total cost
    if let Some(total) = stats.total_cost {
        let total_str = if total >= 1000.0 {
            format!("${:.1}K", total / 1000.0)
        } else if total < 0.01 {
            format!("${total:.3}")
        } else {
            format!("${total:.2}")
        };
        content.push(Line::from(vec![Span::styled(
            format!("total: {total_str}"),
            Style::default().fg(theme::text_muted()),
        )]));
    }

    // ── Wrap content in rounded borders ──────────────────────────────

    // Top border: ╭───...───╮
    lines.push(padded(vec![
        Span::styled("╭", border_style),
        Span::styled("─".repeat(inner_width), border_style),
        Span::styled("╮", border_style),
    ]));

    // Content lines: │ content ... │
    for content_line in content {
        let line_width: usize = content_line.spans.iter().map(|s| s.content.width()).sum();
        let pad = inner_width.saturating_sub(line_width);
        let mut spans = Vec::with_capacity(content_line.spans.len().saturating_add(4));
        spans.push(Span::raw(" ")); // structural 1-char indent
        spans.push(Span::styled("│", border_style));
        spans.extend(content_line.spans);
        spans.push(Span::raw(" ".repeat(pad)));
        spans.push(Span::styled("│", border_style));
        lines.push(Line::from(spans));
    }

    // Bottom border: ╰───...───╯
    lines.push(padded(vec![
        Span::styled("╰", border_style),
        Span::styled("─".repeat(inner_width), border_style),
        Span::styled("╯", border_style),
    ]));
}
