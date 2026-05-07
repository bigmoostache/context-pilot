//! Ctrl+I Meilisearch indexing status overlay.
//!
//! Renders a floating, centered info box showing the Meilisearch server
//! status, indexing metrics, extension breakdown, splitter stats,
//! and OCR pipeline status.

use ratatui::Frame;
use ratatui::prelude::{Rect, Style};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::state::State;
use crate::ui::theme;

/// Overlay width in terminal cells.
const OVERLAY_WIDTH: u16 = 58;

/// Render the Meilisearch indexing status overlay.
///
/// Displays server status, index metrics, extension breakdown, splitter
/// stats, and OCR pipeline info in a centered, bordered box.
pub(crate) fn render_index_overlay(frame: &mut Frame<'_>, state: &State, area: Rect) {
    let lines = build_overlay_lines(state);
    let height = u16::try_from(lines.len().saturating_add(2)).unwrap_or(30).min(area.height);
    let popup = centered_rect(OVERLAY_WIDTH, height, area);

    let block = Block::default()
        .title(" Indexing Status ")
        .borders(Borders::ALL)
        .style(Style::default().bg(theme::bg_base()).fg(theme::text()));

    let paragraph = Paragraph::new(lines).block(block);

    frame.render_widget(Clear, popup);
    frame.render_widget(paragraph, popup);
}

/// Build the overlay content lines from the search module's state.
fn build_overlay_lines(state: &State) -> Vec<Line<'static>> {
    let Some(info) = cp_mod_search::overlay_info(state) else {
        return vec![
            Line::from(""),
            Line::from("  Search module not initialized."),
            Line::from(""),
            Line::from(dim_span("  Press Ctrl+I or Esc to dismiss")),
        ];
    };

    let mut lines = Vec::with_capacity(32);

    // ── Server ──
    let server_url = format!("http://127.0.0.1:{}", info.port);
    let (status_label, status_color) =
        if info.port > 0 { ("● online", theme::success()) } else { ("○ offline", theme::error()) };

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::raw("  Server  "),
        Span::styled(server_url, Style::default().fg(theme::text())),
        Span::raw("  "),
        Span::styled(status_label, Style::default().fg(status_color)),
    ]));

    // ── Core Stats (two-column) ──
    lines.push(Line::from(""));
    lines.push(Line::from(format!("  Files  {:<10} Chunks  {}", info.files_indexed, info.chunks_indexed,)));
    lines.push(Line::from(format!(
        "  Queue  {:<10} Errors  {}",
        format!("{} pending", info.queue_depth),
        info.error_count,
    )));
    let last = if info.last_activity_ms > 0 { format_ago(info.last_activity_ms) } else { "never".to_string() };
    let ready = if info.index_ready { "Ready" } else { "Scanning…" };
    lines.push(Line::from(format!("  Status {ready:<10} Last    {last}")));

    // ── Extensions ──
    if !info.top_extensions.is_empty() {
        lines.push(Line::from(""));
        lines.push(section_header("Extensions"));

        let max_count = info.top_extensions.first().map_or(1, |e| e.1.max(1));
        let total_files: u64 = info.top_extensions.iter().map(|e| e.1).sum();
        let bar_max_width: u64 = 22;

        for (ext, count) in &info.top_extensions {
            let bar_len = count.saturating_mul(bar_max_width).checked_div(max_count).unwrap_or(0);
            let bar_usize = usize::try_from(bar_len).unwrap_or(0).max(1);
            let fill = "█".repeat(bar_usize);
            let pct = if total_files > 0 { count.saturating_mul(100).checked_div(total_files).unwrap_or(0) } else { 0 };
            lines.push(Line::from(vec![
                Span::raw(format!("  {ext:<6} {count:>4}  ")),
                Span::styled(fill, Style::default().fg(theme::accent())),
                Span::styled(format!("  {pct}%"), Style::default().fg(theme::text_muted())),
            ]));
        }
    }

    // ── Splitter ──
    let total_chunks = info.tree_sitter_chunks.saturating_add(info.fallback_chunks);
    if total_chunks > 0 {
        lines.push(Line::from(""));
        lines.push(section_header("Splitter"));

        let ts_pct = info.tree_sitter_chunks.saturating_mul(100).checked_div(total_chunks).unwrap_or(0);
        let fb_pct = 100_u64.saturating_sub(ts_pct);

        lines.push(Line::from(vec![
            Span::raw("  Tree-sitter  "),
            Span::styled(format!("{} chunks", info.tree_sitter_chunks), Style::default().fg(theme::success())),
            Span::styled(format!("  ({ts_pct}%)"), Style::default().fg(theme::text_muted())),
        ]));
        lines.push(Line::from(vec![
            Span::raw("  Fallback     "),
            Span::styled(format!("{} chunks", info.fallback_chunks), Style::default().fg(theme::warning())),
            Span::styled(format!("  ({fb_pct}%)"), Style::default().fg(theme::text_muted())),
        ]));
    }

    // ── OCR Pipeline ──
    if info.ocr_available || info.ocr_attempted > 0 {
        lines.push(Line::from(""));
        lines.push(section_header("OCR Pipeline"));

        if info.ocr_attempted > 0 {
            lines.push(Line::from(format!(
                "  Attempted  {}   Succeeded  {}   Cached  {}",
                info.ocr_attempted, info.ocr_succeeded, info.ocr_cached,
            )));
            if info.ocr_failed > 0 {
                lines.push(Line::from(vec![
                    Span::raw("  Failed     "),
                    Span::styled(format!("{}", info.ocr_failed), Style::default().fg(theme::error())),
                ]));
            }
        } else {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled("Enabled", Style::default().fg(theme::success())),
                Span::styled(" — no OCR files found yet", Style::default().fg(theme::text_muted())),
            ]));
        }
    }

    // ── Footer ──
    lines.push(Line::from(""));
    lines.push(Line::from(dim_span("  Press Ctrl+I or Esc to dismiss")));

    lines
}

/// Render a section header line with dashes.
fn section_header(title: &str) -> Line<'static> {
    let dashes = "─".repeat(48_usize.saturating_sub(title.len()).saturating_sub(4));
    Line::from(vec![
        Span::styled(format!("  ── {title} "), Style::default().fg(theme::accent())),
        Span::styled(dashes, Style::default().fg(theme::text_muted())),
    ])
}

/// Compute a centered rectangle within the given area.
fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let eff_w = width.min(area.width);
    let eff_h = height.min(area.height);
    let x_off = area.width.saturating_sub(eff_w).checked_div(2).unwrap_or(0);
    let y_off = area.height.saturating_sub(eff_h).checked_div(2).unwrap_or(0);
    Rect::new(area.x.saturating_add(x_off), area.y.saturating_add(y_off), eff_w, eff_h)
}

/// Format a millisecond timestamp as a relative "X ago" string.
fn format_ago(ms_then: u64) -> String {
    let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
    let now_u64 = u64::try_from(now_ms).unwrap_or(u64::MAX);
    let diff_sec = now_u64.saturating_sub(ms_then).checked_div(1000).unwrap_or(0);
    if diff_sec < 60 {
        format!("{diff_sec}s ago")
    } else if diff_sec < 3600 {
        format!("{}m ago", diff_sec.checked_div(60).unwrap_or(0))
    } else {
        format!("{}h ago", diff_sec.checked_div(3600).unwrap_or(0))
    }
}

/// Create a dimmed span for hint text.
fn dim_span(text: &'static str) -> Span<'static> {
    Span::styled(text, Style::default().add_modifier(Modifier::DIM))
}
