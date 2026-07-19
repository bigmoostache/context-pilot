//! Plain-text export of the Meilisearch indexing status overlay.
//!
//! Produces a clipboard-friendly text version from the [`SearchIndexOverlay`]
//! IR data. Used by the `CopyIndexOverlay` action (Ctrl+C while the overlay
//! is open).

use cp_render::overlay_ir::SearchIndexOverlay;

/// Write the server + process header.
fn write_server(out: &mut String, overlay: &SearchIndexOverlay) {
    use std::fmt::Write as _;
    let status = if overlay.server.online { "online" } else { "offline" };
    let version =
        if overlay.server.version.is_empty() { String::new() } else { format!("  {}", overlay.server.version) };
    writeln!(out, "Indexing Status\n\nServer  {}  {status}{version}\n", overlay.server.url).unwrap_or(());

    if let (Some(cpu), Some(mem)) = (overlay.server.cpu_pct, overlay.server.memory_display.as_ref()) {
        writeln!(out, "Process CPU {cpu:.1}%    RAM {mem}\n").unwrap_or(());
    }
}

/// Write the database + core index stats block.
fn write_index_stats(out: &mut String, overlay: &SearchIndexOverlay) {
    use std::fmt::Write as _;
    if !overlay.index.disk_total.is_empty() && overlay.index.disk_total != "0 B" {
        writeln!(
            out,
            "── Database ──\nDisk  {} / {}    Docs  {}",
            overlay.index.disk_used, overlay.index.disk_total, overlay.index.docs_display,
        )
        .unwrap_or(());
        if let Some(avg) = &(overlay.index.avg_chunk) {
            writeln!(out, "Avg chunk  {avg}").unwrap_or(());
        }
        out.push('\n');
    }

    let ready = if overlay.index.index_ready { "Ready" } else { "Scanning" };
    writeln!(
        out,
        "Files  {}    Chunks  {}\nQueue  {} pending    Errors  {}\nStatus {ready}    Last  {}",
        overlay.index.files_indexed,
        overlay.index.chunks_indexed,
        overlay.index.queue_depth,
        overlay.index.error_count,
        overlay.index.last_activity,
    )
    .unwrap_or(());
}

/// Write extensions + splitter breakdown.
fn write_extensions_splitter(out: &mut String, overlay: &SearchIndexOverlay) {
    use std::fmt::Write as _;
    if !overlay.extensions.is_empty() {
        out.push_str("\n── Extensions ──\n");
        for ext in &overlay.extensions {
            writeln!(out, "  {:<6} {:>4}  {}%", ext.name, ext.count, ext.pct).unwrap_or(());
        }
    }

    if let Some(sp) = &(overlay.splitter) {
        writeln!(
            out,
            "\n── Splitter ──\nTree-sitter  {} chunks ({}%)\nFallback     {} chunks ({}%)",
            sp.tree_sitter_chunks, sp.tree_sitter_pct, sp.fallback_chunks, sp.fallback_pct,
        )
        .unwrap_or(());
    }
}

/// Write the embeddings block.
fn write_embeddings(out: &mut String, overlay: &SearchIndexOverlay) {
    use std::fmt::Write as _;
    let Some(emb) = &(overlay.embeddings) else {
        return;
    };
    out.push_str("\n── Embeddings ──\n");
    if !emb.model.is_empty() {
        writeln!(out, "Model   {}", emb.model).unwrap_or(());
    }
    let emb_status = if emb.is_indexing { "generating" } else { "ready" };
    writeln!(out, "Vectors {}  {emb_status}", emb.vector_count).unwrap_or(());
    if emb.total_docs > 0 {
        writeln!(out, "Coverage {}/{}  ({}%)", emb.embedded_docs, emb.total_docs, emb.coverage_pct).unwrap_or(());
    }
    if emb.logs_doc_count > 0 {
        writeln!(out, "Logs    {} documents", emb.logs_doc_count).unwrap_or(());
    }
}

/// Write the tasks / recomputed / recently-sent trailers.
fn write_task_trailers(out: &mut String, overlay: &SearchIndexOverlay) {
    use std::fmt::Write as _;
    if !overlay.recent_tasks.is_empty() {
        out.push_str("\n── Recent Tasks ──\n");
        for task in &overlay.recent_tasks {
            writeln!(out, "  #{:<6} {:<10} {:<10} {}", task.uid, task.task_type, task.status, task.duration)
                .unwrap_or(());
        }
    }

    if !overlay.top_recomputed.is_empty() {
        out.push_str("\n── Top Recomputed ──\n");
        for entry in &overlay.top_recomputed {
            writeln!(out, "  {:>4}×  {}", entry.count, entry.path).unwrap_or(());
        }
    }

    if !overlay.recently_sent.is_empty() {
        out.push_str("\n── Recently Sent ──\n");
        for entry in &overlay.recently_sent {
            writeln!(out, "  {:>8}  {}", entry.ago, entry.path).unwrap_or(());
        }
    }
}

/// Build the overlay content as plain text for clipboard copying.
///
/// Consumes the pre-built IR data so both the TUI overlay and the
/// text export share the same data source.
pub(crate) fn build_overlay_text(overlay: &SearchIndexOverlay) -> String {
    let mut out = String::with_capacity(512);
    write_server(&mut out, overlay);
    write_index_stats(&mut out, overlay);
    write_extensions_splitter(&mut out, overlay);
    write_embeddings(&mut out, overlay);
    write_task_trailers(&mut out, overlay);
    out
}
