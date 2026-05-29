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
    if let Some(ref meili) = perf.meili {
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
    let filled = ((budget_bar.percent / 100.0) * bar_width.to_f64()).to_usize();

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
    const SPARK_CHARS: &[char] = &['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

    if values.is_empty() {
        return Line::from(vec![
            Span::styled(" Recent: ", semantic_to_style(Semantic::Muted)),
            Span::styled("(collecting...)", semantic_to_style(Semantic::Muted)),
        ]);
    }

    let max_val = values.iter().copied().fold(1.0_f64, f64::max);
    let sparkline: String = values
        .iter()
        .map(|&v| {
            let idx = ((v / max_val) * SPARK_CHARS.len().saturating_sub(1).to_f64()).to_usize();
            let clamped = idx.min(SPARK_CHARS.len().saturating_sub(1));
            SPARK_CHARS.get(clamped).copied().unwrap_or('▁')
        })
        .collect();

    Line::from(vec![
        Span::styled(" Recent: ", semantic_to_style(Semantic::Muted)),
        Span::styled(sparkline, semantic_to_style(Semantic::Accent)),
    ])
}

/// Render the operation table using IR semantic styles.
fn render_op_table(ops: &[cp_render::conversation::PerfOp], lines: &mut Vec<Line<'static>>) {
    use unicode_width::UnicodeWidthStr as _;

    // Column definitions: name, mean, std, cumul
    let headers = ["Operation", "Mean", "Std", "Cumul"];
    let aligns = [false, true, true, true]; // false = left, true = right

    // Compute column widths
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

    let border_style = semantic_to_style(Semantic::Border);
    let header_style = semantic_to_style(Semantic::Accent).bold();

    // Helper to pad and align
    let pad = |text: &str, width: usize, right_align: bool| -> String {
        let deficit = width.saturating_sub(text.width());
        if right_align { format!("{}{text}", " ".repeat(deficit)) } else { format!("{text}{}", " ".repeat(deficit)) }
    };

    // Header row
    let mut spans = vec![Span::raw(" ")];
    for (i, hdr) in headers.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" │ ", border_style));
        }
        let w = widths.get(i).copied().unwrap_or(0);
        spans.push(Span::styled(pad(hdr, w, *aligns.get(i).unwrap_or(&false)), header_style));
    }
    lines.push(Line::from(spans));

    // Separator
    let mut sep_spans = vec![Span::raw(" ")];
    for (i, width) in widths.iter().enumerate() {
        if i > 0 {
            sep_spans.push(Span::styled("─┼─", border_style));
        }
        sep_spans.push(Span::styled("─".repeat(*width), border_style));
    }
    lines.push(Line::from(sep_spans));

    // Data rows
    for op in ops {
        let name_prefix = if op.is_hotspot { "! " } else { "  " };
        let name_str = format!("{name_prefix}{}", op.name);
        let name_style = if op.is_hotspot {
            semantic_to_style(Semantic::Warning).bold()
        } else {
            semantic_to_style(Semantic::Default)
        };

        let mean_str = format!("{:.2}ms", op.mean_ms);
        let std_str = format!("{:.2}ms", op.std_ms);

        let mut row_spans = vec![Span::raw(" ")];
        // Name
        row_spans.push(Span::styled(pad(&name_str, widths.first().copied().unwrap_or(0), false), name_style));
        // Mean
        row_spans.push(Span::styled(" │ ", border_style));
        row_spans.push(Span::styled(
            pad(&mean_str, widths.get(1).copied().unwrap_or(0), true),
            semantic_to_style(op.mean_semantic),
        ));
        // Std
        row_spans.push(Span::styled(" │ ", border_style));
        row_spans.push(Span::styled(
            pad(&std_str, widths.get(2).copied().unwrap_or(0), true),
            semantic_to_style(op.std_semantic),
        ));
        // Cumul
        row_spans.push(Span::styled(" │ ", border_style));
        row_spans.push(Span::styled(
            pad(&op.total_display, widths.get(3).copied().unwrap_or(0), true),
            semantic_to_style(Semantic::Muted),
        ));

        lines.push(Line::from(row_spans));
    }
}
