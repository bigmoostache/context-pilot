//! Incarnation-agnostic embedding backup.
//!
//! The Voyage vectors live in the GLOBAL Meilisearch data dir, outside the
//! project folder, so a cross-machine copy of the agent folder lands on an
//! empty index and re-embeds everything (Voyage cost). This module carries the
//! vectors *inside* the folder: it exports `{ document id, fields, vector }`
//! rows to `./.context-pilot/embeddings/` and, on a fresh boot where the
//! locally-computed index is empty, reimports them with `regenerate: false` so
//! Meilisearch stores the vectors without a single Voyage call.
//!
//! The backup stores no index uid — only project-relative document ids (see the
//! reconcile module) and vectors — so it restores into whatever uid the
//! destination computes locally, at any path, on any machine.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use cp_base::config::constants;

use crate::meili::api::MeiliClient;
use crate::meili::tasks;

/// Embedder name configured on both indexes (see `meili::bootstrap`).
const EMBEDDER: &str = "default";

/// Reimport batch size (documents per `add_documents` call).
const BATCH: usize = 500;

/// Manifest describing a backup: the freshness fingerprint plus per-index
/// counts used to reject a half-written or stale set on reimport.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub(crate) struct Manifest {
    /// `hash(model + embedder_name + template)` — a mismatch means the backup's
    /// vectors were produced by a different embedder and must NOT be reused.
    pub fingerprint: String,
    /// Embedder name the `_vectors` map is keyed by.
    pub embedder_name: String,
    /// Number of rows in `files.jsonl` (validated on reimport).
    pub count_files: u64,
    /// Number of rows in `logs.jsonl` (validated on reimport).
    pub count_logs: u64,
}

/// `./.context-pilot/embeddings/` — travels with the agent folder.
fn embeddings_dir() -> PathBuf {
    PathBuf::from(constants::STORE_DIR).join("embeddings")
}

/// Runtime freshness fingerprint from the live embedder settings.
///
/// `hash(model + embedder_name + documentTemplate)`, read from
/// `get_embedder_settings` — not a hand-maintained version int, so any change
/// to the model, embedder, or template auto-invalidates every backup. Returns
/// `None` when no embedder is configured (keyword-only mode — nothing to back
/// up or restore).
pub(crate) fn fingerprint(client: &MeiliClient, files_uid: &str) -> Option<String> {
    let settings = client.get_embedder_settings(files_uid).ok()?;
    let d = settings.get(EMBEDDER)?;
    let model = d.get("request").and_then(|r| r.get("model")).and_then(serde_json::Value::as_str).unwrap_or("");
    let template = d.get("documentTemplate").and_then(serde_json::Value::as_str).unwrap_or("");
    if model.is_empty() {
        return None;
    }
    Some(cp_mod_utilities::hash::compute_str(&format!("{model}\n{EMBEDDER}\n{template}")))
}

/// Whether a backup may warm this index. Pure so it is trivially testable.
///
/// The backup is a cold-start warmer ONLY: applied when the locally computed
/// index is empty AND a backup is present AND its fingerprint matches the live
/// embedder. A populated index (ordinary restart) is never touched —
/// reconciliation handles drift instead — so the backup can only ever speed up
/// an empty index, never corrupt a populated one.
pub(crate) fn should_reimport(index_count: u64, backup_present: bool, manifest_fp: &str, current_fp: &str) -> bool {
    index_count == 0 && backup_present && manifest_fp == current_fp
}

// -- Export ------------------------------------------------------------------

/// Atomically write JSONL rows: `*.tmp` + fsync + rename, so a crash mid-write
/// leaves any previous file intact.
fn write_jsonl_atomic(path: &Path, rows: &[serde_json::Value]) -> Result<(), String> {
    let tmp_path = path.with_extension("jsonl.tmp");
    let mut f = std::fs::File::create(&tmp_path).map_err(|e| format!("create {}: {e}", tmp_path.display()))?;
    for row in rows {
        let line = serde_json::to_string(row).map_err(|e| format!("serialize row: {e}"))?;
        f.write_all(line.as_bytes()).map_err(|e| format!("write row: {e}"))?;
        f.write_all(b"\n").map_err(|e| format!("write newline: {e}"))?;
    }
    f.sync_all().map_err(|e| format!("fsync {}: {e}", tmp_path.display()))?;
    std::fs::rename(&tmp_path, path).map_err(|e| format!("rename {}: {e}", tmp_path.display()))
}

/// Atomically write the manifest LAST, after both jsonl files are committed, so
/// a manifest on disk always describes a complete backup.
fn write_manifest_atomic(dir: &Path, manifest: &Manifest) -> Result<(), String> {
    let path = dir.join("manifest.json");
    let tmp_path = dir.join("manifest.json.tmp");
    let body = serde_json::to_string_pretty(manifest).map_err(|e| format!("serialize manifest: {e}"))?;
    let mut f = std::fs::File::create(&tmp_path).map_err(|e| format!("create manifest tmp: {e}"))?;
    f.write_all(body.as_bytes()).map_err(|e| format!("write manifest: {e}"))?;
    f.sync_all().map_err(|e| format!("fsync manifest: {e}"))?;
    std::fs::rename(&tmp_path, &path).map_err(|e| format!("rename manifest: {e}"))
}

/// Export both indexes' documents (with vectors) + a manifest into the in-folder
/// embeddings dir. Overwrites the previous backup — a single rolling snapshot.
///
/// # Errors
///
/// Returns an error if the embedder is unconfigured, a fetch fails, or a write
/// fails.
pub(crate) fn export_backup(client: &MeiliClient, files_uid: &str, logs_uid: &str) -> Result<(), String> {
    let Some(fp) = fingerprint(client, files_uid) else {
        return Err("no embedder configured — nothing to back up".to_string());
    };

    let files_rows = tasks::fetch_all_with_vectors(client, files_uid)?;
    let logs_rows = tasks::fetch_all_with_vectors(client, logs_uid)?;

    let dir = embeddings_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("create embeddings dir: {e}"))?;

    write_jsonl_atomic(&dir.join("files.jsonl"), &files_rows)?;
    write_jsonl_atomic(&dir.join("logs.jsonl"), &logs_rows)?;

    let manifest = Manifest {
        fingerprint: fp,
        embedder_name: EMBEDDER.to_string(),
        count_files: u64::try_from(files_rows.len()).unwrap_or(u64::MAX),
        count_logs: u64::try_from(logs_rows.len()).unwrap_or(u64::MAX),
    };
    write_manifest_atomic(&dir, &manifest)
}

// -- Reimport ----------------------------------------------------------------

/// Read the manifest, if present and parseable.
fn read_manifest(dir: &Path) -> Option<Manifest> {
    let body = std::fs::read_to_string(dir.join("manifest.json")).ok()?;
    serde_json::from_str(&body).ok()
}

/// Read JSONL rows, forcing `_vectors.<embedder>.regenerate = false` on each so
/// Meilisearch stores the vector verbatim instead of re-embedding.
fn read_jsonl_for_reimport(path: &Path) -> Result<Vec<serde_json::Value>, String> {
    let body = std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut rows: Vec<serde_json::Value> = Vec::new();
    for line in body.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let mut row: serde_json::Value = serde_json::from_str(line).map_err(|e| format!("parse jsonl row: {e}"))?;
        force_regenerate_false(&mut row);
        rows.push(row);
    }
    Ok(rows)
}

/// Set `_vectors.<embedder>.regenerate = false` on a document so the stored
/// vector is kept as-is (zero Voyage). No-op if the doc carries no vector.
fn force_regenerate_false(row: &mut serde_json::Value) {
    let Some(vectors) = row.get_mut("_vectors").and_then(serde_json::Value::as_object_mut) else {
        return;
    };
    let Some(entry) = vectors.get_mut(EMBEDDER).and_then(serde_json::Value::as_object_mut) else {
        return;
    };
    let _prev = entry.insert("regenerate".to_string(), serde_json::Value::Bool(false));
}

/// Reimport one index's rows in batches, waiting for each batch. Returns the
/// number of documents restored.
fn apply_reimport(client: &MeiliClient, uid: &str, jsonl_path: &Path) -> Result<usize, String> {
    let rows = read_jsonl_for_reimport(jsonl_path)?;
    let total = rows.len();
    for batch in rows.chunks(BATCH) {
        let docs = serde_json::Value::Array(batch.to_vec());
        let task = client.add_documents(uid, &docs)?;
        tasks::wait_for_task(client, task)?;
    }
    Ok(total)
}

/// Count non-empty lines in a jsonl body.
fn count_nonempty_lines(body: &str) -> u64 {
    u64::try_from(body.lines().filter(|l| !l.trim().is_empty()).count()).unwrap_or(u64::MAX)
}

/// One index's reimport target: `(uid, jsonl path, manifest count, label)`.
type ReimportTarget<'plan> = (&'plan str, &'plan Path, u64, &'plan str);

/// Boot-time reimport-on-empty for both indexes.
///
/// For each index: if it is empty, a backup is present, its row count matches
/// the manifest, and the fingerprint matches the live embedder ([`should_reimport`]),
/// restore the vectors (zero Voyage). Any mismatch/absence is silently skipped —
/// normal indexing (via reconcile) then handles the index from scratch.
pub(crate) fn maybe_reimport(client: &MeiliClient, files_uid: &str, logs_uid: &str) {
    let Some(current_fp) = fingerprint(client, files_uid) else {
        return; // keyword-only mode — no vectors to restore
    };
    let dir = embeddings_dir();
    let Some(manifest) = read_manifest(&dir) else {
        return; // no (complete) backup
    };

    // (index uid, jsonl path, manifest count, label) for both indexes.
    let files_jsonl = dir.join("files.jsonl");
    let logs_jsonl = dir.join("logs.jsonl");
    let plans: [ReimportTarget<'_>; 2] = [
        (files_uid, files_jsonl.as_path(), manifest.count_files, "files"),
        (logs_uid, logs_jsonl.as_path(), manifest.count_logs, "logs"),
    ];

    for (uid, jsonl, expected, label) in plans {
        let present = jsonl.is_file();
        let count = client.index_stats(uid).map_or(0, |(c, _)| c);

        // Count-validate: a half-written jsonl (line count != manifest) is rejected.
        if present {
            let actual = std::fs::read_to_string(jsonl).map_or(0, |b| count_nonempty_lines(&b));
            if actual != expected {
                log::warn!("Backup {label}.jsonl has {actual} rows, manifest says {expected} — skipping reimport");
                continue;
            }
        }

        if !should_reimport(count, present, &manifest.fingerprint, &current_fp) {
            continue;
        }
        match apply_reimport(client, uid, jsonl) {
            Ok(n) => log::info!("Reimported {n} {label} documents from backup (zero Voyage)"),
            Err(e) => log::warn!("Backup reimport for {label} failed: {e}"),
        }
    }
}

#[cfg(test)]
#[expect(clippy::panic, reason = "backup tests panic via let-else for fs/serde setup failure messages")]
mod tests {
    use super::*;

    /// `_vectors.default.regenerate` as an `Option<&Value>` — pointer access
    /// dodges the panicking `Index` impl (clippy `indexing_slicing`).
    fn regen_flag(row: &serde_json::Value) -> Option<&serde_json::Value> {
        row.pointer("/_vectors/default/regenerate")
    }

    #[test]
    fn manifest_roundtrip() {
        let m = Manifest {
            fingerprint: "abc123".to_string(),
            embedder_name: "default".to_string(),
            count_files: 42,
            count_logs: 7,
        };
        let Ok(body) = serde_json::to_string(&m) else { panic!("serialize manifest") };
        let Ok(back) = serde_json::from_str::<Manifest>(&body) else { panic!("deserialize manifest") };
        assert_eq!(m, back);
    }

    #[test]
    fn should_reimport_only_when_empty_present_and_matching() {
        assert!(should_reimport(0, true, "fp", "fp"));
        assert!(!should_reimport(5, true, "fp", "fp"), "populated index never reimported");
        assert!(!should_reimport(0, false, "fp", "fp"), "no backup present");
        assert!(!should_reimport(0, true, "old", "new"), "fingerprint mismatch rejected");
    }

    #[test]
    fn force_regenerate_false_sets_flag() {
        let mut row = serde_json::json!({
            "id": "x",
            "_vectors": { "default": { "embeddings": [0.1, 0.2], "regenerate": true } }
        });
        force_regenerate_false(&mut row);
        assert_eq!(regen_flag(&row), Some(&serde_json::Value::Bool(false)));
    }

    #[test]
    fn force_regenerate_false_noop_without_vectors() {
        let mut row = serde_json::json!({ "id": "x" });
        force_regenerate_false(&mut row);
        assert!(row.get("_vectors").is_none());
    }

    #[test]
    fn count_nonempty_lines_ignores_blanks() {
        assert_eq!(count_nonempty_lines("a\n\nb\n  \nc"), 3);
    }

    #[test]
    fn jsonl_write_then_read_roundtrip() {
        let mut dir = std::env::temp_dir();
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        dir.push(format!("cp-search-backup-{nanos}"));
        let Ok(()) = std::fs::create_dir_all(&dir) else { panic!("mkdir temp dir") };
        let path = dir.join("files.jsonl");
        let rows = vec![
            serde_json::json!({ "id": "a-0", "_vectors": { "default": { "embeddings": [1.0], "regenerate": true } } }),
            serde_json::json!({ "id": "b-0" }),
        ];
        let Ok(()) = write_jsonl_atomic(&path, &rows) else { panic!("write jsonl") };
        let Ok(read) = read_jsonl_for_reimport(&path) else { panic!("read jsonl") };
        assert_eq!(read.len(), 2);
        assert_eq!(read.first().and_then(regen_flag), Some(&serde_json::Value::Bool(false)));
        assert_eq!(read.get(1).and_then(|r| r.get("id")), Some(&serde_json::Value::String("b-0".to_string())));
        drop(std::fs::remove_dir_all(&dir));
    }
}
