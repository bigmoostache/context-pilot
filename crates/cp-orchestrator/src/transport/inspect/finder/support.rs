//! Shared Finder helpers: realm resolution, path confinement, kind inference,
//! query-string parsing. Used by every Finder sub-module.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::transport::Backend;
use crate::transport::rest::HttpReply;

/// Resolve the agent's working directory from the registry record.
pub(super) fn fleet_folder(state: &Mutex<Backend>, fleet_id: &str) -> Result<String, HttpReply> {
    let entry = crate::transport::rest::resolve_entry(state, fleet_id)?;
    Ok(entry.folder)
}

/// Canonicalize and confine a path to a root directory.
///
/// `relative` may also be absolute: agents in a git worktree cite files by
/// full path (`<root>/.worktrees/<branch>/…`). Either way, `None` when the
/// resolved path escapes `root` (via `..`, symlinks, or an absolute path
/// elsewhere). An empty `relative` resolves to `root` itself.
pub(super) fn confined_path(root: &str, relative: &str) -> Option<PathBuf> {
    let root_path = Path::new(root);
    let root_canonical = root_path.canonicalize().ok()?;

    if relative.is_empty() || relative == "." {
        return Some(root_canonical);
    }

    let candidate = root_path.join(relative);
    let canonical = candidate.canonicalize().ok()?;
    canonical.starts_with(&root_canonical).then_some(canonical)
}

/// Always-previewable root outside the fleet folder: agents write scratch
/// files (prompts, reports) to `/tmp` and cite them in cards.
const PREVIEW_TMP_ROOT: &str = "/tmp";

/// Fleet-shared behaviours (skills, systems, commands) under `$HOME`. Agents
/// cite them in cards; they hold prompts, never secrets, so they are exempt
/// from the hidden-component ban that guards the rest of `~/.context-pilot`.
fn behaviours_root() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".context-pilot").join("behaviours"))
}

/// [`confined_path`] widened with `/tmp` and the behaviours dir — for
/// read-only file previews.
pub(super) fn previewable_path(root: &str, relative: &str) -> Option<PathBuf> {
    let behaviours = behaviours_root();
    let mut fixed = vec![Path::new(PREVIEW_TMP_ROOT)];
    fixed.extend(behaviours.as_deref());
    widened_path(root, relative, &fixed, &cp_env::env().orch.finder_extra_roots)
}

/// [`confined_path`] widened with `CP_FINDER_EXTRA_ROOTS` — for read-only
/// directory listings (`/tmp` is preview-only, never browsable).
pub(super) fn listable_path(root: &str, relative: &str) -> Option<PathBuf> {
    widened_path(root, relative, &[], &cp_env::env().orch.finder_extra_roots)
}

/// Resolve `relative` (relative to `root`, `..` allowed, or absolute) and
/// accept it inside `root` or under any of `fixed` / `extra`. Every root is
/// canonicalized (`/tmp` → `/private/tmp` on macOS), so symlinks cannot
/// smuggle a path past the prefix check. Under an `extra` root, any hidden
/// component (`.ssh`, `.context-pilot`) is refused: `$HOME` is the default
/// extra root and its dotfiles hold credentials. Never use for writes.
fn widened_path(root: &str, relative: &str, fixed: &[&Path], extra: &[PathBuf]) -> Option<PathBuf> {
    if let Some(inside) = confined_path(root, relative) {
        return Some(inside);
    }
    let canonical = Path::new(root).join(relative).canonicalize().ok()?;
    let under_fixed = fixed.iter().filter_map(|r| r.canonicalize().ok()).any(|r| canonical.starts_with(r));
    let under_extra = || {
        extra.iter().filter_map(|r| r.canonicalize().ok()).any(|r| {
            canonical
                .strip_prefix(&r)
                .is_ok_and(|rest| !rest.components().any(|c| c.as_os_str().to_str().is_none_or(|s| s.starts_with('.'))))
        })
    };
    (under_fixed || under_extra()).then_some(canonical)
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
        (k == key).then(|| percent_decode(v))
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
            if let (Some(d1), Some(d2)) = (h1, h2) {
                // d1 and d2 are the two chars after `%`; from_str_radix
                // validates they are hex, so the else branch handles non-hex.
                let pair = [d1, d2];
                if let Ok(decoded) = u8::from_str_radix(std::str::from_utf8(&pair).unwrap_or(""), 16) {
                    bytes.push(decoded);
                } else {
                    // Not valid hex — pass through literally.
                    bytes.push(b'%');
                    bytes.push(d1);
                    bytes.push(d2);
                }
            } else {
                // Trailing `%` at end of string — pass through literally.
                bytes.push(b'%');
                if let Some(d1) = h1 {
                    bytes.push(d1);
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
        // Absolute path elsewhere rejected
        assert!(confined_path("/tmp", "/etc/passwd").is_none());
    }

    #[test]
    fn confined_path_accepts_absolute_inside_root() {
        let root = std::env::temp_dir().canonicalize().expect("temp dir");
        let inner = root.join("cp-confined-abs-probe");
        std::fs::create_dir_all(&inner).expect("mkdir");
        let got = confined_path(&root.to_string_lossy(), &inner.to_string_lossy());
        let _r = std::fs::remove_dir(&inner);
        assert_eq!(got, Some(inner));
    }

    #[test]
    fn widened_path_reaches_extra_root_via_dotdot() {
        let base = std::env::temp_dir().canonicalize().expect("temp dir").join("cp-widened-probe");
        let agent = base.join("agent");
        let extra = base.join("dev/ingestion");
        std::fs::create_dir_all(&agent).expect("mkdir agent");
        std::fs::create_dir_all(&extra).expect("mkdir extra");
        let root = agent.to_string_lossy();
        let allowed = [base.join("dev")];
        let got = widened_path(&root, "../dev/ingestion", &[], &allowed);
        let denied = widened_path(&root, "../dev/ingestion", &[], &[]);
        let _r = std::fs::remove_dir_all(&base);
        assert_eq!(got, Some(extra));
        assert!(denied.is_none());
        assert!(widened_path(&root, "/etc", &[], &allowed).is_none());
    }

    #[test]
    fn widened_path_refuses_hidden_under_extra_root() {
        let base = std::env::temp_dir().canonicalize().expect("temp dir").join("cp-widened-hidden");
        let agent = base.join("agent");
        let secret = base.join("home/.ssh");
        std::fs::create_dir_all(&agent).expect("mkdir agent");
        std::fs::create_dir_all(&secret).expect("mkdir secret");
        let root = agent.to_string_lossy();
        let allowed = [base.join("home")];
        let home = widened_path(&root, "../home", &[], &allowed);
        let hidden = widened_path(&root, "../home/.ssh", &[], &allowed);
        let _r = std::fs::remove_dir_all(&base);
        assert_eq!(home, Some(base.join("home")));
        assert!(hidden.is_none());
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
        // A plain call, so each classification case costs nothing against the
        // cognitive-complexity cap (the seven inline assert_eq!s tripped it).
        fn assert_kind(name: &str, want: &str) {
            assert_eq!(infer_kind(name), want);
        }
        assert_kind("main.rs", "code");
        assert_kind("README.md", "markdown");
        assert_kind("data.json", "json");
        assert_kind("photo.png", "image");
        assert_kind("config.yaml", "doc");
        assert_kind("archive.zip", "archive");
        assert_kind("mystery", "binary");
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
        assert_eq!(percent_decode("t%C3%A9st"), "t\u{e9}st");
        // ñ = U+00F1 = UTF-8 bytes C3 B1
        assert_eq!(percent_decode("espa%C3%B1ol"), "espa\u{f1}ol");
        // 日 = U+65E5 = UTF-8 bytes E6 97 A5
        assert_eq!(percent_decode("%E6%97%A5%E6%9C%AC"), "\u{65e5}\u{672c}");
        // Mixed: spaces + accents
        assert_eq!(percent_decode("caf%C3%A9%20latt%C3%A9"), "caf\u{e9} latt\u{e9}");
        // emoji: 🎉 = U+1F389 = UTF-8 bytes F0 9F 8E 89
        assert_eq!(percent_decode("%F0%9F%8E%89"), "\u{1f389}");
    }

    #[test]
    fn extract_param_finds_value() {
        assert_eq!(extract_param("path=src%2Flib&format=json", "path"), Some("src/lib".to_owned()));
        assert_eq!(extract_param("path=src%2Flib&format=json", "format"), Some("json".to_owned()));
        assert_eq!(extract_param("path=src", "missing"), None);
        // Multi-byte UTF-8: é in a filename
        assert_eq!(extract_param("name=t%C3%A9st.xlsx", "name"), Some("t\u{e9}st.xlsx".to_owned()));
    }
}
