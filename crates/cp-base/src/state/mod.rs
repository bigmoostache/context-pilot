/// Action and result types for the event-driven dispatch loop.
pub mod actions;
/// File-path autocomplete state for @-triggered popup.
pub mod autocomplete;
/// Context types, elements, and token estimation.
pub mod context;
/// Serializable data structures: config, messages, persistence types.
pub mod data;
/// Stream-phase state machine, boolean flag structs, and streaming-tool advisory state.
pub mod flags;
/// Runtime state: the in-memory `State` struct with all live fields.
pub mod runtime;
/// Watcher trait and registry for async condition monitoring.
pub mod watchers;

// ─── Reverie State ──────────────────────────────────────────────────────────
// Ephemeral sub-agent state — lives as Option<reverie::Session> on the main State.

/// Ephemeral reverie sub-agent state (context optimizer, cartographer).
pub mod reverie {
    use super::data::message::Message;

    /// The kind of reverie running.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Kind {
        /// Context optimizer — reshapes context for relevance and budget.
        ContextOptimizer,
    }

    /// Ephemeral state for an active reverie session.
    ///
    /// Lives as `Option<reverie::Session>` on the main `State` struct.
    /// Not persisted — discarded after each run (fresh start every time).
    #[derive(Debug, Clone)]
    pub struct Session {
        /// What kind of reverie this is.
        pub kind: Kind,
        /// Agent ID driving this reverie (e.g., "cleaner"). The agent's content
        /// is injected into the P-reverie panel, NOT as a system prompt.
        pub agent_id: String,
        /// Optional additional context from the caller (e.g., "focus on UI files").
        pub context: Option<String>,
        /// The reverie's own conversation (separate from main chat).
        pub messages: Vec<Message>,
        /// Number of tool calls executed this run (for guard rail cap).
        pub tool_call_count: usize,
        /// Whether the reverie LLM stream is currently active.
        pub is_streaming: bool,
        /// How many times we've auto-relaunched for missing Report (max 1).
        pub report_retries: usize,
        /// Whether this reverie's tool calls should be queued (RAM-only, not persisted).
        pub queue_active: bool,
    }

    impl Session {
        /// Create a new reverie session driven by the given agent.
        #[must_use]
        pub const fn new(kind: Kind, agent_id: String, context: Option<String>) -> Self {
            Self {
                kind,
                agent_id,
                context,
                messages: Vec::new(),
                tool_call_count: 0,
                is_streaming: true,
                report_retries: 0,
                queue_active: false,
            }
        }
    }

    impl std::fmt::Display for Kind {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::ContextOptimizer => write!(f, "Context Optimizer"),
            }
        }
    }
}
