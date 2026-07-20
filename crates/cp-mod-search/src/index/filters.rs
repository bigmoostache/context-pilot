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
