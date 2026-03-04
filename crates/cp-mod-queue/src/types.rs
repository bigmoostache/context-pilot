use serde::{Deserialize, Serialize};

/// A single queued tool call, waiting to be flushed.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Module state for the queue system.
#[derive(Debug, Clone)]
pub struct QueueState {
    /// Whether the queue is actively intercepting tool calls
    pub active: bool,
    /// Ordered list of queued tool calls
    pub queued_calls: Vec<QueuedToolCall>,
    /// Next index counter (1-based)
    pub next_index: usize,
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
    pub fn new() -> Self {
        Self { active: false, queued_calls: Vec::new(), next_index: 1 }
    }

    /// Returns true if the given tool name is a Queue tool (always bypasses interception).
    pub fn is_queue_tool(name: &str) -> bool {
        name.starts_with(QUEUE_TOOL_PREFIX)
    }

    /// Get shared ref from State's `TypeMap`
    pub fn get(state: &cp_base::state::State) -> &Self {
        state.get_ext::<Self>().expect("QueueState not initialized")
    }

    /// Get mutable ref from State's `TypeMap`
    pub fn get_mut(state: &mut cp_base::state::State) -> &mut Self {
        state.get_ext_mut::<Self>().expect("QueueState not initialized")
    }

    /// Queue a tool call. Returns the assigned index.
    pub fn enqueue(&mut self, tool_name: String, tool_use_id: String, input: serde_json::Value, now_ms: u64) -> usize {
        let index = self.next_index;
        self.next_index += 1;
        self.queued_calls.push(QueuedToolCall { index, tool_name, tool_use_id, input, queued_at: now_ms });
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
