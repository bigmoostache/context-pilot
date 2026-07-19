//! Agent registry entry — the discovery record written at boot.
//!
//! An [`Entry`] is the JSON file an agent writes to
//! `~/.context-pilot/agents/<id>.json` on startup (design doc §10).  The
//! backend's `AgentRegistry` watches this directory and emits
//! appeared/disappeared/stale events.

use serde::{Deserialize, Serialize};

/// One agent's registration record, written atomically (tmp + rename) on
/// boot and updated only on status changes — **not** per heartbeat.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
}

/// Coarse agent status as recorded in the registry.
///
/// The backend combines this with heartbeat liveness and pid checks to
/// derive a richer verdict (design doc §10).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[expect(
    clippy::exhaustive_enums,
    reason = "wire-protocol contract: AgentStatus is a closed lifecycle set written cross-crate into the registry entry and matched exhaustively by the backend; #[non_exhaustive] would forbid that construction"
)]
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
            schema_version: 1,
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
}
