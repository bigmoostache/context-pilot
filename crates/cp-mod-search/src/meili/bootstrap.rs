//! Init-time helpers: index creation, metrics population, project hashing.
//!
//! Extracted from `lib.rs` to keep the module trait implementation focused.
//! Called during `init_state` / `load_module_data` — not on the hot path.

use super::api;
use crate::types;

/// Compute an 8-character hex hash of a path for per-project index naming.
pub(crate) fn hash_project_path(path: &str) -> String {
    let hex = cp_mod_utilities::hash::compute_str(path);
    hex.get(..8).unwrap_or(&hex).to_owned()
}

/// Create per-project Meilisearch indexes if they don't already exist.
///
/// Creates `cp_{hash}_files` and `cp_{hash}_logs` indexes with appropriate
/// settings (searchable, filterable, sortable attributes).
///
/// # Errors
///
/// Returns an error if any API call fails.
pub(crate) fn ensure_indexes(port: u16, master_key: &str, project_hash: &str) -> Result<(), String> {
    let meili = api::MeiliClient::new(port, master_key)?;

    let files_uid = format!("cp_{project_hash}_files");
    let logs_uid = format!("cp_{project_hash}_logs");

    // Files index
    if !meili.index_exists(&files_uid)? {
        let create_task = meili.create_index(&files_uid, "id")?;
        super::tasks::wait_for_task(&meili, create_task)?;
        let settings_task = meili.update_settings(&files_uid, &types::files_index_settings())?;
        super::tasks::wait_for_task(&meili, settings_task)?;
        log::info!("Created files index: {files_uid}");
    }

    // Logs index
    if !meili.index_exists(&logs_uid)? {
        let create_task = meili.create_index(&logs_uid, "id")?;
        super::tasks::wait_for_task(&meili, create_task)?;
        let settings_task = meili.update_settings(&logs_uid, &types::logs_index_settings())?;
        super::tasks::wait_for_task(&meili, settings_task)?;
        log::info!("Created logs index: {logs_uid}");
    }

    // Configure embedders for hybrid search (idempotent — only if not already set)
    configure_embedders(&meili, &files_uid, &logs_uid);

    Ok(())
}

/// Parsed facet breakdown: per-extension chunk counts + tree-sitter/fallback split.
struct FacetCounts {
    /// Per-extension chunk counts from the `extension` facet.
    extension_counts: std::collections::HashMap<String, u64>,
    /// Chunks produced by a tree-sitter grammar.
    tree_sitter_chunks: u64,
    /// Chunks produced by the fixed-size fallback splitter (`chunk_type == "raw"`).
    fallback_chunks: u64,
}

/// Fold the `extension` facet map (`{ "rs": 3000, ... }`) into per-extension
/// chunk counts.
fn fold_extension_facet(facets: &serde_json::Value, out: &mut std::collections::HashMap<String, u64>) {
    let Some(ext_map) = facets.get("extension").and_then(serde_json::Value::as_object) else {
        return;
    };
    for (ext, count) in ext_map {
        if let Some(n) = count.as_u64() {
            let _prev = out.insert(ext.clone(), n);
        }
    }
}

/// Fold the `chunk_type` facet map into the tree-sitter/fallback split
/// (`raw` == fallback splitter, anything else == a tree-sitter grammar).
fn fold_chunk_type_facet(facets: &serde_json::Value, tree_sitter: &mut u64, fallback: &mut u64) {
    let Some(ct_map) = facets.get("chunk_type").and_then(serde_json::Value::as_object) else {
        return;
    };
    for (chunk_type, count) in ct_map {
        let Some(n) = count.as_u64() else { continue };
        if chunk_type == "raw" {
            *fallback = fallback.saturating_add(n);
        } else {
            *tree_sitter = tree_sitter.saturating_add(n);
        }
    }
}

/// Query the `extension` + `chunk_type` facet distributions and fold them into
/// per-extension counts and the tree-sitter/fallback split.
fn query_facet_counts(meili: &api::MeiliClient, files_uid: &str) -> FacetCounts {
    let mut extension_counts: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    let mut tree_sitter_chunks: u64 = 0;
    let mut fallback_chunks: u64 = 0;

    if let Ok(facets) = meili.facet_distribution(files_uid, &["extension", "chunk_type"]) {
        fold_extension_facet(&facets, &mut extension_counts);
        fold_chunk_type_facet(&facets, &mut tree_sitter_chunks, &mut fallback_chunks);
    }
    FacetCounts { extension_counts, tree_sitter_chunks, fallback_chunks }
}

/// Convert per-extension CHUNK counts to approximate per-extension FILE counts
/// using the overall `files / total_chunks` ratio. Passes counts through
/// unchanged when the ratio is undefined (no files or no chunks).
fn derive_file_ext_counts(
    extension_counts: std::collections::HashMap<String, u64>,
    files: u64,
) -> std::collections::HashMap<String, u64> {
    let total_ext_chunks: u64 = extension_counts.values().sum();
    if total_ext_chunks == 0 || files == 0 {
        return extension_counts;
    }
    extension_counts
        .iter()
        .map(|(ext, &chunk_count)| {
            let file_count = chunk_count
                .saturating_mul(files)
                .checked_div(total_ext_chunks)
                .unwrap_or(0)
                .max(u64::from(chunk_count > 0));
            (ext.clone(), file_count)
        })
        .collect()
}

/// Query Meilisearch for initial index statistics and populate metrics.
///
/// Called once during `init_state` / `load_module_data` so the Ctrl+I overlay
/// shows correct counts immediately (before the indexer has done any work).
/// Queries both basic stats (doc count) and facet distributions (extension
/// breakdown, chunk type split).
pub(crate) fn populate_initial_metrics(
    port: u16,
    master_key: &str,
    project_hash: &str,
    metrics: &std::sync::Arc<std::sync::Mutex<types::SearchMetrics>>,
) {
    let Ok(meili) = api::MeiliClient::new(port, master_key) else {
        return;
    };

    let files_uid = format!("cp_{project_hash}_files");
    let logs_uid = format!("cp_{project_hash}_logs");

    let (mut chunks, files) = if let Ok((count, _indexing)) = meili.index_stats(&files_uid) {
        let f = count.checked_div(3).unwrap_or(0).max(u64::from(count > 0));
        (count, f)
    } else {
        (0, 0)
    };

    // Also count logs (optional — just for awareness)
    if let Ok((log_count, _)) = meili.index_stats(&logs_uid) {
        chunks = chunks.saturating_add(log_count);
    }

    let facets = query_facet_counts(&meili, &files_uid);

    // Derive file count from extension counts (more accurate than chunk/3 estimate).
    let file_ext_counts = derive_file_ext_counts(facets.extension_counts, files);

    if let Ok(mut m) = metrics.lock() {
        m.chunks_indexed = chunks;
        m.files_indexed = files;
        m.extension_counts = file_ext_counts;
        m.tree_sitter_chunks = facets.tree_sitter_chunks;
        m.fallback_chunks = facets.fallback_chunks;
    }
}

// -- Embedder configuration --------------------------------------------------

/// Voyage AI API endpoint for embeddings.
const VOYAGE_URL: &str = "https://api.voyageai.com/v1/embeddings";

/// Voyage AI model optimized for code search.
///
/// voyage-code-3: 1024 dimensions, 32K context window, optimized for code
/// retrieval and semantic search across source files.
const VOYAGE_MODEL: &str = "voyage-code-3";

/// Configure the embedder on one index, skipping if already present.
/// `settings` is the index-specific embedder settings JSON.
fn configure_one_embedder(meili: &api::MeiliClient, uid: &str, settings: &serde_json::Value) {
    let has_embedder =
        meili.get_embedder_settings(uid).ok().and_then(|v| v.as_object().map(|m| !m.is_empty())).unwrap_or(false);
    if has_embedder {
        return;
    }
    match meili.update_embedder_settings(uid, settings) {
        Ok(task_uid) => log::info!("Configuring Voyage embedder for {uid} (task {task_uid})"),
        Err(e) => log::warn!("Failed to configure embedder for {uid}: {e}"),
    }
}

/// Configure embedders on the files and logs indexes if not already set.
///
/// Uses the Voyage AI REST API for embeddings — zero local CPU usage.
/// Requires the `voyage` key in the credential vault. If missing,
/// embedders are skipped and search falls back to keyword-only mode.
///
/// This is a fire-and-forget operation: Meilisearch will call the Voyage API
/// in the background to generate embeddings for all documents.
fn configure_embedders(meili: &api::MeiliClient, files_uid: &str, logs_uid: &str) {
    let Some(api_key) = read_voyage_api_key() else {
        log::info!("Voyage API key not configured \u{2014} skipping embedder setup (keyword-only search)");
        return;
    };

    configure_one_embedder(meili, files_uid, &files_embedder_settings(&api_key));
    configure_one_embedder(meili, logs_uid, &logs_embedder_settings(&api_key));
}

/// Read the Voyage AI API key from the credential vault.
///
/// Returns `None` if not configured. Embedders are skipped and search
/// falls back to keyword-only mode.
fn read_voyage_api_key() -> Option<String> {
    cp_vault::vault().get("voyage").map(|s| s.expose().to_owned())
}

/// Embedder settings for the files index.
///
/// Uses Voyage AI REST API with `voyage-code-3` model. The document template
/// combines file path, chunk type/name, and content into a rich embedding
/// input that captures WHERE and WHAT the code is.
fn files_embedder_settings(api_key: &str) -> serde_json::Value {
    serde_json::json!({
        "default": {
            "source": "rest",
            "url": VOYAGE_URL,
            "apiKey": api_key,
            "request": {
                "model": VOYAGE_MODEL,
                "input": ["{{text}}", "{{..}}"]
            },
            "response": {
                "data": [
                    { "embedding": "{{embedding}}" },
                    "{{..}}"
                ]
            },
            "documentTemplate": "{{doc.file_path}} [{{doc.chunk_type}}] {{doc.chunk_name}}: {{doc.content | truncatewords: 100}}",
            "documentTemplateMaxBytes": 1024
        }
    })
}

/// Embedder settings for the logs index.
///
/// Simpler template since logs are short free-text entries.
fn logs_embedder_settings(api_key: &str) -> serde_json::Value {
    serde_json::json!({
        "default": {
            "source": "rest",
            "url": VOYAGE_URL,
            "apiKey": api_key,
            "request": {
                "model": VOYAGE_MODEL,
                "input": ["{{text}}", "{{..}}"]
            },
            "response": {
                "data": [
                    { "embedding": "{{embedding}}" },
                    "{{..}}"
                ]
            },
            "documentTemplate": "[{{doc.importance}}] {{doc.content}}"
        }
    })
}
