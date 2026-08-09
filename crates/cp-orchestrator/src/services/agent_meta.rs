//! Orchestrator-owned **agent metadata** — display-name overrides (T328) and
//! profile-picture avatars (T338).
//!
//! Merged from the former `names.rs` + `avatars.rs` for the directory-entry
//! budget.  Both stores share the same shape: a `HashMap` index persisted to a
//! sidecar JSON file under `<agents_dir>/`, loaded at startup, written
//! atomically on mutation.  They are cosmetic — never a hard dependency for the
//! backend to boot.
//!
//! # Display names
//!
//! By default an agent's display name is the basename of its realm folder
//! (e.g. `context-pilot` for `/Users/gui/context-pilot`).
//! [`NameOverrides`] lets the dashboard user set a custom label per agent.
//!
//! # Avatars
//!
//! Agents can have a profile picture set via the dashboard.  The image bytes
//! live under `<agents_dir>/agent-avatars/<id>` (no extension — the
//! content-type is tracked in a sidecar JSON map) and are served inline by the
//! transport layer with the correct `Content-Type`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use cp_wire::types::command::Command;
use cp_wire::types::registry::Entry;

// ── Display-name overrides ──────────────────────────────────────────────

/// In-memory + on-disk map of agent id → custom display name.
#[derive(Debug, Default)]
pub struct NameOverrides {
    /// Agent id → display name.
    names: HashMap<String, String>,
    /// The backing file (`<agents_dir>/agent-names.json`).
    path: PathBuf,
}

impl NameOverrides {
    /// Load from `<agents_dir>/agent-names.json`, or start empty.
    ///
    /// A missing or corrupt file silently yields an empty map — naming is a
    /// convenience, never a hard dependency for the backend to boot.
    #[must_use]
    pub fn load(agents_dir: &Path) -> Self {
        let path = agents_dir.join("agent-names.json");
        let names = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<HashMap<String, String>>(&bytes).ok())
            .unwrap_or_default();
        Self { names, path }
    }

    /// Look up a custom display name for `agent_id`.
    ///
    /// Returns `None` when the agent has no override (callers fall back to the
    /// folder-derived basename).
    #[must_use]
    pub fn get(&self, agent_id: &str) -> Option<&str> {
        self.names.get(agent_id).map(String::as_str)
    }

    /// Set or clear a display-name override.
    ///
    /// An empty or whitespace-only `name` **removes** the override (reverts to
    /// the folder-derived default).  Returns the previous override, if any.
    pub fn set(&mut self, agent_id: &str, name: &str) -> Option<String> {
        let trimmed = name.trim();
        let prev = if trimmed.is_empty() {
            self.names.remove(agent_id)
        } else {
            self.names.insert(agent_id.to_owned(), trimmed.to_owned())
        };
        self.persist();
        prev
    }

    /// Atomically write the map to disk (`tmp` → `rename`).
    fn persist(&self) {
        let Ok(bytes) = serde_json::to_vec_pretty(&self.names) else {
            eprintln!("names: serialize failed");
            return;
        };
        let tmp = self.path.with_extension("json.tmp");
        if std::fs::write(&tmp, &bytes).is_err() {
            eprintln!("names: write tmp failed: {}", tmp.display());
            return;
        }
        if let Err(e) = std::fs::rename(&tmp, &self.path) {
            eprintln!("names: rename failed: {e}");
        }
    }
}

// ── Avatars ─────────────────────────────────────────────────────────────

/// Maximum accepted avatar upload size (2 MiB). Generous for a profile
/// picture; prevents accidental multi-megabyte uploads from ballooning the
/// agents directory.
pub const MAX_AVATAR_BYTES: usize = 2 * 1024 * 1024;

/// In-memory + on-disk map of agent id → avatar content type, backed by
/// raw image files on disk.
#[derive(Debug, Default)]
pub struct AvatarStore {
    /// Agent id → MIME content type (e.g. `"image/png"`).
    types: HashMap<String, String>,
    /// The directory holding raw avatar files (`<agents_dir>/agent-avatars/`).
    dir: PathBuf,
    /// The sidecar JSON index (`<agents_dir>/agent-avatars.json`).
    index_path: PathBuf,
}

impl AvatarStore {
    /// Load from `<agents_dir>/agent-avatars.json` + the sibling directory,
    /// or start empty. A missing or corrupt index silently yields an empty
    /// map — avatars are cosmetic, never a hard dependency.
    #[must_use]
    pub fn load(agents_dir: &Path) -> Self {
        let index_path = agents_dir.join("agent-avatars.json");
        let dir = agents_dir.join("agent-avatars");
        let types = std::fs::read(&index_path)
            .ok()
            .and_then(|b| serde_json::from_slice::<HashMap<String, String>>(&b).ok())
            .unwrap_or_default();
        Self { types, dir, index_path }
    }

    /// Whether `agent_id` has a stored avatar.
    #[must_use]
    pub fn has(&self, agent_id: &str) -> bool {
        self.types.contains_key(agent_id)
    }

    /// Read the avatar bytes + content type for `agent_id`.
    ///
    /// Returns `None` when no avatar is stored or the file is missing/corrupt.
    #[must_use]
    pub fn get(&self, agent_id: &str) -> Option<(Vec<u8>, String)> {
        let ctype = self.types.get(agent_id)?;
        let bytes = std::fs::read(self.dir.join(agent_id)).ok()?;
        Some((bytes, ctype.clone()))
    }

    /// Store (or replace) an avatar. `bytes` are the raw image; the content
    /// type is sniffed from magic bytes. Returns an error string on failure.
    pub fn set(&mut self, agent_id: &str, bytes: &[u8]) -> Result<(), String> {
        if bytes.len() > MAX_AVATAR_BYTES {
            return Err(format!("avatar too large ({} bytes, max {})", bytes.len(), MAX_AVATAR_BYTES));
        }
        let ctype = sniff_image_type(bytes).ok_or("unrecognised image format")?;
        // Ensure the directory exists.
        if let Err(e) = std::fs::create_dir_all(&self.dir) {
            return Err(format!("mkdir agent-avatars: {e}"));
        }
        // Write the image atomically (tmp → rename).
        let file_path = self.dir.join(agent_id);
        let tmp = file_path.with_extension("tmp");
        std::fs::write(&tmp, bytes).map_err(|e| format!("write tmp: {e}"))?;
        std::fs::rename(&tmp, &file_path).map_err(|e| format!("rename: {e}"))?;
        let _prev = self.types.insert(agent_id.to_owned(), ctype.to_owned());
        self.persist_index();
        Ok(())
    }

    /// Remove an avatar. Returns `true` if one existed.
    pub fn remove(&mut self, agent_id: &str) -> bool {
        let existed = self.types.remove(agent_id).is_some();
        if existed {
            let _rm = std::fs::remove_file(self.dir.join(agent_id));
            self.persist_index();
        }
        existed
    }

    /// Atomically write the content-type index to disk.
    fn persist_index(&self) {
        let Ok(bytes) = serde_json::to_vec_pretty(&self.types) else {
            eprintln!("avatars: serialize index failed");
            return;
        };
        let tmp = self.index_path.with_extension("json.tmp");
        if std::fs::write(&tmp, &bytes).is_err() {
            eprintln!("avatars: write tmp failed: {}", tmp.display());
            return;
        }
        if let Err(e) = std::fs::rename(&tmp, &self.index_path) {
            eprintln!("avatars: rename failed: {e}");
        }
    }
}

/// Sniff the MIME content type from the first bytes of an image.
///
/// Recognises PNG, JPEG, GIF, WebP, BMP, ICO, and SVG (text-based, detected
/// by a leading `<svg` or `<?xml` + `<svg`).
fn sniff_image_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() < 4 {
        return None;
    }
    // PNG: 89 50 4E 47
    if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        return Some("image/png");
    }
    // JPEG: FF D8 FF
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    // GIF: GIF8
    if bytes.starts_with(b"GIF8") {
        return Some("image/gif");
    }
    // WebP: RIFF....WEBP
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    // BMP: BM
    if bytes.starts_with(b"BM") {
        return Some("image/bmp");
    }
    // ICO: 00 00 01 00
    if bytes.starts_with(&[0x00, 0x00, 0x01, 0x00]) {
        return Some("image/x-icon");
    }
    // SVG (text-based) — look for <svg in the first 256 bytes.
    let head = &bytes[..bytes.len().min(256)];
    if let Ok(text) = std::str::from_utf8(head) {
        let lower = text.to_lowercase();
        if lower.contains("<svg") {
            return Some("image/svg+xml");
        }
    }
    None
}

// ── Agent-owned profile (T739b) ─────────────────────────────────────────

/// Realm-relative path (minus extension) an uploaded avatar is written to.
///
/// Inside the agent's own realm, under its existing `.context-pilot` directory,
/// so the reference the agent advertises satisfies its own containment rule (a
/// realm-relative path that cannot climb out) and travels with the folder if it
/// is ever moved.
const AVATAR_STEM: &str = ".context-pilot/avatar";

/// The file extension to store an image of `content_type` under.
///
/// The extension is cosmetic for serving (the content type is re-sniffed from
/// the bytes on read) but keeps the file recognisable to a human browsing the
/// realm, and lets the stored reference name a single concrete file.
fn extension_for(content_type: &str) -> &'static str {
    match content_type {
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        "image/x-icon" => "ico",
        "image/svg+xml" => "svg",
        // PNG is both the common case and the safe default: an unrecognised
        // type never reaches here (`sniff_image_type` rejects it first).
        _ => "png",
    }
}

/// Write uploaded avatar `bytes` into the agent's realm and return the
/// **realm-relative reference** to advertise.
///
/// This is the byte half of the ownership split (T739b): the orchestrator stays
/// the transport (it is the only party with an HTTP surface, so an upload must
/// land here) while the *authority* over what the image is moves to the agent,
/// which receives only this reference. Keeping multi-megabyte bytes out of the
/// `0600` registry record matters because that record is re-read on every fleet
/// scan.
///
/// Any previously stored avatar file is removed first, so switching format
/// (`avatar.png` → `avatar.jpg`) cannot leave the old file orphaned in the
/// realm.
///
/// # Errors
///
/// Returns a human-readable message when the upload exceeds
/// [`MAX_AVATAR_BYTES`], is not a recognised image, or cannot be written.
pub fn write_avatar_into_realm(folder: &str, bytes: &[u8]) -> Result<String, String> {
    if bytes.len() > MAX_AVATAR_BYTES {
        return Err(format!("avatar too large ({} bytes, max {})", bytes.len(), MAX_AVATAR_BYTES));
    }
    let ctype = sniff_image_type(bytes).ok_or("unrecognised image format")?;
    let rel = format!("{AVATAR_STEM}.{ext}", ext = extension_for(ctype));
    let abs = Path::new(folder).join(&rel);

    if let Some(parent) = abs.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return Err(format!("create realm avatar dir: {e}"));
    }
    // Drop any prior avatar in a different format before writing the new one.
    remove_realm_avatars(folder);

    std::fs::write(&abs, bytes).map_err(|e| format!("write avatar: {e}"))?;
    Ok(rel)
}

/// Remove every stored avatar file from the agent's realm (all extensions).
///
/// Best-effort and format-agnostic: the caller does not know which extension a
/// previous upload used, and a missing file is the expected case rather than an
/// error.
pub fn remove_realm_avatars(folder: &str) {
    for ext in ["png", "jpg", "gif", "webp", "bmp", "ico", "svg"] {
        let _removed = std::fs::remove_file(Path::new(folder).join(format!("{AVATAR_STEM}.{ext}")));
    }
}

/// Read an agent-owned avatar back out of its realm, given the reference the
/// agent advertises.
///
/// Returns the bytes plus a content type **re-sniffed from those bytes** rather
/// than trusted from the reference's extension: the reference is a value the
/// agent owns and could name anything, so deriving the type from the content is
/// the only honest source. A URL reference yields `None` (there are no local
/// bytes to serve — the caller redirects instead), as does a reference that
/// escapes the realm, is missing, or is not an image.
///
/// The containment re-check is deliberate belt-and-braces. The agent already
/// rejects an escaping path when it accepts the value, but this reads a file
/// from a path that ultimately arrived over the wire, so it re-establishes the
/// boundary at the point of use rather than trusting a check made elsewhere.
#[must_use]
pub fn read_realm_avatar(folder: &str, image: &str) -> Option<(Vec<u8>, String)> {
    if image.is_empty() || image.starts_with("http://") || image.starts_with("https://") {
        return None;
    }
    let rel = Path::new(image);
    if rel.is_absolute() || rel.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return None;
    }
    let bytes = std::fs::read(Path::new(folder).join(rel)).ok()?;
    let ctype = sniff_image_type(&bytes)?;
    Some((bytes, ctype.to_owned()))
}

/// Send a [`SetProfile`](cp_wire::types::command::Kind::SetProfile) command to a
/// live agent — the write half of the T739b ownership move.
///
/// Both fields are patch-shaped and forwarded verbatim, so a rename
/// (`slug: Some`, `image: None`) cannot clear an avatar and vice versa. The
/// agent re-validates before applying, which is the point of the move: the
/// containment rules for an image reference are enforced by the party that owns
/// the value, not by each reader.
///
/// # Errors
///
/// Returns a message when the agent is unreachable (a dead or retired agent has
/// no process to command) or rejects the command. Callers treat that as the
/// signal to fall back to the legacy orchestrator-side sidecar rather than
/// failing the user's request.
pub fn send_profile(entry: &Entry, slug: Option<String>, image: Option<String>) -> Result<(), String> {
    let dedup = format!("profile-{id}-{now}", id = entry.id, now = now_ms());
    let command =
        Command::new(format!("cmd-{dedup}"), 1, dedup, cp_wire::types::command::Kind::SetProfile { slug, image });
    let ack = crate::channel::AgentChannel::from_entry(entry).send(command).map_err(|e| format!("{e}"))?;
    match ack.status {
        cp_wire::types::ack::Status::Accepted => Ok(()),
        _ => Err("agent rejected the profile command".to_owned()),
    }
}

/// Wall-clock milliseconds, for a unique dedup token per profile command.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avatar_written_into_realm_is_relative_and_contained() {
        let realm = std::env::temp_dir().join(format!("cp-realm-avatar-{}", std::process::id()));
        drop(std::fs::create_dir_all(&realm));
        let folder = realm.to_string_lossy().into_owned();

        let png: &[u8] = &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00];
        let rel = write_avatar_into_realm(&folder, png).expect("write");

        assert_eq!(rel, ".context-pilot/avatar.png");
        assert!(!rel.starts_with('/'), "the advertised reference must be realm-relative");
        assert!(realm.join(&rel).is_file(), "the bytes land inside the realm");

        // A second upload in another format replaces rather than orphans.
        let gif: &[u8] = b"GIF89a-------";
        let rel2 = write_avatar_into_realm(&folder, gif).expect("write gif");
        assert_eq!(rel2, ".context-pilot/avatar.gif");
        assert!(!realm.join(".context-pilot/avatar.png").exists(), "the previous format is cleaned up");

        remove_realm_avatars(&folder);
        assert!(!realm.join(&rel2).exists(), "removal clears every format");

        drop(std::fs::remove_dir_all(&realm));
    }

    #[test]
    fn realm_avatar_rejects_oversized_and_non_images() {
        let realm = std::env::temp_dir().join(format!("cp-realm-bad-{}", std::process::id()));
        drop(std::fs::create_dir_all(&realm));
        let folder = realm.to_string_lossy().into_owned();

        assert!(write_avatar_into_realm(&folder, &vec![0u8; MAX_AVATAR_BYTES + 1]).is_err());
        assert!(write_avatar_into_realm(&folder, b"not an image at all").is_err());

        drop(std::fs::remove_dir_all(&realm));
    }

    #[test]
    fn name_set_get_roundtrip() {
        let dir = std::env::temp_dir().join(format!("cp-names-test-{}", std::process::id()));
        drop(std::fs::create_dir_all(&dir));

        let mut store = NameOverrides::load(&dir);
        assert!(store.get("a").is_none());

        let _prev = store.set("a", "My Agent");
        assert_eq!(store.get("a"), Some("My Agent"));

        // Reload from disk proves persistence.
        let reloaded = NameOverrides::load(&dir);
        assert_eq!(reloaded.get("a"), Some("My Agent"));

        // Empty name clears the override.
        let _prev = store.set("a", "  ");
        assert!(store.get("a").is_none());
        assert!(NameOverrides::load(&dir).get("a").is_none());

        drop(std::fs::remove_dir_all(&dir));
    }

    #[test]
    fn avatar_roundtrip_set_get_remove() {
        let dir = std::env::temp_dir().join(format!("cp-avatar-test-{}", std::process::id()));
        drop(std::fs::create_dir_all(&dir));

        let mut store = AvatarStore::load(&dir);
        assert!(!store.has("a1"));

        // A minimal 1×1 PNG (inline bytes — no external file needed).
        let png: &[u8] = &[
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
            0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR chunk
            0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, // 1×1
            0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, // RGB, CRC
            0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, // IDAT chunk
            0x54, 0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01, 0xE2, 0x21, 0xBC, 0x33, 0x00,
            0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, // IEND chunk
            0x44, 0xAE, 0x42, 0x60, 0x82,
        ];
        store.set("a1", png).expect("set should succeed");
        assert!(store.has("a1"));

        let (bytes, ctype) = store.get("a1").expect("get should succeed");
        assert_eq!(ctype, "image/png");
        assert_eq!(bytes, png);

        // Reload from disk proves persistence.
        let reloaded = AvatarStore::load(&dir);
        assert!(reloaded.has("a1"));

        // Remove.
        assert!(store.remove("a1"));
        assert!(!store.has("a1"));
        assert!(AvatarStore::load(&dir).get("a1").is_none());

        drop(std::fs::remove_dir_all(&dir));
    }

    #[test]
    fn avatar_rejects_oversized() {
        let dir = std::env::temp_dir().join(format!("cp-avatar-big-{}", std::process::id()));
        drop(std::fs::create_dir_all(&dir));

        let mut store = AvatarStore::load(&dir);
        let huge = vec![0u8; MAX_AVATAR_BYTES + 1];
        assert!(store.set("a1", &huge).is_err());

        drop(std::fs::remove_dir_all(&dir));
    }

    #[test]
    fn sniff_formats() {
        assert_eq!(sniff_image_type(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A]), Some("image/png"));
        assert_eq!(sniff_image_type(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(sniff_image_type(b"GIF89a..."), Some("image/gif"));
        assert_eq!(sniff_image_type(b"RIFF\x00\x00\x00\x00WEBP"), Some("image/webp"));
        assert_eq!(sniff_image_type(b"<svg xmlns="), Some("image/svg+xml"));
        assert_eq!(sniff_image_type(b"\x00\x01"), None); // too short
    }
}
