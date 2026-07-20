//! Performance overlay adapter — renders [`PerfOverlay`] IR to ratatui.
//!
//! Consumes the pre-built IR snapshot instead of reading perf metrics
//! directly. All data and semantic styling decisions are made by the
//! IR builder; this module only maps them to ratatui widgets.

use cp_render::Semantic;
use cp_render::conversation::PerfOverlay;
use ratatui::Frame;
use ratatui::prelude::{Line, Rect, Span, Style};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};

use crate::ui::ir::semantic_to_style;
use crate::ui::{chars, theme};
use cp_base::cast::Safe as _;
use cp_base::cast::float_math;

/// Render the performance overlay from its IR snapshot.
pub(crate) fn render_perf_overlay_from_ir(frame: &mut Frame<'_>, area: Rect, perf: &PerfOverlay) {
    // Overlay dimensions
    let overlay_width = 62u16;
    let overlay_height = 30u16;

    // Position in top-right
    let x = area.width.saturating_sub(overlay_width.saturating_add(2));
    let y = 1;
    let overlay_area = Rect::new(x, y, overlay_width, overlay_height.min(area.height.saturating_sub(2)));

    let mut lines: Vec<Line<'_>> = Vec::new();

    // FPS and frame time
    lines.push(Line::from(vec![
        Span::styled(format!(" FPS: {:.0}", perf.fps), semantic_to_style(perf.frame_semantic).bold()),
        Span::styled(
            format!("  Frame: {:.1}ms avg  {:.1}ms max", perf.frame_avg_ms, perf.frame_max_ms),
            semantic_to_style(Semantic::Muted),
        ),
    ]));

    // CPU and RAM
    lines.push(Line::from(vec![
        Span::styled(format!(" CPU: {:.1}%", perf.cpu_usage), semantic_to_style(perf.cpu_semantic)),
        Span::styled(format!("  RAM: {:.1} MB", perf.memory_mb), semantic_to_style(Semantic::Muted)),
    ]));

    // Open file descriptors
    lines.push(Line::from(vec![
        Span::styled(format!(" FDs: {}", perf.open_fds), semantic_to_style(perf.fd_semantic)),
        Span::styled(format!(" / {}", perf.fd_limit_soft), semantic_to_style(Semantic::Muted)),
    ]));

    // Meilisearch process stats
    if let Some(meili) = perf.meili.as_ref() {
        lines.push(Line::from(vec![
            Span::styled(format!(" Meili CPU: {:.1}%", meili.cpu_pct), semantic_to_style(meili.cpu_semantic)),
            Span::styled(format!("  RAM: {:.1} MB", meili.memory_mb), semantic_to_style(Semantic::Muted)),
        ]));
    }
    lines.push(Line::from(""));

    // Budget bars
    for budget_bar in &perf.budget_bars {
        lines.push(render_budget_bar(budget_bar));
    }

    // Sparkline
    lines.push(Line::from(""));
    lines.push(render_sparkline(&perf.sparkline));
    lines.push(Line::from(""));

    // Operation table
    render_op_table(&perf.operations, &mut lines);

    // Footer
    lines.push(Line::from(vec![
        Span::styled(" F12", semantic_to_style(Semantic::Accent)),
        Span::styled(" toggle  ", semantic_to_style(Semantic::Muted)),
        Span::styled("!", semantic_to_style(Semantic::Warning)),
        Span::styled(" hotspot (>30%)", semantic_to_style(Semantic::Muted)),
    ]));

    // Render popup
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::border()))
        .style(Style::default().bg(theme::bg_base()))
        .title(Span::styled(" Perf ", Style::default().fg(theme::accent()).bold()));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(Clear, overlay_area);
    frame.render_widget(paragraph, overlay_area);
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Render a budget bar from IR data.
fn render_budget_bar(budget_bar: &cp_render::conversation::PerfBudgetBar) -> Line<'static> {
    let bar_width = 30usize;
    let filled = float_math::mul(float_math::div(budget_bar.percent, 100.0f64), bar_width.to_f64()).to_usize();

    Line::from(vec![
        Span::styled(format!(" {:<6}", budget_bar.label), semantic_to_style(Semantic::Muted)),
        Span::styled(chars::BLOCK_FULL.repeat(filled.min(bar_width)), semantic_to_style(budget_bar.semantic)),
        Span::styled(
            chars::BLOCK_LIGHT.repeat(bar_width.saturating_sub(filled)),
            Style::default().fg(theme::bg_elevated()),
        ),
        Span::styled(format!(" {:>5.0}%", budget_bar.percent), semantic_to_style(budget_bar.semantic)),
    ])
}

/// Render a sparkline from frame time samples.
fn render_sparkline(values: &[f64]) -> Line<'static> {
    const SPARK_CHARS: &[char] =
        &['\u{2581}', '\u{2582}', '\u{2583}', '\u{2584}', '\u{2585}', '\u{2586}', '\u{2587}', '\u{2588}'];

    if values.is_empty() {
        return Line::from(vec![
            Span::styled(" Recent: ", semantic_to_style(Semantic::Muted)),
            Span::styled("(collecting...)", semantic_to_style(Semantic::Muted)),
        ]);
    }

    let max_val = values.iter().copied().fold(1.0f64, f64::max);
    let sparkline: String = values
        .iter()
        .map(|&v| {
            let idx =
                float_math::mul(float_math::div(v, max_val), SPARK_CHARS.len().saturating_sub(1).to_f64()).to_usize();
            let clamped = idx.min(SPARK_CHARS.len().saturating_sub(1));
            SPARK_CHARS.get(clamped).copied().unwrap_or('\u{2581}')
        })
        .collect();

    Line::from(vec![
        Span::styled(" Recent: ", semantic_to_style(Semantic::Muted)),
        Span::styled(sparkline, semantic_to_style(Semantic::Accent)),
    ])
}

/// Render the operation table using IR semantic styles.
fn render_op_table(ops: &[cp_render::conversation::PerfOp], lines: &mut Vec<Line<'static>>) {
    // Column definitions: name, mean, std, cumul
    let headers = ["Operation", "Mean", "Std", "Cumul"];
    let aligns = [false, true, true, true]; // false = left, true = right
    let widths = op_table_widths(ops, &headers);

    let border_style = semantic_to_style(Semantic::Border);
    let header_style = semantic_to_style(Semantic::Accent).bold();

    lines.push(op_table_header_row(&HeaderRowCtx {
        headers: &headers,
        widths: &widths,
        aligns,
        header_style,
        border_style,
    }));
    lines.push(op_table_separator_row(&widths, border_style));
    for op in ops {
        lines.push(op_table_data_row(op, &widths, border_style));
    }
}

/// Pad and align `text` to `width` (right-align when `right_align`).
fn pad_cell(text: &str, width: usize, right_align: bool) -> String {
    use unicode_width::UnicodeWidthStr as _;
    let deficit = width.saturating_sub(text.width());
    if right_align { format!("{}{text}", " ".repeat(deficit)) } else { format!("{text}{}", " ".repeat(deficit)) }
}

/// Compute per-column widths: max of header width and every op's cell width.
fn op_table_widths(ops: &[cp_render::conversation::PerfOp], headers: &[&str; 4]) -> Vec<usize> {
    use unicode_width::UnicodeWidthStr as _;
    let mut widths: Vec<usize> = headers.iter().map(|h| h.width()).collect();
    for op in ops {
        let name_w = op.name.len().saturating_add(2); // "! " or "  " prefix
        if let Some(w) = widths.first_mut() {
            *w = (*w).max(name_w);
        }
        if let Some(w) = widths.get_mut(1) {
            *w = (*w).max(format!("{:.2}ms", op.mean_ms).len());
        }
        if let Some(w) = widths.get_mut(2) {
            *w = (*w).max(format!("{:.2}ms", op.std_ms).len());
        }
        if let Some(w) = widths.get_mut(3) {
            *w = (*w).max(op.total_display.len());
        }
    }
    widths
}

/// Column layout + styling for the perf-table header row.
struct HeaderRowCtx<'ctx> {
    /// Column header labels.
    headers: &'ctx [&'ctx str; 4],
    /// Per-column display widths.
    widths: &'ctx [usize],
    /// Per-column right-align flags (false = left).
    aligns: [bool; 4],
    /// Style for header text.
    header_style: Style,
    /// Style for the `│` separators.
    border_style: Style,
}

/// Build the header row: ` Operation │ Mean │ Std │ Cumul`.
fn op_table_header_row(ctx: &HeaderRowCtx<'_>) -> Line<'static> {
    let mut spans = vec![Span::raw(" ")];
    for (i, hdr) in ctx.headers.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" \u{2502} ", ctx.border_style));
        }
        let w = ctx.widths.get(i).copied().unwrap_or(0);
        spans.push(Span::styled(pad_cell(hdr, w, *ctx.aligns.get(i).unwrap_or(&false)), ctx.header_style));
    }
    Line::from(spans)
}

/// Build the header/data separator row: `───┼───┼───┼───`.
fn op_table_separator_row(widths: &[usize], border_style: Style) -> Line<'static> {
    let mut sep_spans = vec![Span::raw(" ")];
    for (i, width) in widths.iter().enumerate() {
        if i > 0 {
            sep_spans.push(Span::styled("\u{2500}\u{253c}\u{2500}", border_style));
        }
        sep_spans.push(Span::styled("\u{2500}".repeat(*width), border_style));
    }
    Line::from(sep_spans)
}

/// Build one operation data row (hotspot-marked name + mean/std/cumul cells).
fn op_table_data_row(op: &cp_render::conversation::PerfOp, widths: &[usize], border_style: Style) -> Line<'static> {
    let name_prefix = if op.is_hotspot { "! " } else { "  " };
    let name_str = format!("{name_prefix}{}", op.name);
    let name_style =
        if op.is_hotspot { semantic_to_style(Semantic::Warning).bold() } else { semantic_to_style(Semantic::Default) };
    let mean_str = format!("{:.2}ms", op.mean_ms);
    let std_str = format!("{:.2}ms", op.std_ms);

    let mut row_spans = vec![Span::raw(" ")];
    row_spans.push(Span::styled(pad_cell(&name_str, widths.first().copied().unwrap_or(0), false), name_style));
    row_spans.push(Span::styled(" \u{2502} ", border_style));
    row_spans.push(Span::styled(
        pad_cell(&mean_str, widths.get(1).copied().unwrap_or(0), true),
        semantic_to_style(op.mean_semantic),
    ));
    row_spans.push(Span::styled(" \u{2502} ", border_style));
    row_spans.push(Span::styled(
        pad_cell(&std_str, widths.get(2).copied().unwrap_or(0), true),
        semantic_to_style(op.std_semantic),
    ));
    row_spans.push(Span::styled(" \u{2502} ", border_style));
    row_spans.push(Span::styled(
        pad_cell(&op.total_display, widths.get(3).copied().unwrap_or(0), true),
        semantic_to_style(Semantic::Muted),
    ));
    Line::from(row_spans)
}
