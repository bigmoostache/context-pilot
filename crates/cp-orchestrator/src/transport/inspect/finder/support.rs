//! Shared Finder helpers: realm resolution, path confinement, kind inference,
//! query-string parsing. Used by every Finder sub-module.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::transport::Backend;
use crate::transport::rest::HttpReply;

/// Resolve the agent's working directory from the registry record.
pub(super) fn agent_folder(state: &Mutex<Backend>, agent_id: &str) -> Result<String, HttpReply> {
    let entry = crate::transport::rest::resolve_entry(state, agent_id)?;
    Ok(entry.folder)
}

/// Canonicalize and confine a relative path to a root directory.
///
/// Returns `None` if the resolved path escapes `root` (via `..`, symlinks,
/// or absolute paths). An empty `relative` resolves to `root` itself.
pub(super) fn confined_path(root: &str, relative: &str) -> Option<PathBuf> {
    let root_path = Path::new(root);
    let root_canonical = root_path.canonicalize().ok()?;

    if relative.is_empty() || relative == "." {
        return Some(root_canonical);
    }

    // Reject absolute paths outright.
    if relative.starts_with('/') {
        return None;
    }

    let candidate = root_path.join(relative);
    let canonical = candidate.canonicalize().ok()?;
    if canonical.starts_with(&root_canonical) { Some(canonical) } else { None }
}

/// Count the non-hidden direct children of a directory.
///
/// Used to annotate folder nodes with an item count for the Finder views.
/// Returns `0` on any I/O error (unreadable dir) — a best-effort hint, never
/// an error surface. Skips dotfiles to match the listing's own filtering, so
/// the count equals what opening the folder would display.
pub(super) fn count_visible_children(dir: &Path) -> usize {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return 0;
    };
    rd.filter_map(Result::ok).filter(|e| e.file_name().to_str().is_some_and(|n| !n.starts_with('.'))).count()
}

/// Infer a `FinderKind` string from a filename's extension.
pub(super) fn infer_kind(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "rs" | "py" | "js" | "ts" | "tsx" | "jsx" | "go" | "c" | "cpp" | "h" | "hpp" | "java" | "rb" | "sh"
        | "bash" | "zsh" | "lua" | "zig" | "swift" | "kt" | "scala" | "ex" | "exs" | "erl" | "hs" | "ml" | "css"
        | "scss" | "html" | "sql" | "r" | "pl" | "php" | "cs" | "fs" | "vue" | "svelte" | "dart" | "nim" | "v"
        | "wasm" => "code",
        "md" | "mdx" => "markdown",
        "json" | "jsonl" | "json5" => "json",
        "pdf" => "pdf",
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "bmp" | "ico" | "tiff" | "heic" => "image",
        "csv" | "xlsx" | "xls" | "ods" | "tsv" => "sheet",
        "pptx" | "ppt" | "odp" => "slides",
        "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar" | "zst" => "archive",
        "mp3" | "wav" | "flac" | "m4a" | "ogg" | "aac" | "wma" => "audio",
        "mp4" | "mov" | "avi" | "mkv" | "webm" | "wmv" | "flv" => "video",
        "txt" | "log" | "yml" | "yaml" | "toml" | "cfg" | "ini" | "env" | "conf" | "properties" | "lock"
        | "editorconfig" | "gitignore" | "dockerignore" => "doc",
        _ => "binary",
    }
}

/// Extract a query parameter value by key from a raw query string.
pub(super) fn extract_param(query: &str, key: &str) -> Option<String> {
    query.split('&').filter(|s| !s.is_empty()).find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        if k == key {
            // Percent-decode the value (basic: %20 → space, %2F → /).
            Some(percent_decode(v))
        } else {
            None
        }
    })
}

/// Percent-decode a query-parameter value.
///
/// Operates on raw bytes so multi-byte UTF-8 sequences (`%C3%A9` → `é`) are
/// reassembled correctly. The old char-by-char approach pushed each decoded
/// byte as a Unicode code point, turning `é` into `Ã©` (mojibake).
fn percent_decode(input: &str) -> String {
    let mut bytes = Vec::with_capacity(input.len());
    let mut src = input.as_bytes().iter().copied();
    while let Some(b) = src.next() {
        if b == b'%' {
            let h1 = src.next();
            let h2 = src.next();
            if let (Some(h1), Some(h2)) = (h1, h2) {
                // SAFETY: h1 and h2 are ASCII hex digits (or not); from_str_radix
                // validates, so the unwrap_or fallback is unreachable for valid
                // percent-encoded input.
                let pair = [h1, h2];
                if let Ok(decoded) = u8::from_str_radix(std::str::from_utf8(&pair).unwrap_or(""), 16) {
                    bytes.push(decoded);
                } else {
                    // Not valid hex — pass through literally.
                    bytes.push(b'%');
                    bytes.push(h1);
                    bytes.push(h2);
                }
            } else {
                // Trailing `%` at end of string — pass through literally.
                bytes.push(b'%');
                if let Some(h1) = h1 {
                    bytes.push(h1);
                }
            }
        } else if b == b'+' {
            bytes.push(b' ');
        } else {
            bytes.push(b);
        }
    }
    String::from_utf8(bytes).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confined_path_rejects_escape() {
        // Trying to escape via ..
        assert!(confined_path("/tmp", "../etc/passwd").is_none());
        // Absolute path rejected
        assert!(confined_path("/tmp", "/etc/passwd").is_none());
    }

    #[test]
    fn confined_path_accepts_valid() {
        // Empty relative = root itself
        let root = std::env::temp_dir();
        let root_str = root.to_string_lossy().to_string();
        let result = confined_path(&root_str, "");
        assert!(result.is_some());
    }

    #[test]
    fn infer_kind_classifies_extensions() {
        assert_eq!(infer_kind("main.rs"), "code");
        assert_eq!(infer_kind("README.md"), "markdown");
        assert_eq!(infer_kind("data.json"), "json");
        assert_eq!(infer_kind("photo.png"), "image");
        assert_eq!(infer_kind("config.yaml"), "doc");
        assert_eq!(infer_kind("archive.zip"), "archive");
        assert_eq!(infer_kind("mystery"), "binary");
    }

    #[test]
    fn percent_decode_handles_common_cases() {
        assert_eq!(percent_decode("hello%20world"), "hello world");
        assert_eq!(percent_decode("src%2Fmain.rs"), "src/main.rs");
        assert_eq!(percent_decode("plain"), "plain");
    }

    #[test]
    fn percent_decode_handles_multibyte_utf8() {
        // é = U+00E9 = UTF-8 bytes C3 A9
        assert_eq!(percent_decode("t%C3%A9st"), "tést");
        // ñ = U+00F1 = UTF-8 bytes C3 B1
        assert_eq!(percent_decode("espa%C3%B1ol"), "español");
        // 日 = U+65E5 = UTF-8 bytes E6 97 A5
        assert_eq!(percent_decode("%E6%97%A5%E6%9C%AC"), "日本");
        // Mixed: spaces + accents
        assert_eq!(percent_decode("caf%C3%A9%20latt%C3%A9"), "café latté");
        // emoji: 🎉 = U+1F389 = UTF-8 bytes F0 9F 8E 89
        assert_eq!(percent_decode("%F0%9F%8E%89"), "🎉");
    }

    #[test]
    fn extract_param_finds_value() {
        assert_eq!(extract_param("path=src%2Flib&format=json", "path"), Some("src/lib".to_owned()));
        assert_eq!(extract_param("path=src%2Flib&format=json", "format"), Some("json".to_owned()));
        assert_eq!(extract_param("path=src", "missing"), None);
        // Multi-byte UTF-8: é in a filename
        assert_eq!(extract_param("name=t%C3%A9st.xlsx", "name"), Some("tést.xlsx".to_owned()));
    }
}
