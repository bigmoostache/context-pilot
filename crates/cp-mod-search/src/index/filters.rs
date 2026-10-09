//! File indexability gates: extension allowlist, directory/suffix exclusions,
//! size cap, and the shared `is_indexable` predicate used by both the live
//! indexer and the boot/hourly reconcile disk-walk.

// -- Configuration constants -------------------------------------------------

/// Maximum file size in bytes (1 MB).
///
/// Files larger than this are skipped during indexing to avoid
/// overwhelming the search index with very large generated files.
pub(crate) const MAX_FILE_SIZE: u64 = 0x0010_0000;

/// Default chunk size in characters for the fixed-size fallback splitter.
pub(crate) const FALLBACK_CHUNK_SIZE: usize = 4000;

/// Extensions that are eligible for indexing (code, config, docs, web, build).
///
/// Returns `true` if the extension is in the hardcoded allowlist.
pub(crate) fn is_allowed_extension(ext: &str) -> bool {
    matches!(
        ext,
        // Code
        "rs" | "py" | "js" | "ts" | "jsx" | "tsx"
            | "go" | "java" | "c" | "h" | "cpp" | "hpp" | "cc"
            | "rb" | "php" | "swift" | "kt" | "scala"
            | "ex" | "exs" | "hs" | "ml" | "lua" | "dart"
            | "zig" | "nix" | "tf" | "sh" | "bash" | "zsh"
            | "sql" | "cs" | "fs" | "vb" | "pl" | "pm"
            | "r" | "jl" | "nim" | "sol" | "v" | "vy" | "move"
        // Config / data
        | "toml" | "yaml" | "yml" | "json" | "xml"
            | "ini" | "cfg" | "conf" | "properties"
        // Documentation
        | "md" | "txt" | "rst" | "adoc" | "org" | "tex"
        // Web
        | "html" | "htm" | "css" | "scss" | "sass" | "less" | "svg"
        // Build
        | "dockerfile" | "makefile" | "cmake" | "gradle" | "sbt"
        // Other
        | "graphql" | "proto" | "thrift"
    )
}

/// Directory names that are always skipped during indexing.
const EXCLUDED_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "vendor",
    "target",
    "dist",
    "build",
    "out",
    "__pycache__",
    ".next",
    ".nuxt",
    ".context-pilot",
];

/// File patterns (suffixes) that are always skipped during indexing.
const EXCLUDED_SUFFIXES: &[&str] = &[".min.js", ".min.css", ".map", ".lock", ".sum"];

/// Check if a path component is an excluded directory.
pub(crate) fn is_excluded_dir(name: &str) -> bool {
    EXCLUDED_DIRS.contains(&name)
}

/// Check if a filename matches an excluded suffix pattern.
pub(crate) fn is_excluded_file(filename: &str) -> bool {
    EXCLUDED_SUFFIXES.iter().any(|suffix| filename.ends_with(suffix))
}

/// Shared indexability gate — the single source of truth for "does this file
/// belong in the search index?".
///
/// Called from BOTH `index_one_file` (live path) and the boot/hourly reconcile
/// disk-walk. Filter parity is load-bearing: if the reconcile walk were looser
/// than the indexer, it would forever re-queue files the indexer silently
/// rejects (they'd stay "on disk, not in index" → infinite re-queue churn).
///
/// Applies the cheap, read-free gates (symlink, excluded dir, extension
/// allowlist, excluded suffix, size cap). The UTF-8 readability check is an
/// inherent post-gate shared by both paths (reconcile routes through
/// `index_one_file`, which reads the file), so it is deliberately not here.
///
/// `meta` must be the file's `metadata()` (used for the size cap); `abs_path`
/// and `project_root` are used to derive the relative path components.
pub(crate) fn is_indexable(
    abs_path: &std::path::Path,
    project_root: &std::path::Path,
    meta: &std::fs::Metadata,
) -> bool {
    if abs_path.is_symlink() {
        return false;
    }

    let rel_path = abs_path.strip_prefix(project_root).unwrap_or(abs_path);

    // Excluded directory in any path component.
    for component in rel_path.components() {
        if let std::path::Component::Normal(name) = component
            && is_excluded_dir(name.to_str().unwrap_or(""))
        {
            return false;
        }
    }

    // Extension allowlist (text files only).
    let ext = rel_path.extension().and_then(std::ffi::OsStr::to_str).unwrap_or("");
    if !is_allowed_extension(ext) {
        return false;
    }

    // Excluded file suffix patterns (.min.js, .lock, …).
    let filename = rel_path.file_name().and_then(std::ffi::OsStr::to_str).unwrap_or("");
    if is_excluded_file(filename) {
        return false;
    }

    // Size cap.
    meta.len() <= MAX_FILE_SIZE
}

/// Visit every regular file under `root` that survives the directory gates:
/// [`EXCLUDED_DIRS`] and the `.gitignore` files found along the tree. Ignored
/// directories are pruned, never entered. Without this, a gitignored
/// `.claude/worktrees/` (full repo copies) or `.venv` meant ~200k stats on the
/// boot thread.
///
/// Only `.gitignore` files count (no global gitignore, no `.git/info/exclude`,
/// no parent dirs above `root`), matching [`GitignoreCache`] used by the live
/// indexer. Callers still apply [`is_indexable`] to each file.
pub(crate) fn walk_files(root: &std::path::Path, mut visit: impl FnMut(&std::path::Path, &std::fs::Metadata)) {
    let walker = ignore::WalkBuilder::new(root)
        .standard_filters(false)
        .git_ignore(true)
        .require_git(false)
        .parents(false)
        .follow_links(false)
        .filter_entry(|e| {
            let is_dir = e.file_type().is_some_and(|t| t.is_dir());
            e.depth() == 0 || !is_dir || !is_excluded_dir(e.file_name().to_str().unwrap_or(""))
        })
        .build();
    for entry in walker.flatten() {
        // Skip dirs and symlinks; other non-regular entries fail the extension gate.
        if entry.file_type().is_none_or(|t| t.is_dir() || t.is_symlink()) {
            continue;
        }
        if let Ok(meta) = entry.metadata() {
            visit(entry.path(), &meta);
        }
    }
}

/// Per-directory `.gitignore` matchers, built lazily, for the live indexer's
/// single-file checks. Same rules as [`walk_files`]: a path is ignored when it
/// or any ancestor below the root is ignored by the nearest `.gitignore` that
/// matches it (deepest wins, `!` re-includes).
#[derive(Default)]
pub(crate) struct GitignoreCache {
    /// Directory → its parsed `.gitignore` (`None` = no file or unparsable).
    by_dir: std::collections::HashMap<std::path::PathBuf, Option<ignore::gitignore::Gitignore>>,
}

impl GitignoreCache {
    /// Forget every parsed matcher (a `.gitignore` changed on disk).
    pub(crate) fn clear(&mut self) {
        self.by_dir.clear();
    }

    /// Whether `abs` (a file under `root`) is gitignored.
    pub(crate) fn is_ignored(&mut self, root: &std::path::Path, abs: &std::path::Path) -> bool {
        let Ok(rel) = abs.strip_prefix(root) else { return false };
        let comps: Vec<_> = rel.components().collect();
        let mut dirs = vec![root.to_path_buf()];
        for (i, comp) in comps.iter().enumerate() {
            let candidate = dirs.last().map_or_else(|| root.join(comp), |d| d.join(comp));
            let is_dir = i.saturating_add(1) < comps.len();
            if self.matched(&dirs, &candidate, is_dir) {
                return true;
            }
            dirs.push(candidate);
        }
        false
    }

    /// Decide `candidate` against the matchers of `dirs`, deepest first: the
    /// first ignore/whitelist verdict wins.
    fn matched(&mut self, dirs: &[std::path::PathBuf], candidate: &std::path::Path, is_dir: bool) -> bool {
        for dir in dirs.iter().rev() {
            let Some(gi) = self.matcher(dir) else { continue };
            let m = gi.matched(candidate, is_dir);
            if m.is_ignore() {
                return true;
            }
            if m.is_whitelist() {
                return false;
            }
        }
        false
    }

    /// The parsed `.gitignore` of `dir`, built on first use.
    fn matcher(&mut self, dir: &std::path::Path) -> Option<&ignore::gitignore::Gitignore> {
        self.by_dir
            .entry(dir.to_path_buf())
            .or_insert_with(|| {
                let file = dir.join(".gitignore");
                if !file.is_file() {
                    return None;
                }
                let mut builder = ignore::gitignore::GitignoreBuilder::new(dir);
                drop(builder.add(file));
                builder.build().ok()
            })
            .as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a unique temp dir under the system temp root for fs-touching tests.
    fn tmp_root(tag: &str) -> Result<std::path::PathBuf, String> {
        let mut p = std::env::temp_dir();
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        p.push(format!("cp-search-filters-{tag}-{nanos}"));
        std::fs::create_dir_all(&p).map_err(|e| format!("mkdir tmp root: {e}"))?;
        Ok(p)
    }

    /// Write a file under `root` and report whether the shared gate accepts it.
    /// Fallible setup surfaces as `Err` so tests need no `unwrap`/`expect`/`panic`.
    fn indexable(root: &std::path::Path, rel: &str, bytes: &[u8]) -> Result<bool, String> {
        let abs = root.join(rel);
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("mkdir parent: {e}"))?;
        }
        std::fs::write(&abs, bytes).map_err(|e| format!("write test file: {e}"))?;
        let meta = std::fs::metadata(&abs).map_err(|e| format!("stat test file: {e}"))?;
        Ok(is_indexable(&abs, root, &meta))
    }

    #[test]
    fn allowlisted_source_is_indexable() -> Result<(), String> {
        let root = tmp_root("ok")?;
        let ok = indexable(&root, "src/main.rs", b"fn main() {}")?;
        drop(std::fs::remove_dir_all(&root));
        if !ok {
            return Err("allowlisted source rejected".to_owned());
        }
        Ok(())
    }

    #[test]
    fn disallowed_extension_rejected() -> Result<(), String> {
        let root = tmp_root("ext")?;
        let ok = indexable(&root, "logo.png", b"\x89PNG")?;
        drop(std::fs::remove_dir_all(&root));
        if ok {
            return Err("disallowed extension accepted".to_owned());
        }
        Ok(())
    }

    #[test]
    fn excluded_dir_rejected() -> Result<(), String> {
        let root = tmp_root("dir")?;
        let ok = indexable(&root, "node_modules/pkg/index.js", b"x")?;
        drop(std::fs::remove_dir_all(&root));
        if ok {
            return Err("excluded dir accepted".to_owned());
        }
        Ok(())
    }

    #[test]
    fn excluded_suffix_rejected() -> Result<(), String> {
        let root = tmp_root("suf")?;
        let ok = indexable(&root, "app.min.js", b"x")?;
        drop(std::fs::remove_dir_all(&root));
        if ok {
            return Err("excluded suffix accepted".to_owned());
        }
        Ok(())
    }

    #[test]
    fn oversized_file_rejected() -> Result<(), String> {
        let root = tmp_root("big")?;
        let big = vec![b'a'; usize::try_from(MAX_FILE_SIZE).unwrap_or(usize::MAX) + 1];
        let ok = indexable(&root, "huge.rs", &big)?;
        drop(std::fs::remove_dir_all(&root));
        if ok {
            return Err("oversized file accepted".to_owned());
        }
        Ok(())
    }
}
