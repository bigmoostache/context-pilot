//! Agent registry entry — the discovery record an agent projects about itself.
//!
//! An [`Entry`] is the JSON file an agent writes to
//! `~/.context-pilot/agents/<id>.json` (design doc §10).  The backend's
//! `AgentRegistry` watches this directory and emits appeared/disappeared/stale
//! events.
//!
//! # A projection, not a boot snapshot
//!
//! The record used to be written **once** at boot and never again, which made
//! every mutable field a lie the moment it changed: `status` stayed `Starting`
//! forever, and `model` snapshotted one provider's model regardless of the live
//! one (the backend grew documented workarounds for both).
//!
//! It is now a **projection of live agent state**, re-derived on the agent's
//! ordinary persistence cadence and written only when the serialised bytes
//! actually change (see `cp-mod-bridge`'s `advert` module).  A field cannot go
//! stale by a mutation site forgetting to re-publish, because no site
//! publishes — the projection simply reflects current state.

use serde::{Deserialize, Serialize};

/// Current registry-record schema revision.
///
/// * **1** — the original boot-time snapshot.
/// * **2** — the live projection, adding `slug`, `image` and `identity`.
///
/// Bumped only for shape changes. Readers must not gate on it (decoding is
/// tolerant in both directions); it exists so a record's provenance is legible
/// and so a future migration has a discriminator.
pub const SCHEMA_VERSION: u32 = 2;

/// One agent's registration record: a dirty-checked projection of live agent
/// state, written atomically (tmp + rename) at `0600`.
///
/// # Compatibility
///
/// Decoding is tolerant (unknown fields are ignored, see
/// `registry_extra_fields_ignored`) and every field added after
/// `schema_version` 1 carries `#[serde(default)]`, so a new binary reads an old
/// record and an old binary reads a new one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[expect(
    clippy::exhaustive_structs,
    reason = "registry-record contract: Entry is a 14-field agent discovery record written cross-crate by the bridge boot path and read field-by-field by the backend; a constructor would take 13 positional arguments (tripping too_many_arguments) with no natural grouping, so #[non_exhaustive] is impossible and the exhaustive literal is the honest shape"
)]
pub struct Entry {
    /// Wire-schema revision for this struct.
    pub schema_version: u32,

    /// Stable agent identifier (FNV-1a of the canonical folder path).
    pub id: String,

    /// Canonical absolute path of the agent's realm folder.
    pub folder: String,

    /// OS process id of the agent.
    pub pid: u32,

    /// Random token minted per boot — disambiguates pid reuse.
    pub boot_id: String,

    /// LLM model the agent is configured to use.
    pub model: String,

    /// Wire protocol major version the agent speaks.
    pub protocol_version: u32,

    /// Binary version string (e.g. `"0.2.10"`).
    pub binary_version: String,

    /// Path to the agent's UDS stream socket.
    pub socket_path: String,

    /// Path to the agent's oplog directory.
    pub oplog_path: String,

    /// Path to the agent's heartbeat file.
    pub heartbeat_path: String,

    /// Bearer capability token for command authn (I9, `0600`).
    pub cap_token: String,

    /// Milliseconds since the Unix epoch when the agent started.
    pub started_at_ms: u64,

    /// Current agent status.
    pub status: AgentStatus,

    /// Human-readable handle for the agent, defaulting to the realm folder's
    /// basename (the same default the backend derives).  Added in
    /// `schema_version` 2.
    ///
    /// The dashboard's per-agent name override still wins in the backend's
    /// projection — this is the agent's own view of what it is called, which is
    /// what a *peer* agent reading the directory needs.
    #[serde(default)]
    pub slug: String,

    /// Reference to the agent's avatar — a filesystem path or a URL, empty when
    /// unset.  Added in `schema_version` 2.
    ///
    /// Deliberately a **reference, not bytes**: the image itself stays in the
    /// backend's avatar store.  A multi-megabyte blob has no business inside a
    /// `0600` discovery record that every fleet scan parses.
    #[serde(default)]
    pub image: String,

    /// The agent's durable self-identity (the Agora fixed-key value store), or
    /// `None` when the agent has never introduced itself.  Added in
    /// `schema_version` 2.
    ///
    /// Advertised here so an agent discovering its peers learns *who they are*
    /// from the same single read that discovers *that they are* — rather than
    /// needing a per-agent round trip through the backend.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<SelfIdentity>,
}

/// An agent's self-identity as advertised in its registry record — the ten
/// fixed keys of the Agora value store, mirrored 1:1 from the agent's own
/// `SelfIdentity` (same name, deliberately).
///
/// Duplicated into the wire crate (rather than depending on `cp-agora`) because
/// `cp-wire` is deliberately dependency-free: it is the shared vocabulary both
/// sides compile against, and the backend must decode this without linking the
/// agent's module tree.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[expect(
    clippy::exhaustive_structs,
    reason = "registry-record contract: SelfIdentity is a closed set of ten fixed keys mirroring the Agora tool parameters; a constructor would take ten positional String arguments (tripping too_many_arguments) with no natural grouping, so the exhaustive literal is the honest shape"
)]
pub struct SelfIdentity {
    /// Who the agent fundamentally is.
    #[serde(default)]
    pub identity: String,
    /// Core values the agent holds.
    #[serde(default)]
    pub values: String,
    /// Operating principles the agent follows.
    #[serde(default)]
    pub principles: String,
    /// Behavioural character / temperament.
    #[serde(default)]
    pub character: String,
    /// Domains of competence.
    #[serde(default)]
    pub expertise: String,
    /// The agent's functional role.
    #[serde(default)]
    pub role: String,
    /// Day-to-day operational duties.
    #[serde(default)]
    pub operational_responsibilities: String,
    /// Duties as a source of knowledge / truth.
    #[serde(default)]
    pub knowledge_responsibilities: String,
    /// Who the agent is responsible *for* (subordinates).
    #[serde(default)]
    pub organic_responsibilities: String,
    /// Who directs the agent — the source of authority.
    #[serde(default)]
    pub direct_management: String,
}

/// Coarse agent status as recorded in the registry.
///
/// The backend combines this with heartbeat liveness and pid checks to
/// derive a richer verdict (design doc §10).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AgentStatus {
    /// Agent is booting (bridge init in progress).
    Starting,
    /// Fully operational.
    Running,
    /// Graceful shutdown in progress.
    Stopping,
    /// Agent has exited (registry entry may linger until reaped).
    Down,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to build a minimal valid entry for tests.
    fn sample_entry() -> Entry {
        Entry {
            schema_version: SCHEMA_VERSION,
            id: "abc123".into(),
            folder: "/home/user/project".into(),
            pid: 12345,
            boot_id: "b-001".into(),
            model: "claude-opus-4-8".into(),
            protocol_version: 1,
            binary_version: "0.2.10".into(),
            socket_path: "/home/user/project/.context-pilot/stream.sock".into(),
            oplog_path: "/home/user/project/.context-pilot/oplog".into(),
            heartbeat_path: "/home/user/project/.context-pilot/heartbeat".into(),
            cap_token: "tok-secret-256bit".into(),
            started_at_ms: 1_718_000_000_000,
            status: AgentStatus::Running,
            slug: "project".into(),
            image: "/home/user/project/.context-pilot/avatar.png".into(),
            identity: Some(SelfIdentity { identity: "a builder".into(), ..SelfIdentity::default() }),
        }
    }

    #[test]
    fn registry_entry_round_trip() {
        let entry = sample_entry();
        let json = serde_json::to_string(&entry).expect("serialize");
        let back: Entry = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(entry, back);
    }

    #[test]
    fn agent_status_round_trip() {
        for status in [AgentStatus::Starting, AgentStatus::Running, AgentStatus::Stopping, AgentStatus::Down] {
            let json = serde_json::to_string(&status).expect("serialize");
            let back: AgentStatus = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(status, back);
        }
    }

    #[test]
    fn registry_extra_fields_ignored() {
        let json = r#"{
            "schema_version": 1,
            "id": "x",
            "folder": "/tmp",
            "pid": 1,
            "boot_id": "b",
            "model": "m",
            "protocol_version": 1,
            "binary_version": "0.1.0",
            "socket_path": "/s",
            "oplog_path": "/o",
            "heartbeat_path": "/h",
            "cap_token": "t",
            "started_at_ms": 0,
            "status": "running",
            "future_field": 42
        }"#;
        let entry: Entry = serde_json::from_str(json).expect("tolerant decode");
        assert_eq!(entry.status, AgentStatus::Running);
    }

    /// A `schema_version` 1 record — written by a binary that predates
    /// slug/image/identity — must still decode, with the new fields falling back
    /// to their defaults. This is the N-1 direction: a NEW backend reading an
    /// OLD agent's record.
    #[test]
    fn schema_v1_record_decodes_with_defaults() {
        let json = r#"{
            "schema_version": 1,
            "id": "x",
            "folder": "/tmp/realm",
            "pid": 1,
            "boot_id": "b",
            "model": "m",
            "protocol_version": 1,
            "binary_version": "0.1.0",
            "socket_path": "/s",
            "oplog_path": "/o",
            "heartbeat_path": "/h",
            "cap_token": "t",
            "started_at_ms": 0,
            "status": "running"
        }"#;
        let entry: Entry = serde_json::from_str(json).expect("v1 record still decodes");
        assert_eq!(entry.schema_version, 1, "the record's own version is preserved, not rewritten");
        assert!(entry.slug.is_empty(), "absent slug defaults");
        assert!(entry.image.is_empty(), "absent image defaults");
        assert!(entry.identity.is_none(), "absent identity defaults");
    }

    /// An agent that has never introduced itself omits `identity` from the JSON
    /// entirely (rather than emitting a null or ten empty strings), so an old
    /// reader sees exactly the v1 shape it expects.
    #[test]
    fn absent_identity_is_omitted_from_json() {
        let mut entry = sample_entry();
        entry.identity = None;
        let json = serde_json::to_string(&entry).expect("serialize");
        assert!(!json.contains("identity"), "no identity key when unset: {json}");
    }

    #[test]
    fn identity_round_trips() {
        let entry = sample_entry();
        let back: Entry = serde_json::from_str(&serde_json::to_string(&entry).expect("ser")).expect("de");
        assert_eq!(back.identity, entry.identity);
        assert_eq!(back.slug, "project");
        assert_eq!(back.image, "/home/user/project/.context-pilot/avatar.png");
    }
}
