use serde::{Deserialize, Serialize};

use cp_base::state::runtime::State;

/// Advertised soft word ceiling shown in tool param descriptions.
///
/// The model is asked to keep every identity value under this many words.
/// Deliberately LOWER than the hard reject limit ([`IDENTITY_VALUE_HARD_CAP`])
/// so a marginal overshoot still passes — the model routinely overshoots a
/// stated cap by a little.
pub const IDENTITY_VALUE_ADVERTISED_WORDS: usize = 8;

/// The REAL enforced word ceiling: `Agora_set_identity` rejects above this.
///
/// Never surfaced to the model (it only ever sees the advertised 8), so
/// self-trimming aims well below and marginal overruns pass.
pub const IDENTITY_VALUE_HARD_CAP: usize = 10;

/// The agent's durable self-identity — a fixed-key value store, global (main
/// state, not per-worker) so it is one shared truth across every worker.
///
/// Each field is a short free-text value (< 10 words, enforced at the tool
/// boundary). The keys are a closed set mirrored 1:1 by the
/// `Agora_set_identity` tool parameters.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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

impl SelfIdentity {
    /// Ordered (key, value) pairs for YAML-style rendering + context. The order
    /// is the canonical declaration order, matching the tool parameters and the
    /// panel layout.
    #[must_use]
    pub fn pairs(&self) -> [(&'static str, &str); 10] {
        [
            ("identity", &self.identity),
            ("values", &self.values),
            ("principles", &self.principles),
            ("character", &self.character),
            ("expertise", &self.expertise),
            ("role", &self.role),
            ("operational_responsibilities", &self.operational_responsibilities),
            ("knowledge_responsibilities", &self.knowledge_responsibilities),
            ("organic_responsibilities", &self.organic_responsibilities),
            ("direct_management", &self.direct_management),
        ]
    }

    /// True when no field has been set yet (fresh agent, never introduced).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pairs().into_iter().all(|(_, v)| v.is_empty())
    }
}

/// Maximum accepted length of a display [`slug`](AgoraState::slug).
///
/// Generous for a display name while keeping the registry record small — that
/// record is `0600`, re-read on every fleet scan, and a runaway slug would be
/// paid for on each pass.
pub const SLUG_MAX_CHARS: usize = 128;

/// Maximum accepted length of an [`image`](AgoraState::image) reference.
///
/// Sized for a long URL rather than a path; the bytes themselves never live in
/// the record, only this reference.
pub const IMAGE_MAX_CHARS: usize = 2048;

/// One *other* agent, as read from its own advertisement in the shared
/// registry directory.
///
/// Carries only what the Agora shows — who the agent is, where it lives, and
/// what it says about itself. The record holds a great deal more (sockets,
/// tokens, pids); none of it belongs in a panel about identity, and a bearer
/// token in particular has no business being rendered.
#[derive(Debug, Clone, Default)]
pub struct PeerAgent {
    /// The peer's display name, already resolved (an empty slug in the record
    /// has been replaced by the folder basename, matching the dashboard).
    pub slug: String,
    /// The peer's realm — the absolute canonical folder it runs in.
    pub path: String,
    /// The peer's self-identity. Empty for an agent that has never introduced
    /// itself, or whose record predates identity being advertised at all.
    pub identity: SelfIdentity,
}

/// Module-owned state for the Agora module: the agent's self-identity plus the
/// public profile (slug + image) it advertises to the fleet.
///
/// The two halves are deliberately separate. [`identity`](Self::identity) is
/// *prose the agent holds about itself* and rides the LLM context; the profile
/// is *how humans see the agent* in the dashboard and rides the registry
/// record. Keeping the profile out of [`SelfIdentity`] preserves that type's
/// closed ten-key shape (mirrored 1:1 by the tool parameters and the `cp-wire`
/// twin) and keeps two unrelated concerns from sharing one struct.
#[derive(Debug, Default)]
pub struct AgoraState {
    /// The agent's current self-identity.
    pub identity: SelfIdentity,
    /// Display slug — the agent's public name. Empty means "unset": consumers
    /// fall back to the folder-derived basename, so clearing this reverts to
    /// the default rather than blanking the name.
    pub slug: String,
    /// Image reference — a realm-relative path or an `http(s)` URL, never the
    /// bytes. Empty means no picture.
    pub image: String,
    /// The other agents currently advertising themselves, refreshed from disk
    /// on a throttle.
    ///
    /// Derived state, never persisted: it is a cache of what the neighbours
    /// published, and writing it into this agent's own config would be storing
    /// someone else's truth under our name — stale the moment they change it.
    pub fleet: Vec<PeerAgent>,
    /// When the fleet was last read, for throttling (see
    /// [`SCAN_INTERVAL_MS`](crate::fleet::SCAN_INTERVAL_MS)).
    pub fleet_scanned_ms: u64,
}

impl AgoraState {
    /// Construct empty state.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            identity: SelfIdentity {
                identity: String::new(),
                values: String::new(),
                principles: String::new(),
                character: String::new(),
                expertise: String::new(),
                role: String::new(),
                operational_responsibilities: String::new(),
                knowledge_responsibilities: String::new(),
                organic_responsibilities: String::new(),
                direct_management: String::new(),
            },
            slug: String::new(),
            image: String::new(),
            fleet: Vec::new(),
            fleet_scanned_ms: 0,
        }
    }

    /// Get shared ref from State's `TypeMap`.
    ///
    /// # Panics
    ///
    /// Panics if an internal invariant is violated.
    #[must_use]
    pub fn get(state: &State) -> &Self {
        state.ext::<Self>()
    }

    /// Get mutable ref from State's `TypeMap`.
    ///
    /// # Panics
    ///
    /// Panics if an internal invariant is violated.
    pub fn get_mut(state: &mut State) -> &mut Self {
        state.ext_mut::<Self>()
    }
}
