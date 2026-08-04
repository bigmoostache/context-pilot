//! The registry record as a **live projection** of agent state (T739).
//!
//! # Why a projection
//!
//! The record at `~/.context-pilot/agents/<id>.json` used to be written once,
//! at boot, and never again. Every mutable field it carried therefore became a
//! lie the moment it changed, and the backend grew documented workarounds for
//! exactly that: it ignores `status` (permanently `Starting`) in favour of a
//! liveness verdict, and `transport/inspect/meta.rs` calls `entry.model`
//! "unreliable" and reads the agent's `config.json` instead.
//!
//! The alternative to a projection is a *re-publish chokepoint* called from
//! every mutation site — which only works if every site is found. That is
//! audit-dependent, and an audit can be wrong; a forgotten site produces
//! exactly the silent staleness above.
//!
//! So the record is a projection instead: [`project`] re-derives it from live
//! state, and the caller writes it **only when the serialised bytes actually
//! change**. No mutation site has to remember anything, because no site
//! publishes — a field cannot go stale by omission.
//!
//! # Why the dirty check matters
//!
//! [`write_entry`](super::registry::write_entry) is `tmp → fsync → rename →
//! fsync(dir)`: two fsyncs plus a directory mutation. That is cheap when rare
//! and wasteful when constant, and the agents directory has a *live reader*
//! (the backend's `AgentRegistry::scan` poll, plus `reap_tmp` deleting stale
//! `*.tmp`). The sibling [`heartbeat`](crate::heartbeat) module documents the
//! same hazard, which is why it overwrites in place instead of renaming.
//!
//! Diffing against the last-written serialisation keeps the atomic write (so a
//! scan can never observe a torn record) while making an idle agent cost
//! **zero syscalls**: identity, folder, sockets and token simply do not change
//! between ticks.

use std::path::Path;

use cp_wire::types::registry::{AgentStatus, Entry, SelfIdentity};

/// Re-derive the agent's registry record from live state.
///
/// Boot-time facts (id, folder, pid, sockets, token, start time) are carried
/// over verbatim from the record boot minted — they genuinely cannot change
/// while the process lives. Everything else is re-derived from the caller's
/// live values on every call, which is precisely what stops them going stale:
///
/// * `status` becomes [`Running`](AgentStatus::Running) once the agent is up,
///   instead of remaining `Starting` forever;
/// * `model` reflects the model the agent is *currently* configured with,
///   killing the backend's `entry.model`-is-unreliable workaround;
/// * `identity` mirrors the Agora self-identity, whatever last wrote it — the
///   `Agora_set_identity` tool or the web `SetIdentity` command.
///
/// * `slug` and `image` mirror the agent's own public profile, which it now
///   **owns** (T739b): a dashboard rename or avatar change arrives as a
///   `SetProfile` command, lands in the agent's Agora state, and reaches the
///   record through this projection like every other live field.
///
/// Every live input arrives **by value** rather than being read from a `&State`
/// here. That is deliberate: the caller holds a mutable borrow of the agent
/// state (to reach the `Boot` that owns this record), so it must resolve them
/// before taking it. Passing them in also keeps this module pure data-in /
/// data-out, with no dependency on the agent's state tree.
///
/// An **empty** slug falls back to the boot record's folder-derived basename
/// rather than advertising nothing. That is what makes clearing a rename mean
/// "revert to the default name" instead of "blank the name", and it keeps the
/// fallback in one place — every reader can then trust `slug` to be populated.
/// An empty `image` is passed through as empty, because "no picture" is a
/// legitimate final state with no default to fall back to.
///
/// The advertised `identity` is dropped to `None` while every Agora field is
/// still empty, so a fresh agent that has never introduced itself serialises to
/// exactly the shape an older reader expects rather than ten empty strings.
/// That decision lives here rather than in the caller: the caller can always
/// produce an identity (a blank one is still a value), so making it choose
/// would push a projection rule out into the observer.
#[must_use]
pub fn project(profile: Profile, booted: &Entry, identity: SelfIdentity) -> Entry {
    let advertised = if is_blank(&identity) { None } else { Some(identity) };
    let slug = if profile.slug.is_empty() { booted.slug.clone() } else { profile.slug };
    Entry {
        model: profile.model,
        status: AgentStatus::Running,
        slug,
        image: profile.image,
        identity: advertised,
        ..booted.clone()
    }
}

/// The live, agent-owned values a projection tick folds into the record.
///
/// Grouped into one struct rather than passed as three loose `String`
/// arguments because they share a single origin (the agent's own state,
/// resolved together just before the projection) and because naming them at
/// the construction site is the only defence against silent transposition —
/// every field is a `String`, so the compiler could not catch a swapped `slug`
/// and `image`.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Profile {
    /// The model the agent is *currently* configured with.
    pub model: String,
    /// The agent's display slug — empty means "unset", and the projection then
    /// falls back to the boot record's folder-derived default.
    pub slug: String,
    /// The agent's image reference (realm-relative path or URL), never bytes.
    /// Empty means no picture.
    pub image: String,
}

impl Profile {
    /// Bundle the live profile values for a projection tick.
    ///
    /// A constructor keeps [`Profile`] `#[non_exhaustive]` while still letting
    /// the agent crate build one, so a field added here later is a compile
    /// error at this signature rather than a silently defaulted record field.
    #[must_use]
    pub const fn new(model: String, slug: String, image: String) -> Self {
        Self { model, slug, image }
    }
}

/// Whether every identity field is empty — an agent that has never introduced
/// itself, which is advertised as an absent `identity` rather than ten empty
/// strings.
///
/// The fields are listed explicitly rather than destructured because both a
/// `let SelfIdentity { .. } = identity` binding (`pattern_type_mismatch`) and
/// its `&`-pattern remedy (`ref_patterns`) are forbidden lints here.
fn is_blank(identity: &SelfIdentity) -> bool {
    [
        &identity.identity,
        &identity.values,
        &identity.principles,
        &identity.character,
        &identity.expertise,
        &identity.role,
        &identity.operational_responsibilities,
        &identity.knowledge_responsibilities,
        &identity.organic_responsibilities,
        &identity.direct_management,
    ]
    .iter()
    .all(|field| field.is_empty())
}

/// Publish `entry` if its serialisation differs from `last` — the dirty check
/// that makes the projection nearly free.
///
/// On a change the record is written through the ordinary atomic `0600` path
/// and `last` is updated to the new serialisation. When nothing changed this
/// performs **no I/O at all**, which is the steady state: an idle agent
/// re-derives an identical record every tick and writes none of them.
///
/// Returns `true` when a write actually happened.
///
/// # Errors
///
/// A failed write is reported to the caller, which treats advertising as
/// best-effort: the stale record on disk stays valid (it is a projection, so
/// the next tick simply retries) and a missed update self-heals.
pub fn publish_if_changed(
    agents_dir: &Path,
    entry: &Entry,
    last: &mut Option<String>,
) -> crate::error::BootResult<bool> {
    let serialised = serde_json::to_string(entry)
        .map_err(|e| crate::error::Error::io("serialise advert", std::io::Error::other(e)))?;
    if last.as_deref() == Some(serialised.as_str()) {
        return Ok(false);
    }
    let _written = super::registry::write_entry(agents_dir, entry)?;
    *last = Some(serialised);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cp_wire::types::registry::SCHEMA_VERSION;
    use tempfile::tempdir;

    fn booted() -> Entry {
        Entry {
            schema_version: SCHEMA_VERSION,
            id: "abc".to_owned(),
            folder: "/realm".to_owned(),
            pid: 1,
            boot_id: "b".to_owned(),
            model: "boot-model".to_owned(),
            protocol_version: 1,
            binary_version: "0.1.0".to_owned(),
            socket_path: "/s".to_owned(),
            oplog_path: "/o".to_owned(),
            heartbeat_path: "/h".to_owned(),
            cap_token: "secret".to_owned(),
            started_at_ms: 7,
            status: AgentStatus::Starting,
            slug: "realm".to_owned(),
            image: String::new(),
            identity: None,
        }
    }

    #[test]
    fn blank_identity_is_not_advertised() {
        assert!(is_blank(&SelfIdentity::default()), "a never-introduced agent is blank");
        let named = SelfIdentity { role: "builder".to_owned(), ..SelfIdentity::default() };
        assert!(!is_blank(&named), "one populated field is enough to advertise");
    }

    #[test]
    fn unchanged_projection_writes_nothing() {
        let dir = tempdir().expect("tempdir");
        let entry = booted();
        let mut last = None;

        assert!(publish_if_changed(dir.path(), &entry, &mut last).expect("first"), "first publish writes");
        assert!(!publish_if_changed(dir.path(), &entry, &mut last).expect("second"), "identical bytes write nothing");
    }

    #[test]
    fn changed_projection_rewrites_the_record() {
        let dir = tempdir().expect("tempdir");
        let mut entry = booted();
        let mut last = None;
        let _first = publish_if_changed(dir.path(), &entry, &mut last).expect("first");

        entry.status = AgentStatus::Running;
        assert!(publish_if_changed(dir.path(), &entry, &mut last).expect("second"), "a changed field rewrites");

        let path = super::super::registry::path(dir.path(), &entry.id);
        let raw = std::fs::read(&path).expect("read back");
        let back: Entry = serde_json::from_slice(&raw).expect("parse");
        assert_eq!(back.status, AgentStatus::Running, "the record on disk reflects the projection");
    }

    #[test]
    fn republished_record_keeps_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempdir().expect("tempdir");
        let mut entry = booted();
        let mut last = None;
        let _first = publish_if_changed(dir.path(), &entry, &mut last).expect("first");
        entry.model = "next".to_owned();
        let _second = publish_if_changed(dir.path(), &entry, &mut last).expect("second");

        let path = super::super::registry::path(dir.path(), &entry.id);
        let mode = std::fs::metadata(&path).expect("metadata").permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "the record carries cap_token — every rewrite stays owner-only");
    }
}
