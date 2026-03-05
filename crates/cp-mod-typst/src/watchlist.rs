//! Typst watchlist: tracks which .typ documents to auto-compile and their dependency trees.
//!
//! Each watched document has a .deps.json manifest listing all files accessed during
//! compilation. When any dependency changes, the document is recompiled and the manifest
//! is refreshed.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Persisted watchlist state — which .typ documents are watched + their dependency manifests.
/// Stored at `.context-pilot/shared/typst-watchlist.json`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Watchlist {
    /// Map from source .typ path (relative to project root) → watch entry
    pub entries: HashMap<String, WatchEntry>,
}

/// A single watched document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchEntry {
    /// Output PDF path (relative to project root)
    pub output: String,
    /// All files this document depends on (relative to project root).
    /// Includes the source itself, all #import'd .typ files, images, bib files, toml, etc.
    pub deps: Vec<String>,
}

/// Disk path for the persisted watchlist JSON file.
const WATCHLIST_PATH: &str = ".context-pilot/shared/typst-watchlist.json";

impl Watchlist {
    /// Load from disk, or return empty if not found.
    #[must_use]
    pub fn load() -> Self {
        fs::read_to_string(WATCHLIST_PATH).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    /// Save to disk.
    pub fn save(&self) {
        if let Some(parent) = Path::new(WATCHLIST_PATH).parent() {
            let _r = fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _r = fs::write(WATCHLIST_PATH, json);
        }
    }

    /// Add or update a watched document with its dependency list.
    pub fn watch(&mut self, source: &str, output: &str, deps: Vec<String>) {
        drop(self.entries.insert(source.to_string(), WatchEntry { output: output.to_string(), deps }));
        self.save();
    }

    /// Remove a document from the watchlist.
    pub fn unwatch(&mut self, source: &str) -> bool {
        let removed = self.entries.remove(source).is_some();
        if removed {
            self.save();
        }
        removed
    }

    /// Given a changed file path (relative to project root), return all watched
    /// documents that depend on it (source path, output path).
    #[must_use]
    pub fn find_affected(&self, changed_file: &str) -> Vec<(String, String)> {
        let changed = normalize_path(changed_file);
        self.entries
            .iter()
            .filter(|(source, entry)| {
                // Check if the changed file IS the source or is in its deps
                normalize_path(source) == changed || entry.deps.iter().any(|d| normalize_path(d) == changed)
            })
            .map(|(source, entry)| (source.clone(), entry.output.clone()))
            .collect()
    }

    /// List all watched documents.
    #[must_use]
    pub fn list(&self) -> Vec<(&str, &WatchEntry)> {
        let mut items: Vec<_> = self.entries.iter().map(|(k, v)| (k.as_str(), v)).collect();
        items.sort_by_key(|(k, _)| k.to_string());
        items
    }
}

/// Normalize a path for comparison (resolve ../, remove trailing slashes).
fn normalize_path(p: &str) -> String {
    // Use canonicalize if the file exists, otherwise just clean up the string
    PathBuf::from(p)
        .canonicalize()
        .map_or_else(|_| PathBuf::from(p).to_string_lossy().to_string(), |abs| abs.to_string_lossy().to_string())
}

/// Compile a watched document and update its dependency manifest.
/// Returns `Ok(message)` or `Err(error)`.
///
/// # Errors
///
/// Returns `Err` if compilation fails, or the output PDF cannot be written.
pub fn compile_and_update_deps(source: &str, output: &str) -> Result<String, String> {
    let (pdf_bytes, warnings, deps) = crate::compiler::compile_to_pdf(source)?;

    // Write PDF
    if let Some(parent) = Path::new(output).parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {}", parent.display(), e))?;
    }
    fs::write(output, &pdf_bytes).map_err(|e| format!("write {output}: {e}"))?;

    // Convert absolute dep paths to relative (to project root = cwd)
    let cwd = std::env::current_dir().unwrap_or_default();
    let rel_deps: Vec<String> = deps
        .iter()
        .filter_map(|abs_path| abs_path.strip_prefix(&cwd).ok().map(|rel| rel.to_string_lossy().to_string()))
        .collect();

    // Update watchlist with fresh deps
    let mut watchlist = Watchlist::load();
    watchlist.watch(source, output, rel_deps.clone());

    let mut msg = format!("✓ {} → {} ({} bytes, {} deps)", source, output, pdf_bytes.len(), rel_deps.len());
    if !warnings.is_empty() {
        msg.push('\n');
        msg.push_str(&warnings);
    }
    Ok(msg)
}
