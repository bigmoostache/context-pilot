//! ` ```file-upload ` blocks → one-line attachment chips.
//!
//! The webapp composer and the TUI large-paste path both embed attachments in
//! the user message as a YAML fenced block (`file:` + indented `path`/`name`/
//! `size`/`note`). The agent reads the YAML as plain context; the TUI collapses
//! each block into a single `📎 name · size` line, both in the composer
//! placeholder and in the rendered conversation.

/// Opening fence of an attachment block.
pub(crate) const FILE_UPLOAD_FENCE: &str = "```file-upload";

/// One attachment parsed from a ` ```file-upload ` block.
pub(crate) struct FileChip {
    /// Stored filename (falls back to the path's basename).
    pub name: String,
    /// Realm-relative stored path, e.g. `.uploads/paste-20261020-140000.txt`.
    pub path: String,
    /// Byte count, when the block carries a parsable `size`.
    pub size: Option<u64>,
}

impl FileChip {
    /// Display label: `📎 name · 12.3 KB`.
    pub(crate) fn label(&self) -> String {
        self.size.map_or_else(|| format!("📎 {}", self.name), |s| format!("📎 {} · {}", self.name, format_size(s)))
    }
}

/// Human byte size: `512 B`, `12.3 KB`, `4.0 MB`.
fn format_size(bytes: u64) -> String {
    let tenths = |unit: u64| {
        let t = bytes.saturating_mul(10).checked_div(unit).unwrap_or(0);
        format!("{}.{}", t.checked_div(10).unwrap_or(0), t.checked_rem(10).unwrap_or(0))
    };
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{} KB", tenths(1024))
    } else {
        format!("{} MB", tenths(1024 * 1024))
    }
}

/// Parse a block body (the lines between the fences). Tolerant: unknown keys
/// are ignored; `None` only when there is no `path`.
fn parse_body(body: &[&str]) -> Option<FileChip> {
    let mut found_path = None;
    let mut found_name = None;
    let mut size = None;
    for line in body {
        let Some((key, raw)) = line.trim().split_once(':') else { continue };
        let value = raw.trim().to_owned();
        match key.trim() {
            "path" => found_path = Some(value),
            "name" => found_name = Some(value),
            "size" => size = value.parse::<u64>().ok(),
            _ => {}
        }
    }
    let path = found_path?;
    let name = found_name.unwrap_or_else(|| path.rsplit('/').next().unwrap_or(&path).to_owned());
    Some(FileChip { name, path, size })
}

/// If `lines[start]` opens a ` ```file-upload ` block, parse it and return the
/// chip plus the index just past the closing fence. `None` when the line is not
/// a fence, the block is unterminated, or it has no `path`.
pub(crate) fn parse_file_upload_at(lines: &[&str], start: usize) -> Option<(FileChip, usize)> {
    if lines.get(start)?.trim() != FILE_UPLOAD_FENCE {
        return None;
    }
    let body_start = start.saturating_add(1);
    let close = lines.get(body_start..)?.iter().position(|l| l.trim() == "```")?;
    let body = lines.get(body_start..body_start.saturating_add(close))?;
    let chip = parse_body(body)?;
    Some((chip, body_start.saturating_add(close).saturating_add(1)))
}

/// Parse a paste buffer that is exactly one attachment block (surrounding
/// blank lines allowed) — how the TUI stores a large paste.
pub(crate) fn parse_file_upload_buffer(text: &str) -> Option<FileChip> {
    let lines: Vec<&str> = text.trim().lines().collect();
    let (chip, end) = parse_file_upload_at(&lines, 0)?;
    (end == lines.len()).then_some(chip)
}
