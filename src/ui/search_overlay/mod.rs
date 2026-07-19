//! Ctrl+I Meilisearch indexing status overlay.
//!
//! Builder produces [`SearchIndexOverlay`] IR from application state.
//! Adapter renders the IR to ratatui widgets.

/// Display column builders for the two-column overlay layout.
mod display;
/// Plain-text export of the indexing overlay for clipboard copy.
pub(crate) mod text;

use cp_render::Semantic;
use cp_render::overlay_ir::{
    SearchEmbeddings, SearchExtension, SearchIndex, SearchIndexOverlay, SearchRecentFile, SearchRecomputed,
    SearchServer, SearchSplitter, SearchTask,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::prelude::{Rect, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::state::State;
use crate::ui::theme;

// ── Builder ──────────────────────────────────────────────────────────

/// Build the search index overlay IR data from application state.
#[must_use]
pub(crate) fn build_search_index_overlay(state: &State) -> SearchIndexOverlay {
    let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
    let now_u64 = u64::try_from(now_ms).unwrap_or(u64::MAX);
    let flash_active =
        state.flags.overlays.copied_flash_ms > 0 && now_u64.saturating_sub(state.flags.overlays.copied_flash_ms) < 1500;
    let title = if flash_active { " ✓ Copied! ".into() } else { " Indexing Status ".into() };

    let Some(info) = cp_mod_search::overlay_info(state) else {
        return SearchIndexOverlay {
            title,
            footer: " Ctrl+C copy · Ctrl+I or Esc to dismiss ".into(),
            server: SearchServer {
                url: String::new(),
                online: false,
                version: String::new(),
                cpu_pct: None,
                cpu_semantic: Semantic::Muted,
                memory_display: None,
            },
            index: SearchIndex {
                files_indexed: 0,
                chunks_indexed: 0,
                queue_depth: 0,
                error_count: 0,
                index_ready: false,
                last_activity: "never".into(),
                disk_used: String::new(),
                disk_total: String::new(),
                docs_display: String::new(),
                avg_chunk: None,
            },
            extensions: Vec::new(),
            splitter: None,
            embeddings: None,
            recent_tasks: Vec::new(),
            top_recomputed: Vec::new(),
            recently_sent: Vec::new(),
        };
    };

    let server = build_server(&info);
    let index = build_index(&info);
    let extensions = build_extensions(&info);
    let splitter = build_splitter(&info);
    let embeddings = build_embeddings(&info);
    let recent_tasks = build_tasks(&info);
    let top_recomputed = build_recomputed(&info);
    let recently_sent = build_recently_sent(&info);

    SearchIndexOverlay {
        title,
        footer: " Ctrl+C copy · Ctrl+I or Esc to dismiss ".into(),
        server,
        index,
        extensions,
        splitter,
        embeddings,
        recent_tasks,
        top_recomputed,
        recently_sent,
    }
}

/// Build server status from overlay info.
fn build_server(info: &cp_mod_search::types::SearchOverlayInfo) -> SearchServer {
    let online = info.port > 0;
    let cpu_semantic = if info.meili_cpu_pct < 25.0 {
        Semantic::Success
    } else if info.meili_cpu_pct < 50.0 {
        Semantic::Warning
    } else {
        Semantic::Error
    };
    let has_process = info.meili_memory_bytes > 0 || info.meili_cpu_pct > 0.0;
    SearchServer {
        url: format!("http://127.0.0.1:{}", info.port),
        online,
        version: if info.meili_version.is_empty() { String::new() } else { format!("v{}", info.meili_version) },
        cpu_pct: has_process.then(|| f64::from(info.meili_cpu_pct)),
        cpu_semantic,
        memory_display: has_process.then(|| format_bytes(info.meili_memory_bytes)),
    }
}

/// Build core index statistics from overlay info.
fn build_index(info: &cp_mod_search::types::SearchOverlayInfo) -> SearchIndex {
    let last = if info.last_activity_ms > 0 { format_ago(info.last_activity_ms) } else { "never".to_owned() };
    SearchIndex {
        files_indexed: info.files_indexed,
        chunks_indexed: info.chunks_indexed,
        queue_depth: info.queue_depth,
        error_count: info.error_count,
        index_ready: info.index_ready,
        last_activity: last,
        disk_used: format_bytes(info.used_database_size_bytes),
        disk_total: format_bytes(info.database_size_bytes),
        docs_display: format_bytes(info.raw_document_db_size),
        avg_chunk: (info.avg_document_size > 0).then(|| format_bytes(info.avg_document_size)),
    }
}

/// Build extension breakdown from overlay info.
fn build_extensions(info: &cp_mod_search::types::SearchOverlayInfo) -> Vec<SearchExtension> {
    if info.top_extensions.is_empty() {
        return Vec::new();
    }
    let max_count = info.top_extensions.first().map_or(1, |e| e.1.max(1));
    let total_files: u64 = info.top_extensions.iter().map(|e| e.1).sum();
    let bar_max_width: u64 = 28;
    info.top_extensions
        .iter()
        .map(|(ext, count)| {
            let bar_len = count.saturating_mul(bar_max_width).checked_div(max_count).unwrap_or(0);
            let bar_usize = usize::try_from(bar_len).unwrap_or(0).max(1);
            let pct = if total_files > 0 { count.saturating_mul(100).checked_div(total_files).unwrap_or(0) } else { 0 };
            SearchExtension { name: ext.clone(), count: *count, bar_width: bar_usize, pct }
        })
        .collect()
}

/// Build splitter statistics from overlay info.
fn build_splitter(info: &cp_mod_search::types::SearchOverlayInfo) -> Option<SearchSplitter> {
    let total = info.tree_sitter_chunks.saturating_add(info.fallback_chunks);
    if total == 0 {
        return None;
    }
    let ts_pct = info.tree_sitter_chunks.saturating_mul(100).checked_div(total).unwrap_or(0);
    Some(SearchSplitter {
        tree_sitter_chunks: info.tree_sitter_chunks,
        tree_sitter_pct: ts_pct,
        fallback_chunks: info.fallback_chunks,
        fallback_pct: 100u64.saturating_sub(ts_pct),
    })
}

/// Build embedding statistics from overlay info.
fn build_embeddings(info: &cp_mod_search::types::SearchOverlayInfo) -> Option<SearchEmbeddings> {
    if info.embedding_model.is_empty() && info.files_embedding_count == 0 {
        return None;
    }
    let coverage_pct =
        info.files_embedded_doc_count.saturating_mul(100).checked_div(info.files_total_doc_count).unwrap_or(0);
    let coverage_semantic = if coverage_pct >= 100 { Semantic::Success } else { Semantic::Warning };
    Some(SearchEmbeddings {
        model: info.embedding_model.clone(),
        vector_count: info.files_embedding_count,
        is_indexing: info.files_is_indexing,
        embedded_docs: info.files_embedded_doc_count,
        total_docs: info.files_total_doc_count,
        coverage_pct,
        coverage_semantic,
        logs_doc_count: info.logs_doc_count,
    })
}

/// Build recent task entries from overlay info.
fn build_tasks(info: &cp_mod_search::types::SearchOverlayInfo) -> Vec<SearchTask> {
    info.recent_tasks
        .iter()
        .map(|t| {
            let status_semantic = match t.status.as_str() {
                "succeeded" => Semantic::Success,
                "failed" => Semantic::Error,
                "processing" => Semantic::Warning,
                _ => Semantic::Muted,
            };
            SearchTask {
                uid: t.uid,
                task_type: t.task_type.clone(),
                status: t.status.clone(),
                status_semantic,
                duration: t.duration.clone(),
            }
        })
        .collect()
}

/// Build top-recomputed file entries from overlay info.
fn build_recomputed(info: &cp_mod_search::types::SearchOverlayInfo) -> Vec<SearchRecomputed> {
    info.top_recomputed.iter().map(|(p, c)| SearchRecomputed { path: truncate_path(p, 46), count: *c }).collect()
}

/// Build recently-sent file entries from overlay info.
fn build_recently_sent(info: &cp_mod_search::types::SearchOverlayInfo) -> Vec<SearchRecentFile> {
    info.recently_sent
        .iter()
        .map(|(p, ts_ms)| {
            let ago = if *ts_ms > 0 { format_ago(*ts_ms) } else { "?".to_owned() };
            SearchRecentFile { path: truncate_path(p, 42), ago }
        })
        .collect()
}

// ── Adapter ──────────────────────────────────────────────────────────

/// Overlay width in terminal cells (two-column layout).
const OVERLAY_WIDTH: u16 = 120;

/// Render the search index overlay from IR data.
pub(crate) fn render_search_index_overlay(frame: &mut Frame<'_>, overlay: &SearchIndexOverlay, area: Rect) {
    let (left_lines, right_lines) = display::build_display_columns(overlay);

    let left_len = u16::try_from(left_lines.len().saturating_add(2)).unwrap_or(30);
    let right_len = u16::try_from(right_lines.len().saturating_add(2)).unwrap_or(30);
    let height = left_len.max(right_len).min(area.height);
    let popup = centered_rect(OVERLAY_WIDTH, height, area);

    let block = Block::default()
        .title(overlay.title.as_str())
        .title_bottom(overlay.footer.as_str())
        .borders(Borders::ALL)
        .style(Style::default().bg(theme::bg_base()).fg(theme::text()));

    let inner = block.inner(popup);
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);

    let columns = Layout::horizontal([Constraint::Fill(1), Constraint::Length(1), Constraint::Fill(1)]).split(inner);

    let left_para = Paragraph::new(left_lines);
    let right_para = Paragraph::new(right_lines);

    let sep_height = usize::from(inner.height);
    let sep_lines: Vec<Line<'_>> =
        std::iter::repeat_with(|| Line::from(Span::styled("│", Style::default().fg(theme::text_muted()))))
            .take(sep_height)
            .collect();
    let sep_para = Paragraph::new(sep_lines);

    let (Some(&left_col), Some(&sep_col), Some(&right_col)) = (columns.first(), columns.get(1), columns.get(2)) else {
        debug_assert!(false, "overlay column layout must have 3 chunks");
        return;
    };

    frame.render_widget(left_para, left_col);
    frame.render_widget(sep_para, sep_col);
    frame.render_widget(right_para, right_col);
}

// ── Helpers ──────────────────────────────────────────────────────────
/// Compute a centered rectangle within the given area.
fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let eff_w = width.min(area.width);
    let eff_h = height.min(area.height);
    let x_off = area.width.saturating_sub(eff_w).checked_div(2).unwrap_or(0);
    let y_off = area.height.saturating_sub(eff_h).checked_div(2).unwrap_or(0);
    Rect::new(area.x.saturating_add(x_off), area.y.saturating_add(y_off), eff_w, eff_h)
}

/// Format a millisecond timestamp as a relative "X ago" string.
pub(crate) fn format_ago(ms_then: u64) -> String {
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

/// Truncate a file path to fit within `max_len` characters.
fn truncate_path(path: &str, max_len: usize) -> String {
    if path.len() <= max_len {
        return path.to_owned();
    }
    let start = path.len().saturating_sub(max_len.saturating_sub(1));
    format!("…{}", path.get(start..).unwrap_or(path))
}

/// Format a byte count as a human-readable string (e.g. "215 MB").
pub(crate) fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * 1024 * 1024;

    if bytes >= GB {
        let whole = bytes.checked_div(GB).unwrap_or(0);
        let frac = bytes.wrapping_rem(GB).saturating_mul(10).checked_div(GB).unwrap_or(0);
        format!("{whole}.{frac} GB")
    } else if bytes >= MB {
        format!("{} MB", bytes.checked_div(MB).unwrap_or(0))
    } else if bytes >= KB {
        format!("{} KB", bytes.checked_div(KB).unwrap_or(0))
    } else {
        format!("{bytes} B")
    }
}
