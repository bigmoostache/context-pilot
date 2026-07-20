//! Display column builders for the search index overlay.
//!
//! Converts [`SearchIndexOverlay`] IR data into ratatui [`Line`] sequences
//! for the two-column layout (left: server/database/index/extensions,
//! right: splitter/embeddings/tasks/recomputed/recent).

use cp_render::overlay_ir::SearchIndexOverlay;
use ratatui::prelude::Style;
use ratatui::text::{Line, Span};

use crate::ui::theme;

/// Build display columns from IR data → ratatui Lines.
pub(super) fn build_display_columns(overlay: &SearchIndexOverlay) -> (Vec<Line<'static>>, Vec<Line<'static>>) {
    if !overlay.server.online && overlay.server.url.is_empty() {
        let fallback = vec![Line::from(""), Line::from("  Search module not initialized.")];
        return (fallback, Vec::new());
    }
    let left = build_left_display(overlay);
    let right = build_right_display(overlay);
    (left, right)
}

/// Build left-column display lines (server, database, index, extensions).
fn build_left_display(o: &SearchIndexOverlay) -> Vec<Line<'static>> {
    let mut lines = Vec::with_capacity(24);
    let sem = crate::ui::ir::semantic_to_style;

    // Server
    let (status_label, status_color) =
        if o.server.online { ("\u{25cf} online", theme::success()) } else { ("\u{25cb} offline", theme::error()) };
    let version_label = if o.server.version.is_empty() { String::new() } else { format!("  {}", o.server.version) };

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::raw("  Server  "),
        Span::styled(o.server.url.clone(), Style::default().fg(theme::text())),
        Span::raw("  "),
        Span::styled(status_label, Style::default().fg(status_color)),
        Span::styled(version_label, Style::default().fg(theme::text_muted())),
    ]));

    // Process
    if let (Some(cpu), Some(mem)) = (o.server.cpu_pct, o.server.memory_display.as_ref()) {
        lines.push(Line::from(vec![
            Span::raw("  Process "),
            Span::styled(format!("CPU {cpu:.1}%"), sem(o.server.cpu_semantic)),
            Span::raw("    "),
            Span::styled(format!("RAM {mem}"), Style::default().fg(theme::text())),
        ]));
    }

    push_database_section(&mut lines, o);
    push_index_section(&mut lines, o);
    push_extensions_section(&mut lines, o);

    lines
}

/// Push the Database section (disk used/total, docs, avg chunk) when non-empty.
fn push_database_section(lines: &mut Vec<Line<'static>>, o: &SearchIndexOverlay) {
    if o.index.disk_total.is_empty() || o.index.disk_total == "0 B" {
        return;
    }
    lines.push(Line::from(""));
    lines.push(section_header("Database"));
    lines.push(Line::from(vec![
        Span::raw("  Disk  "),
        Span::styled(o.index.disk_used.clone(), Style::default().fg(theme::text())),
        Span::styled(" / ", Style::default().fg(theme::text_muted())),
        Span::styled(o.index.disk_total.clone(), Style::default().fg(theme::text_muted())),
        Span::raw("    "),
        Span::styled("Docs  ", Style::default().fg(theme::text_muted())),
        Span::styled(o.index.docs_display.clone(), Style::default().fg(theme::text())),
    ]));
    if let Some(avg) = &(o.index.avg_chunk) {
        lines.push(Line::from(vec![
            Span::raw("  Avg chunk  "),
            Span::styled(avg.clone(), Style::default().fg(theme::text())),
        ]));
    }
}

/// Push the Index section (files/chunks/queue/errors/status/last).
fn push_index_section(lines: &mut Vec<Line<'static>>, o: &SearchIndexOverlay) {
    lines.push(Line::from(""));
    lines.push(section_header("Index"));
    lines.push(Line::from(format!("  Files  {:<10} Chunks  {}", o.index.files_indexed, o.index.chunks_indexed)));
    lines.push(Line::from(format!(
        "  Queue  {:<10} Errors  {}",
        format!("{} pending", o.index.queue_depth),
        o.index.error_count,
    )));
    let ready = if o.index.index_ready { "Ready" } else { "Scanning\u{2026}" };
    lines.push(Line::from(format!("  Status {ready:<10} Last    {}", o.index.last_activity)));
}

/// Push the Extensions section (per-ext bar chart) when non-empty.
fn push_extensions_section(lines: &mut Vec<Line<'static>>, o: &SearchIndexOverlay) {
    if o.extensions.is_empty() {
        return;
    }
    lines.push(Line::from(""));
    lines.push(section_header("Extensions"));
    for ext in &o.extensions {
        let fill = "\u{2588}".repeat(ext.bar_width);
        lines.push(Line::from(vec![
            Span::raw(format!("  {:<6} {:>4}  ", ext.name, ext.count)),
            Span::styled(fill, Style::default().fg(theme::accent())),
            Span::styled(format!("  {}%", ext.pct), Style::default().fg(theme::text_muted())),
        ]));
    }
}

/// Build right-column display lines (splitter, embeddings, tasks, recomputed, recent).
fn build_right_display(o: &SearchIndexOverlay) -> Vec<Line<'static>> {
    let mut lines = Vec::with_capacity(32);
    push_splitter_section(&mut lines, o);
    push_embeddings_section(&mut lines, o);
    push_recent_tasks_section(&mut lines, o);
    push_top_recomputed_section(&mut lines, o);
    push_recently_sent_section(&mut lines, o);
    lines
}

/// Push the Splitter section (tree-sitter vs fallback chunk counts) when present.
fn push_splitter_section(lines: &mut Vec<Line<'static>>, o: &SearchIndexOverlay) {
    let Some(sp) = &(o.splitter) else { return };
    lines.push(Line::from(""));
    lines.push(section_header("Splitter"));
    lines.push(Line::from(vec![
        Span::raw("  Tree-sitter  "),
        Span::styled(format!("{} chunks", sp.tree_sitter_chunks), Style::default().fg(theme::success())),
        Span::styled(format!("  ({}%)", sp.tree_sitter_pct), Style::default().fg(theme::text_muted())),
    ]));
    lines.push(Line::from(vec![
        Span::raw("  Fallback     "),
        Span::styled(format!("{} chunks", sp.fallback_chunks), Style::default().fg(theme::warning())),
        Span::styled(format!("  ({}%)", sp.fallback_pct), Style::default().fg(theme::text_muted())),
    ]));
}

/// Push the Embeddings section (model, vectors, coverage, logs) when present.
fn push_embeddings_section(lines: &mut Vec<Line<'static>>, o: &SearchIndexOverlay) {
    let sem = crate::ui::ir::semantic_to_style;
    let Some(emb) = &(o.embeddings) else { return };
    lines.push(Line::from(""));
    lines.push(section_header("Embeddings"));
    if !emb.model.is_empty() {
        lines.push(Line::from(vec![
            Span::raw("  Model   "),
            Span::styled(emb.model.clone(), Style::default().fg(theme::text())),
        ]));
    }
    let (emb_label, emb_color) =
        if emb.is_indexing { ("\u{25cf} generating", theme::warning()) } else { ("\u{2713} ready", theme::success()) };
    lines.push(Line::from(vec![
        Span::raw(format!("  Vectors {:>4}  ", emb.vector_count)),
        Span::styled(emb_label, Style::default().fg(emb_color)),
    ]));
    if emb.total_docs > 0 {
        lines.push(Line::from(vec![
            Span::raw("  Coverage "),
            Span::styled(format!("{}/{}", emb.embedded_docs, emb.total_docs), sem(emb.coverage_semantic)),
            Span::styled(format!("  ({}%)", emb.coverage_pct), Style::default().fg(theme::text_muted())),
        ]));
    }
    if emb.logs_doc_count > 0 {
        lines.push(Line::from(format!("  Logs    {} documents", emb.logs_doc_count)));
    }
}

/// Push the Recent Tasks section (per-task type/status/duration) when non-empty.
fn push_recent_tasks_section(lines: &mut Vec<Line<'static>>, o: &SearchIndexOverlay) {
    let sem = crate::ui::ir::semantic_to_style;
    if o.recent_tasks.is_empty() {
        return;
    }
    lines.push(Line::from(""));
    lines.push(section_header("Recent Tasks"));
    for task in &o.recent_tasks {
        lines.push(Line::from(vec![
            Span::styled(format!("  #{:<6}", task.uid), Style::default().fg(theme::text_muted())),
            Span::raw(format!("{:<10} ", task.task_type)),
            Span::styled(format!("{:<10} ", task.status), sem(task.status_semantic)),
            Span::styled(task.duration.clone(), Style::default().fg(theme::text_muted())),
        ]));
    }
}

/// Push the Top Recomputed section (path recompute counts) when non-empty.
fn push_top_recomputed_section(lines: &mut Vec<Line<'static>>, o: &SearchIndexOverlay) {
    if o.top_recomputed.is_empty() {
        return;
    }
    lines.push(Line::from(""));
    lines.push(section_header("Top Recomputed"));
    for entry in &o.top_recomputed {
        lines.push(Line::from(vec![
            Span::styled(format!("  {:>4}×  ", entry.count), Style::default().fg(theme::warning())),
            Span::styled(entry.path.clone(), Style::default().fg(theme::text())),
        ]));
    }
}

/// Push the Recently Sent section (path + age) when non-empty.
fn push_recently_sent_section(lines: &mut Vec<Line<'static>>, o: &SearchIndexOverlay) {
    if o.recently_sent.is_empty() {
        return;
    }
    lines.push(Line::from(""));
    lines.push(section_header("Recently Sent"));
    for entry in &o.recently_sent {
        lines.push(Line::from(vec![
            Span::styled(format!("  {:>8}  ", entry.ago), Style::default().fg(theme::text_muted())),
            Span::styled(entry.path.clone(), Style::default().fg(theme::text())),
        ]));
    }
}

/// Render a section header line with dashes.
fn section_header(title: &str) -> Line<'static> {
    let dashes = "\u{2500}".repeat(48usize.saturating_sub(title.len()).saturating_sub(4));
    Line::from(vec![
        Span::styled(format!("  ── {title} "), Style::default().fg(theme::accent())),
        Span::styled(dashes, Style::default().fg(theme::text_muted())),
    ])
}
