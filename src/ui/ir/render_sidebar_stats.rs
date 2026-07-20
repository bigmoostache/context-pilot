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
use cp_base::cast::float_math;

use super::render_sidebar::padded;

/// Format an optional cost cell (`$K` tier ≥1000, 3dp <0.01, 2dp <1, else 1dp),
/// empty string for `None`.
fn format_cost(cost: Option<f64>) -> String {
    cost.map_or(String::new(), |c| {
        if c >= 1_000.0f64 {
            format!("${:.1}K", float_math::div(c, 1_000.0f64))
        } else if c < 0.01f64 {
            format!("${c:.3}")
        } else if c < 1.0f64 {
            format!("${c:.2}")
        } else {
            format!("${c:.1}")
        }
    })
}

/// Format the total-cost figure (`$K` tier ≥1000, 3dp <0.01, else 2dp).
fn format_total_cost(total: f64) -> String {
    if total >= 1_000.0f64 {
        format!("${:.1}K", float_math::div(total, 1_000.0f64))
    } else if total < 0.01f64 {
        format!("${total:.3}")
    } else {
        format!("${total:.2}")
    }
}

/// Push the alive-breakpoint count line + its position gauge (when any exist).
fn push_bp_lines(content: &mut Vec<Line<'static>>, stats: &TokenStats, inner_width: usize) {
    if stats.alive_breakpoints == 0 {
        return;
    }
    content.push(Line::from(vec![Span::styled(
        format!("alive BPs: {}", stats.alive_breakpoints),
        Style::default().fg(theme::success()),
    )]));
    if !stats.alive_bp_positions.is_empty() {
        content.push(Line::from(build_bp_gauge(&stats.alive_bp_positions, inner_width)));
    }
}

/// Render the token statistics table from IR, wrapped in rounded borders.
pub(super) fn render_token_stats(lines: &mut Vec<Line<'static>>, stats: &TokenStats, cw: usize) {
    let border_style = Style::default().fg(theme::border_muted());
    let inner_width = cw.saturating_sub(2); // space between │ and │

    // ── Build content lines (no indent — borders handle alignment) ───

    let mut content: Vec<Line<'static>> = Vec::new();

    let hit_icon = chars::ARROW_UP.to_owned();
    let miss_icon = chars::CROSS.to_owned();
    let out_icon = chars::ARROW_DOWN.to_owned();

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
        push_stat_row(&mut content, row, &format_cost, (col_label_w, col_hit_w, col_miss_w, col_out_w));
    }

    // Uncached input tokens
    if stats.uncached_input > 0 {
        content.push(Line::from(vec![Span::styled(
            format!("uncached: {}", format_number(stats.uncached_input.to_usize())),
            Style::default().fg(theme::error()),
        )]));
    }

    // Alive cache breakpoints
    push_bp_lines(&mut content, stats, inner_width);

    // Total cost
    if let Some(total) = stats.total_cost {
        content.push(Line::from(vec![Span::styled(
            format!("total: {}", format_total_cost(total)),
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

/// Push one token-stat data row (label + hit/miss/out counts) plus, when any
/// leg has a non-empty cost, a following cost row. `widths` = (label, hit, miss, out).
fn push_stat_row(
    content: &mut Vec<Line<'static>>,
    row: &cp_render::frame::TokenRow,
    format_cost: &impl Fn(Option<f64>) -> String,
    widths: (usize, usize, usize, usize),
) {
    let (col_label_w, col_hit_w, col_miss_w, col_out_w) = widths;
    let border_style = Style::default().fg(theme::border_muted());

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

    let hit_cost = format_cost(row.hit_cost);
    let miss_cost = format_cost(row.miss_cost);
    let out_cost = format_cost(row.output_cost);
    if hit_cost.is_empty() && miss_cost.is_empty() && out_cost.is_empty() {
        return;
    }
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

/// Build the alive-breakpoint gauge: one cell per column, marked `|` where a
/// breakpoint's per-mille position falls, else a light block.
fn build_bp_gauge(alive_bp_positions: &[u16], gauge_width: usize) -> Vec<Span<'static>> {
    let mut gauge_spans = Vec::new();
    for i in 0..gauge_width {
        let col_permille_start = i.saturating_mul(1000).checked_div(gauge_width).unwrap_or(0);
        let col_permille_end = (i.saturating_add(1)).saturating_mul(1000).checked_div(gauge_width).unwrap_or(0);
        let has_bp = alive_bp_positions
            .iter()
            .any(|&p| usize::from(p) >= col_permille_start && usize::from(p) < col_permille_end);
        if has_bp {
            gauge_spans.push(Span::styled("|", Style::default().fg(theme::success())));
        } else {
            gauge_spans.push(Span::styled(chars::BLOCK_LIGHT, Style::default().fg(theme::bg_elevated())));
        }
    }
    gauge_spans
}
