//! Bounded snapshot heads — the efficient alternative to full enumeration.
//!
//! [`Heads`] captures the current state of an agent at a given `rev` as a
//! bounded set of per-thread and per-panel content hashes, not a manifest
//! of every file.  This keeps snapshot cost **O(threads + panels)** instead
//! of O(total-files) (design doc I3, resolves K3).

use serde::{Deserialize, Serialize};

use super::ContentHash;

/// Snapshot of an agent's current heads at a specific `rev`.
///
/// Each head is a content-addressed reference into the immutable body
/// store; hydrating a snapshot means fetching bodies by hash on demand
/// (lazy, rev-pinned — design doc I5).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Heads {
    /// Wire-schema revision for this struct.
    pub schema_version: u32,

    /// Per-thread last-message head.
    pub threads: Vec<ThreadHead>,

    /// Per-panel content head.
    pub panels: Vec<PanelHead>,
}

/// A single thread's head — the hash of its most recent message body.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThreadHead {
    /// Thread identifier.
    pub thread_id: String,

    /// Content hash of the last message body in this thread.
    pub last_message_hash: ContentHash,
}

/// A single panel's head — the hash of its serialised content.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PanelHead {
    /// Panel identifier.
    pub panel_id: String,

    /// Content hash of the panel's current serialised state.
    pub hash: ContentHash,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heads_round_trip() {
        let heads = Heads {
            schema_version: 1,
            threads: vec![ThreadHead {
                thread_id: "T1".into(),
                last_message_hash: ContentHash::new([0x11; 32]),
            }],
            panels: vec![PanelHead {
                panel_id: "P5".into(),
                hash: ContentHash::new([0x22; 32]),
            }],
        };
        let json = serde_json::to_string(&heads).expect("serialize");
        let back: Heads = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(heads, back);
    }

    #[test]
    fn empty_heads_round_trip() {
        let heads = Heads {
            schema_version: 1,
            threads: vec![],
            panels: vec![],
        };
        let json = serde_json::to_string(&heads).expect("serialize");
        let back: Heads = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(heads, back);
    }
}
