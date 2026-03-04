pub mod actions;
pub mod autocomplete;
pub mod config;
pub mod context;
pub mod message;
pub mod runtime;
pub mod watchers;

// Re-exports for convenience
pub use actions::{Action, ActionResult};
pub use config::{ImportantPanelUids, PanelData, SharedConfig, SidebarMode, WorkerState};
pub use context::{
    ContextElement, ContextType, ContextTypeMeta, compute_total_pages, estimate_tokens, fixed_panel_order,
    get_context_type_meta, init_context_type_registry, make_default_context_element,
};
pub use message::{Message, MessageStatus, MessageType, ToolResultRecord, ToolUseRecord, format_messages_to_chunk};
pub use runtime::{FullContentCache, InputRenderCache, MessageRenderCache, State, hash_values};

// ─── Reverie State ──────────────────────────────────────────────────────────
// Ephemeral sub-agent state — lives as Option<ReverieState> on the main State.

pub mod reverie {
    use super::message::Message;

    /// The type of reverie running.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ReverieType {
        /// Context optimizer — reshapes context for relevance and budget.
        ContextOptimizer,
    }

    /// Ephemeral state for an active reverie session.
    ///
    /// Lives as `Option<ReverieState>` on the main `State` struct.
    /// Not persisted — discarded after each run (fresh start every time).
    #[derive(Debug, Clone)]
    pub struct ReverieState {
        /// What kind of reverie this is.
        pub reverie_type: ReverieType,
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

    impl ReverieState {
        /// Create a new reverie session driven by the given agent.
        pub fn new(reverie_type: ReverieType, agent_id: String, context: Option<String>) -> Self {
            Self {
                reverie_type,
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

    impl std::fmt::Display for ReverieType {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                ReverieType::ContextOptimizer => write!(f, "Context Optimizer"),
            }
        }
    }
}
