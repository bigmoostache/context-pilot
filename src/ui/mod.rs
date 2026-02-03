pub mod chars;
pub mod helpers;
mod input;
pub mod markdown;
mod sidebar;
pub mod spinner;
pub mod theme;

use ratatui::{
    prelude::*,
    widgets::{Block, Borders, BorderType, Clear, Paragraph},
};

use crate::constants::{SIDEBAR_WIDTH, STATUS_BAR_HEIGHT};
use crate::panels;
use crate::perf::{PERF, FRAME_BUDGET_60FPS, FRAME_BUDGET_30FPS};
use crate::state::{ContextType, State};


pub fn render(frame: &mut Frame, state: &mut State) {
    PERF.frame_start();
    let _guard = crate::profile!("ui::render");
    let area = frame.area();

    // Fill base background
    frame.render_widget(
        Block::default().style(Style::default().bg(theme::BG_BASE)),
        area
    );

    // Main layout: body + footer (no header)
    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),                        // Body
            Constraint::Length(STATUS_BAR_HEIGHT),    // Status bar
        ])
        .split(area);

    render_body(frame, state, main_layout[0]);
    input::render_status_bar(frame, state, main_layout[1]);

    // Render performance overlay if enabled
    if state.perf_enabled {
        render_perf_overlay(frame, area);
    }

    PERF.frame_end();
}

fn render_body(frame: &mut Frame, state: &mut State, area: Rect) {
    // Body layout: sidebar + main content
    let body_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(SIDEBAR_WIDTH),  // Sidebar
            Constraint::Min(1),                 // Main content
        ])
        .split(area);

    sidebar::render_sidebar(frame, state, body_layout[0]);
    render_main_content(frame, state, body_layout[1]);
}

fn render_main_content(frame: &mut Frame, state: &mut State, area: Rect) {
    // No separate input box - panels handle their own input display
    render_content_panel(frame, state, area);
}

fn render_content_panel(frame: &mut Frame, state: &mut State, area: Rect) {
    let _guard = crate::profile!("ui::render_panel");
    let context_type = state.context.get(state.selected_context)
        .map(|c| c.context_type)
        .unwrap_or(ContextType::Conversation);

    let panel = panels::get_panel(context_type);
    panel.render(frame, state, area);
}

fn render_perf_overlay(frame: &mut Frame, area: Rect) {
    let snapshot = PERF.snapshot();

    // Overlay dimensions
    let overlay_width = 54u16;
    let overlay_height = 16u16;

    // Position in top-right
    let x = area.width.saturating_sub(overlay_width + 2);
    let y = 1;
    let overlay_area = Rect::new(x, y, overlay_width, overlay_height.min(area.height.saturating_sub(2)));

    // Build content lines
    let mut lines: Vec<Line> = Vec::new();

    // FPS and frame time
    let fps = if snapshot.frame_avg_ms > 0.0 {
        1000.0 / snapshot.frame_avg_ms
    } else {
        0.0
    };
    let fps_color = frame_time_color(snapshot.frame_avg_ms);

    lines.push(Line::from(vec![
        Span::styled(format!(" FPS: {:.0}", fps), Style::default().fg(fps_color).bold()),
        Span::styled(format!("  Frame: {:.1}ms avg  {:.1}ms max", snapshot.frame_avg_ms, snapshot.frame_max_ms), Style::default().fg(theme::TEXT_MUTED)),
    ]));
    lines.push(Line::from(""));

    // Budget bars
    lines.push(render_budget_bar(snapshot.frame_avg_ms, "60fps", FRAME_BUDGET_60FPS));
    lines.push(render_budget_bar(snapshot.frame_avg_ms, "30fps", FRAME_BUDGET_30FPS));

    // Sparkline
    lines.push(Line::from(""));
    lines.push(render_sparkline(&snapshot.frame_times_ms));

    // Separator
    lines.push(Line::from(vec![
        Span::styled(format!(" {}", chars::HORIZONTAL.repeat(50)), Style::default().fg(theme::BORDER)),
    ]));

    // Operation table header
    lines.push(Line::from(vec![
        Span::styled(" ", Style::default()),
        Span::styled(format!("{:<26}", "Operation"), Style::default().fg(theme::TEXT_SECONDARY)),
        Span::styled(format!("{:>10}", "Total"), Style::default().fg(theme::TEXT_SECONDARY)),
        Span::styled(format!("{:>10}", "Max"), Style::default().fg(theme::TEXT_SECONDARY)),
    ]));

    // Calculate total for percentage
    let total_time: f64 = snapshot.ops.iter().map(|o| o.total_ms).sum();

    // Top operations
    for op in snapshot.ops.iter().take(5) {
        let pct = if total_time > 0.0 { op.total_ms / total_time * 100.0 } else { 0.0 };
        let is_hotspot = pct > 30.0;

        let name = truncate_op_name(op.name, 25);
        let marker = if is_hotspot { "!" } else { " " };

        let name_style = if is_hotspot {
            Style::default().fg(theme::WARNING).bold()
        } else {
            Style::default().fg(theme::TEXT)
        };

        lines.push(Line::from(vec![
            Span::styled(marker, Style::default().fg(theme::WARNING)),
            Span::styled(format!("{:<26}", name), name_style),
            Span::styled(format!("{:>9.1}ms", op.total_ms), Style::default().fg(theme::ACCENT)),
            Span::styled(format!("{:>9.2}ms", op.max_ms), Style::default().fg(frame_time_color(op.max_ms))),
        ]));
    }

    // Footer
    lines.push(Line::from(vec![
        Span::styled(format!(" {}", chars::HORIZONTAL.repeat(50)), Style::default().fg(theme::BORDER)),
    ]));
    lines.push(Line::from(vec![
        Span::styled(" F12", Style::default().fg(theme::ACCENT)),
        Span::styled(" toggle  ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("!", Style::default().fg(theme::WARNING)),
        Span::styled(" hotspot (>30%)", Style::default().fg(theme::TEXT_MUTED)),
    ]));

    // Render
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::BORDER))
        .style(Style::default().bg(Color::Rgb(20, 20, 28)))
        .title(Span::styled(" Perf ", Style::default().fg(theme::ACCENT).bold()));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(Clear, overlay_area);
    frame.render_widget(paragraph, overlay_area);
}

fn frame_time_color(ms: f64) -> Color {
    if ms < FRAME_BUDGET_60FPS {
        theme::SUCCESS
    } else if ms < FRAME_BUDGET_30FPS {
        theme::WARNING
    } else {
        theme::ERROR
    }
}

fn render_budget_bar(current_ms: f64, label: &str, budget_ms: f64) -> Line<'static> {
    let pct = (current_ms / budget_ms * 100.0).min(150.0);
    let bar_width = 30usize;
    let filled = ((pct / 100.0) * bar_width as f64) as usize;

    let color = if pct <= 80.0 {
        theme::SUCCESS
    } else if pct <= 100.0 {
        theme::WARNING
    } else {
        theme::ERROR
    };

    Line::from(vec![
        Span::styled(format!(" {:<6}", label), Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(chars::BLOCK_FULL.repeat(filled.min(bar_width)), Style::default().fg(color)),
        Span::styled(chars::BLOCK_LIGHT.repeat(bar_width.saturating_sub(filled)), Style::default().fg(theme::BG_ELEVATED)),
        Span::styled(format!(" {:>5.0}%", pct), Style::default().fg(color)),
    ])
}

fn render_sparkline(values: &[f64]) -> Line<'static> {
    const SPARK_CHARS: &[char] = &['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

    if values.is_empty() {
        return Line::from(vec![
            Span::styled(" Recent: ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("(collecting...)", Style::default().fg(theme::TEXT_MUTED)),
        ]);
    }

    let max_val = values.iter().cloned().fold(1.0_f64, f64::max);
    let sparkline: String = values
        .iter()
        .map(|&v| {
            let idx = ((v / max_val) * (SPARK_CHARS.len() - 1) as f64) as usize;
            SPARK_CHARS[idx.min(SPARK_CHARS.len() - 1)]
        })
        .collect();

    Line::from(vec![
        Span::styled(" Recent: ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(sparkline, Style::default().fg(theme::ACCENT)),
    ])
}

fn truncate_op_name(name: &str, max_len: usize) -> String {
    if name.len() <= max_len {
        name.to_string()
    } else {
        format!("..{}", &name[name.len() - max_len + 2..])
    }
}
