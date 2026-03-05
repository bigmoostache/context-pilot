//! Performance overlay rendering.
//!
//! Draws the F12 perf overlay with FPS, CPU/RAM, budget bars,
//! sparkline, and operation table.

use super::super::helpers::Cell;
use super::super::{chars, theme};
use super::{FRAME_BUDGET_30FPS, FRAME_BUDGET_60FPS, PERF};
use cp_base::cast::Safe as _;
use ratatui::Frame;
use ratatui::prelude::{Color, Line, Rect, Span, Style};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};

/// Render the performance overlay in the top-right corner.
pub(crate) fn render_perf_overlay(frame: &mut Frame<'_>, area: Rect) {
    use super::super::helpers::render_table;

    let snapshot = PERF.snapshot();

    // Overlay dimensions
    let overlay_width = 62u16;
    let overlay_height = 28u16;

    // Position in top-right
    let x = area.width.saturating_sub(overlay_width.saturating_add(2));
    let y = 1;
    let overlay_area = Rect::new(x, y, overlay_width, overlay_height.min(area.height.saturating_sub(2)));

    // Build content lines
    let mut lines: Vec<Line<'_>> = Vec::new();

    // FPS and frame time
    let fps = if snapshot.frame_avg_ms > 0.0 { 1000.0 / snapshot.frame_avg_ms } else { 0.0 };
    let fps_color = frame_time_color(snapshot.frame_avg_ms);

    lines.push(Line::from(vec![
        Span::styled(format!(" FPS: {fps:.0}"), Style::default().fg(fps_color).bold()),
        Span::styled(
            format!("  Frame: {:.1}ms avg  {:.1}ms max", snapshot.frame_avg_ms, snapshot.frame_max_ms),
            Style::default().fg(theme::text_muted()),
        ),
    ]));

    // CPU and RAM line
    let cpu_color = if snapshot.cpu_usage < 25.0 {
        theme::success()
    } else if snapshot.cpu_usage < 50.0 {
        theme::warning()
    } else {
        theme::error()
    };
    lines.push(Line::from(vec![
        Span::styled(format!(" CPU: {:.1}%", snapshot.cpu_usage), Style::default().fg(cpu_color)),
        Span::styled(format!("  RAM: {:.1} MB", snapshot.memory_mb), Style::default().fg(theme::text_muted())),
    ]));
    lines.push(Line::from(""));

    // Budget bars
    lines.push(render_budget_bar(snapshot.frame_avg_ms, "60fps", FRAME_BUDGET_60FPS));
    lines.push(render_budget_bar(snapshot.frame_avg_ms, "30fps", FRAME_BUDGET_30FPS));
    // Sparkline
    lines.push(Line::from(""));
    lines.push(render_sparkline(&snapshot.frame_times_ms));
    lines.push(Line::from(""));
    // Operation table using render_table
    let total_time: f64 = snapshot.ops.iter().map(|o| o.total_ms).sum();

    let header = [
        Cell::new("Operation", Style::default()),
        Cell::right("Mean", Style::default()),
        Cell::right("Std", Style::default()),
        Cell::right("Cumul", Style::default()),
    ];

    let rows: Vec<Vec<Cell>> = snapshot
        .ops
        .iter()
        .take(10)
        .map(|op| {
            let pct = if total_time > 0.0 { op.total_ms / total_time * 100.0 } else { 0.0 };
            let is_hotspot = pct > 30.0;

            let name = if op.name.len() <= 24 {
                op.name.to_string()
            } else {
                let tail_start = op.name.len().saturating_sub(22);
                format!("..{}", op.name.get(tail_start..).unwrap_or(""))
            };
            let name_str = if is_hotspot { format!("! {name}") } else { format!("  {name}") };

            let name_style = if is_hotspot {
                Style::default().fg(theme::warning()).bold()
            } else {
                Style::default().fg(theme::text())
            };

            let mean_color = frame_time_color(op.mean_ms);
            let std_color = if op.std_ms < 1.0 {
                theme::success()
            } else if op.std_ms < 5.0 {
                theme::warning()
            } else {
                theme::error()
            };

            let cumul_str = if op.total_ms >= 1000.0 {
                format!("{:.1}s", op.total_ms / 1000.0)
            } else {
                format!("{:.0}ms", op.total_ms)
            };

            vec![
                Cell::new(name_str, name_style),
                Cell::right(format!("{:.2}ms", op.mean_ms), Style::default().fg(mean_color)),
                Cell::right(format!("{:.2}ms", op.std_ms), Style::default().fg(std_color)),
                Cell::right(cumul_str, Style::default().fg(theme::text_muted())),
            ]
        })
        .collect();

    lines.extend(render_table(&header, &rows, None, 1));

    // Footer
    lines.push(Line::from(vec![
        Span::styled(" F12", Style::default().fg(theme::accent())),
        Span::styled(" toggle  ", Style::default().fg(theme::text_muted())),
        Span::styled("!", Style::default().fg(theme::warning())),
        Span::styled(" hotspot (>30%)", Style::default().fg(theme::text_muted())),
    ]));

    // Render
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::border()))
        .style(Style::default().bg(Color::Rgb(20, 20, 28)))
        .title(Span::styled(" Perf ", Style::default().fg(theme::accent()).bold()));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(Clear, overlay_area);
    frame.render_widget(paragraph, overlay_area);
}

/// Map frame time to a color (green/yellow/red).
fn frame_time_color(ms: f64) -> Color {
    if ms < FRAME_BUDGET_60FPS {
        theme::success()
    } else if ms < FRAME_BUDGET_30FPS {
        theme::warning()
    } else {
        theme::error()
    }
}

/// Render a budget bar showing current frame time as a percentage of the budget.
fn render_budget_bar(current_ms: f64, label: &str, budget_ms: f64) -> Line<'static> {
    let pct = (current_ms / budget_ms * 100.0).min(150.0);
    let bar_width = 30usize;
    let filled = ((pct / 100.0) * bar_width.to_f64()).to_usize();

    let color = if pct <= 80.0 {
        theme::success()
    } else if pct <= 100.0 {
        theme::warning()
    } else {
        theme::error()
    };

    Line::from(vec![
        Span::styled(format!(" {label:<6}"), Style::default().fg(theme::text_muted())),
        Span::styled(chars::BLOCK_FULL.repeat(filled.min(bar_width)), Style::default().fg(color)),
        Span::styled(
            chars::BLOCK_LIGHT.repeat(bar_width.saturating_sub(filled)),
            Style::default().fg(theme::bg_elevated()),
        ),
        Span::styled(format!(" {pct:>5.0}%"), Style::default().fg(color)),
    ])
}

/// Render a sparkline visualization of recent frame times.
fn render_sparkline(values: &[f64]) -> Line<'static> {
    const SPARK_CHARS: &[char] = &['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

    if values.is_empty() {
        return Line::from(vec![
            Span::styled(" Recent: ", Style::default().fg(theme::text_muted())),
            Span::styled("(collecting...)", Style::default().fg(theme::text_muted())),
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
        Span::styled(" Recent: ", Style::default().fg(theme::text_muted())),
        Span::styled(sparkline, Style::default().fg(theme::accent())),
    ])
}
