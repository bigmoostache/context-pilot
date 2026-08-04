//! The fleet half of the Agora — reading *other* agents' advertisements.
//!
//! Every agent publishes a registry record to `~/.context-pilot/agents/<id>.json`
//! describing itself: where it lives, what it calls itself, and (since the
//! record became a live projection) its Agora identity. This module reads that
//! directory so the panel can show the agent who its peers are.
//!
//! # Why the record and not the orchestrator
//!
//! The orchestrator exposes a much richer fleet view, but reaching it would
//! mean an HTTP round-trip from a panel render path, and it would only work
//! while the orchestrator happens to be running. The records are the agents'
//! own advertisements — the authoritative, always-present source — and reading
//! them directly keeps this panel truthful even with no backend alive.
//!
//! # Failure is silent by design
//!
//! A peer record that cannot be read or decoded is skipped, not reported. This
//! is a cosmetic panel: one malformed neighbour must never blank the view or
//! raise an error into the render path. The cost of that choice is that a
//! corrupt record disappears without a trace, which is the right trade here and
//! the wrong one anywhere a decision depends on the data.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::types::{PeerAgent, SelfIdentity};

/// Minimum gap between two directory scans, in milliseconds.
///
/// The panel re-renders far more often than the fleet changes (an agent
/// appears when someone boots one, which is a human-scale event), so scanning
/// on every refresh would be pure syscall waste. Two seconds keeps the view
/// feeling live while collapsing a burst of refreshes into one read.
pub const SCAN_INTERVAL_MS: u64 = 2000;

/// The directory every agent advertises itself into.
///
/// Deliberately re-derived here rather than imported. The bridge (writer) and
/// the orchestrator (reader) already define this path independently — mirroring
/// it is the established choice in this codebase, because the alternative is a
/// panel module depending on the orchestration crate purely to learn one path.
fn agents_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| Path::new(&home).join(".context-pilot").join("agents"))
}

/// The subset of a [`RetiredRecord`] this module needs.
///
/// Declared locally with two fields instead of importing the orchestrator's
/// type: serde ignores unknown fields, so this decodes the real file untouched,
/// and it keeps a panel module from depending on the backend crate.
///
/// [`RetiredRecord`]: https://docs.rs/  
#[derive(Debug, Deserialize)]
struct Retired {
    /// Folder-derived agent id, the same key the registry record uses.
    #[serde(default)]
    id: String,
    /// Realm path, matched as a fallback when ids drift.
    #[serde(default)]
    folder: String,
}

/// Ids and folders of every retired agent.
///
/// Retirement is "stop but keep": the agent's process dies while its record
/// stays on disk, so without this filter a retired agent would still appear in
/// the Agora. An unreadable or absent file yields an empty set — showing a
/// retired agent is a cosmetic wart, whereas failing the whole scan is not.
fn retired_keys(dir: &Path) -> (Vec<String>, Vec<String>) {
    let Ok(bytes) = std::fs::read(dir.join("retired.json")) else {
        return (Vec::new(), Vec::new());
    };
    let Ok(records) = serde_json::from_slice::<Vec<Retired>>(&bytes) else {
        return (Vec::new(), Vec::new());
    };
    let ids = records.iter().map(|r| r.id.clone()).collect();
    let folders = records.into_iter().map(|r| r.folder).collect();
    (ids, folders)
}

/// Convert the wire twin of the identity into the local one, field by field.
///
/// The explicit mapping is the point: `cp-wire` duplicates [`SelfIdentity`]
/// because it must stay dependency-free, so this is the seam where the two
/// copies could silently drift. Naming every field makes an added key a compile
/// error here rather than a value that quietly stops being rendered.
fn local_identity(wire: cp_wire::types::registry::SelfIdentity) -> SelfIdentity {
    SelfIdentity {
        identity: wire.identity,
        values: wire.values,
        principles: wire.principles,
        character: wire.character,
        expertise: wire.expertise,
        role: wire.role,
        operational_responsibilities: wire.operational_responsibilities,
        knowledge_responsibilities: wire.knowledge_responsibilities,
        organic_responsibilities: wire.organic_responsibilities,
        direct_management: wire.direct_management,
    }
}

/// The name to show for a peer whose record carries no slug.
///
/// Mirrors the fallback the orchestrator applies on its own read path, so one
/// agent is called the same thing in the dashboard and in this panel. A record
/// written before the profile existed has no slug at all, and an agent that
/// cleared its name has an empty one — both mean "use the default".
///
/// Takes the three fields rather than the whole record on purpose: the rule is
/// about a name, not about a registry entry, and `Entry` has no constructor
/// (it is an exhaustive 17-field literal), so a test of this rule would
/// otherwise have to fabricate an entire record to exercise one branch.
fn display_slug(slug: &str, folder: &str, id: &str) -> String {
    if slug.is_empty() {
        Path::new(folder).file_name().and_then(std::ffi::OsStr::to_str).unwrap_or(id).to_owned()
    } else {
        slug.to_owned()
    }
}

/// Read every peer's advertisement: all non-retired agents except this one.
///
/// Dead agents are deliberately **included**. Their records survive on purpose
/// (a stopped agent shows as disconnected rather than vanishing), and deciding
/// liveness properly needs the pid, the heartbeat and the boot id together —
/// the orchestrator's job. A naive pid check here would be cheap and *wrong*
/// under pid reuse, which is exactly the kind of quietly-lying field this
/// record was recently reworked to eliminate.
///
/// `self_folder` is the realm of the calling agent, excluded so the panel shows
/// peers rather than a mirror. Matching on the folder rather than the pid keeps
/// the exclusion stable across a reload, where the pid changes but the record
/// still briefly carries the old one.
#[must_use]
pub fn scan(self_folder: &str) -> Vec<PeerAgent> {
    let Some(dir) = agents_dir() else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
    let (retired_ids, retired_folders) = retired_keys(&dir);

    let mut peers: Vec<PeerAgent> = Vec::new();
    for dir_entry in entries.flatten() {
        let path = dir_entry.path();
        // The sidecar files (retired.json, agent-names.json, agent-avatars.json)
        // share this directory. They fail to decode as a record, which filters
        // them without needing a hardcoded list of names to keep in sync.
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let Ok(entry) = serde_json::from_slice::<cp_wire::types::registry::Entry>(&bytes) else { continue };

        if entry.folder == self_folder || retired_ids.contains(&entry.id) || retired_folders.contains(&entry.folder) {
            continue;
        }

        peers.push(PeerAgent {
            slug: display_slug(&entry.slug, &entry.folder, &entry.id),
            path: entry.folder.clone(),
            identity: entry.identity.map(local_identity).unwrap_or_default(),
        });
    }

    // Sorted so the panel does not reshuffle between reads: directory order is
    // unspecified, and a list that reorders itself is unreadable at a glance
    // and needlessly breaks the context cache.
    peers.sort_by(|a, b| a.slug.cmp(&b.slug));
    peers
}

/// This agent's own realm, canonicalised, for excluding itself from the scan.
///
/// The realm *is* the working directory for an agent, and the record stores a
/// canonical path, so both sides must be canonicalised or a symlinked home
/// (`/var` vs `/private/var` on macOS) would fail to match and the agent would
/// list itself as its own peer. A failure yields an empty string, which matches
/// no record — the agent then appears in its own fleet, a cosmetic wart rather
/// than a broken panel.
#[must_use]
pub fn self_folder() -> String {
    std::env::current_dir()
        .and_then(|dir| dir.canonicalize())
        .map(|dir| dir.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Wall-clock milliseconds, for throttling the scan.
#[must_use]
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A record whose slug is empty must fall back to the folder basename,
    /// matching what the dashboard shows for the same agent.
    #[test]
    fn empty_slug_falls_back_to_folder_basename() {
        assert_eq!(display_slug("", "/Users/someone/code/tessera", "abc123"), "tessera");
        assert_eq!(
            display_slug("Renamed", "/Users/someone/code/tessera", "abc123"),
            "Renamed",
            "an explicit slug wins over the default"
        );
        assert_eq!(display_slug("", "", "abc123"), "abc123", "a record with no folder falls back to the id");
    }

    /// The two identity copies must stay field-for-field aligned; a value that
    /// stopped being carried across would show as a blank row in the panel.
    #[test]
    fn identity_survives_the_wire_to_local_conversion() {
        let wire = cp_wire::types::registry::SelfIdentity {
            identity: "a".to_owned(),
            values: "b".to_owned(),
            principles: "c".to_owned(),
            character: "d".to_owned(),
            expertise: "e".to_owned(),
            role: "f".to_owned(),
            operational_responsibilities: "g".to_owned(),
            knowledge_responsibilities: "h".to_owned(),
            organic_responsibilities: "i".to_owned(),
            direct_management: "j".to_owned(),
        };
        let local = local_identity(wire);
        let values: Vec<&str> = local.pairs().into_iter().map(|(_, v)| v).collect();
        assert_eq!(values, ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]);
    }

    /// A missing retired.json is the normal case on a fresh box and must yield
    /// an empty filter rather than aborting the scan.
    #[test]
    fn absent_retired_file_yields_no_filter() {
        let (ids, folders) = retired_keys(Path::new("/nonexistent-agents-dir"));
        assert!(ids.is_empty() && folders.is_empty());
    }
}
