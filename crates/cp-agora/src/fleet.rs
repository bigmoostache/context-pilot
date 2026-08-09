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

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash as _, Hasher as _};
use std::path::{Path, PathBuf};

use serde::Deserialize;

use cp_base::state::runtime::State;

use crate::types::{AgoraState, PeerAgent, SelfIdentity};

/// Minimum gap between two directory *polls*, in milliseconds.
///
/// A poll is a fingerprint, not a scan: one `scandir` plus a `stat` per entry,
/// measured at ~46 µs on a sixteen-agent box. The full read-and-parse it guards
/// costs ~9× that, and now runs only when the fingerprint moved.
///
/// 250 ms is chosen so a peer's rename appears while the human is still looking
/// at the panel. Steady-state cost is nonetheless *lower* than the 2 s
/// unguarded scan this replaces (46 µs per 250 ms against 434 µs per 2 s) —
/// faster and cheaper at once, which is what makes the change uncontroversial.
pub const POLL_INTERVAL_MS: u64 = 250;

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

/// Wall-clock milliseconds, for throttling the poll.
#[must_use]
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// A cheap fingerprint of the agents directory: one `stat` per entry, with no
/// file opened and nothing parsed.
///
/// This is the dirty check that makes the fleet poll nearly free — the same
/// shape the *write* half of the advertisement already uses, where a
/// serialise-and-compare guards an expensive atomic write. Here a ~46 µs
/// fingerprint guards a ~434 µs read-and-parse, and the fleet only changes when
/// a human boots, renames or retires an agent.
///
/// # Why the fold is order-independent
///
/// Directory iteration order is unspecified, so hashing entries in traversal
/// order could yield different values for a byte-identical directory and
/// trigger endless spurious rescans. Each entry is hashed alone and the results
/// are summed, making the fingerprint a property of the *set*. The file name is
/// inside each entry's hash, so an addition, a removal and a rename all move
/// the sum.
///
/// # Why `retired.json` is covered for free
///
/// It lives in this same directory, so retiring an agent moves the fingerprint
/// like any other change. That is load-bearing rather than incidental: an
/// "optimisation" that fingerprinted only the `<id>.json` records would make
/// retirement invisible until something else happened to change.
///
/// # Why mtime is trustworthy *here*
///
/// Records are published `tmp → fsync → rename`, so the visible file is always
/// a fresh inode with a fresh timestamp — there is no in-place overwrite that
/// could leave mtime untouched while the bytes changed. The length rides along
/// as a second signal. If that write ever becomes in-place (as the heartbeat
/// beacon deliberately is, to avoid rename churn) this guard weakens and would
/// need revisiting.
///
/// The hash is compared only against another taken in the same process run,
/// never persisted — which is what makes [`DefaultHasher`], whose output is not
/// stable across Rust releases, a legitimate choice.
fn fingerprint(dir: &Path) -> Option<u64> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut sum: u64 = 0;
    for dir_entry in entries.flatten() {
        let Ok(meta) = dir_entry.metadata() else { continue };
        let mut hasher = DefaultHasher::new();
        dir_entry.file_name().hash(&mut hasher);
        meta.len().hash(&mut hasher);
        // A clock that cannot be read is folded in as a constant: the length
        // and the name still move on any real change, so this degrades the
        // signal rather than losing the entry.
        if let Ok(mtime) = meta.modified() {
            match mtime.duration_since(std::time::UNIX_EPOCH) {
                Ok(since) => since.as_nanos().hash(&mut hasher),
                Err(_) => 0u128.hash(&mut hasher),
            }
        }
        sum = sum.wrapping_add(hasher.finish());
    }
    Some(sum)
}

/// Refresh the cached fleet if the agents directory changed — the main-loop
/// entry point for the *read* half of the advertisement.
///
/// Called from the background phase on every tick, deliberately **beside** the
/// bridge emitters rather than among them: that group is contractually inert
/// while the bridge is off, whereas reading peers must work regardless — an
/// agent run without a bridge still has an Agora panel and still has neighbours
/// on disk.
///
/// Two guards, cheapest first: a wall-clock throttle
/// ([`POLL_INTERVAL_MS`]), then the directory fingerprint. Only a moved
/// fingerprint reaches the parse.
///
/// The throttle lives here rather than at the call site so the loop carries no
/// knowledge of this module's timing, and so the constant sits beside the
/// comparison that uses it.
pub fn poll(state: &mut State) {
    let now = now_ms();
    let agora = AgoraState::get(state);
    if agora.fleet_scanned_ms != 0 && now.saturating_sub(agora.fleet_scanned_ms) < POLL_INTERVAL_MS {
        return;
    }

    let Some(dir) = agents_dir() else { return };
    let print = fingerprint(&dir);

    // An unreadable directory yields `None`, which falls through to a full scan
    // rather than freezing the last known fleet. A cache that outlives its own
    // source is exactly the kind of quiet lie this subsystem exists to avoid:
    // if the agents directory really is gone, the panel must go empty.
    if print.is_some() && print == agora.fleet_fingerprint {
        AgoraState::get_mut(state).fleet_scanned_ms = now;
        return;
    }

    let peers = scan(&self_folder());
    let target = AgoraState::get_mut(state);
    target.fleet = peers;
    target.fleet_fingerprint = print;
    target.fleet_scanned_ms = now;
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

    /// The whole optimisation rests on this: an untouched directory must
    /// fingerprint identically every time, or the guard degrades into an
    /// unconditional rescan and buys nothing.
    #[test]
    fn an_unchanged_directory_fingerprints_identically() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("a.json"), b"{}").expect("write");
        let first = fingerprint(dir.path());
        assert_eq!(first, fingerprint(dir.path()), "no change means no rescan");
        assert!(first.is_some(), "a readable directory always fingerprints");
    }

    /// Appearing, vanishing and being rewritten are the three ways the fleet
    /// changes; each has to move the fingerprint or the panel silently freezes
    /// on a stale peer list.
    #[test]
    fn every_kind_of_change_moves_the_fingerprint() {
        let dir = tempfile::tempdir().expect("tempdir");
        let record = dir.path().join("a.json");
        std::fs::write(&record, b"{}").expect("write");
        let base = fingerprint(dir.path());

        std::fs::write(dir.path().join("b.json"), b"{}").expect("write");
        let added = fingerprint(dir.path());
        assert_ne!(base, added, "an agent booting must be noticed");

        // Length alone carries this one: mtime granularity is coarse enough
        // that a rewrite within the same tick could otherwise look identical.
        std::fs::write(&record, b"{\"slug\":\"renamed\"}").expect("rewrite");
        let edited = fingerprint(dir.path());
        assert_ne!(added, edited, "a rename must be noticed");

        std::fs::remove_file(&record).expect("remove");
        assert_ne!(edited, fingerprint(dir.path()), "an agent vanishing must be noticed");
    }

    /// An unreadable directory yields `None`, which the poll treats as "scan
    /// anyway" rather than "keep the cache" — a fleet cache that outlives its
    /// own source would be exactly the quiet lie this subsystem exists to kill.
    #[test]
    fn a_missing_directory_has_no_fingerprint() {
        assert!(fingerprint(Path::new("/nonexistent-agents-dir")).is_none());
    }
}
