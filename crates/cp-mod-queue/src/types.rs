use serde::{Deserialize, Serialize};

/// A single queued tool call, waiting to be flushed.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct QueuedToolCall {
    /// Index in the queue (1-based, for display and undo)
    pub index: usize,
    /// Tool name (e.g. "`Close_panel`")
    pub tool_name: String,
    /// Original `tool_use` ID from the LLM
    pub tool_use_id: String,
    /// Tool input parameters (JSON)
    pub input: serde_json::Value,
    /// Timestamp when queued (ms since epoch)
    pub queued_at: u64,
}

impl QueuedToolCall {
    /// A queued call from a tool invocation; `index` is assigned on `enqueue`,
    /// `queued_at` stamped now.
    #[must_use]
    pub fn new(tool_name: String, tool_use_id: String, input: serde_json::Value) -> Self {
        Self { index: 0, tool_name, tool_use_id, input, queued_at: cp_base::panels::now_ms() }
    }
}

/// Module state for the queue system.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct QueueState {
    /// Whether the queue is actively intercepting tool calls
    pub active: bool,
    /// Ordered list of queued tool calls
    pub queued_calls: Vec<QueuedToolCall>,
    /// Next index counter (1-based)
    pub next_index: usize,
    /// Whether the history cleanup trap is active (blocks all tools except `Close_conversation_history`)
    pub trap_active: bool,
    /// Panel IDs the trap requires to be closed (oldest → newest)
    pub trap_panel_ids: Vec<String>,
    /// The two most recent panel IDs that may optionally be kept
    pub trap_optional_ids: Vec<String>,
}

impl Default for QueueState {
    fn default() -> Self {
        Self::new()
    }
}

/// Tool name prefix for queue tools — these always bypass the queue.
pub const QUEUE_TOOL_PREFIX: &str = "Queue_";

impl QueueState {
    /// Create an empty inactive queue with index counter at 1.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            active: false,
            queued_calls: Vec::new(),
            next_index: 1,
            trap_active: false,
            trap_panel_ids: Vec::new(),
            trap_optional_ids: Vec::new(),
        }
    }

    /// Returns true if the given tool name is a Queue tool (always bypasses interception).
    #[must_use]
    pub fn is_queue_tool(name: &str) -> bool {
        name.starts_with(QUEUE_TOOL_PREFIX)
    }

    /// Get shared ref from State's `TypeMap`.
    ///
    /// Delegates to [`State::ext()`] which centralizes the panic for unregistered module state.
    #[must_use]
    pub fn get(state: &cp_base::state::runtime::State) -> &Self {
        state.ext::<Self>()
    }

    /// Get mutable ref from State's `TypeMap`.
    ///
    /// Delegates to [`State::ext_mut()`] which centralizes the panic for unregistered module state.
    pub fn get_mut(state: &mut cp_base::state::runtime::State) -> &mut Self {
        state.ext_mut::<Self>()
    }

    /// Queue a tool call. Returns the assigned index.
    pub fn enqueue(&mut self, mut call: QueuedToolCall) -> usize {
        let index = self.next_index;
        self.next_index = self.next_index.saturating_add(1);
        call.index = index;
        self.queued_calls.push(call);
        index
    }

    /// Remove a queued call by index. Returns true if found and removed.
    pub fn remove_by_index(&mut self, index: usize) -> bool {
        let before = self.queued_calls.len();
        self.queued_calls.retain(|c| c.index != index);
        self.queued_calls.len() < before
    }

    /// Drain all queued calls, returning them in order and clearing the queue.
    pub fn flush(&mut self) -> Vec<QueuedToolCall> {
        self.next_index = 1;
        std::mem::take(&mut self.queued_calls)
    }

    /// Discard all queued calls without executing.
    pub fn clear(&mut self) {
        self.queued_calls.clear();
        self.next_index = 1;
    }
}
