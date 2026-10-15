use serde::{Deserialize, Serialize};

/// A single queued tool call, waiting to be flushed.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
    /// Who queued it: `None` = main agent, `Some(system_id)` = that reverie.
    /// Scopes flush/clear so a reverie never drops or runs the main agent's calls.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
}

impl QueuedToolCall {
    /// A queued call from a tool invocation; `index` is assigned on `enqueue`,
    /// `queued_at` stamped now, owned by the main agent.
    #[must_use]
    pub fn new(tool_name: String, tool_use_id: String, input: serde_json::Value) -> Self {
        Self { index: 0, tool_name, tool_use_id, input, queued_at: cp_base::panels::now_ms(), owner: None }
    }

    /// Re-own the call (reverie enqueue sites pass `Some(system_id)`).
    #[must_use]
    pub fn owned_by(mut self, owner: Option<&str>) -> Self {
        self.owner = owner.map(str::to_owned);
        self
    }
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
        let removed = self.queued_calls.len() < before;
        self.reset_index_if_empty();
        removed
    }

    /// Drain `owner`'s queued calls in order; other owners' calls stay queued.
    pub fn flush(&mut self, owner: Option<&str>) -> Vec<QueuedToolCall> {
        let (mine, rest) =
            std::mem::take(&mut self.queued_calls).into_iter().partition(|c| c.owner.as_deref() == owner);
        self.queued_calls = rest;
        self.reset_index_if_empty();
        mine
    }

    /// Discard `owner`'s queued calls without executing; other owners' calls stay.
    pub fn clear(&mut self, owner: Option<&str>) {
        self.queued_calls.retain(|c| c.owner.as_deref() != owner);
        self.reset_index_if_empty();
    }

    /// Number of calls queued by `owner`.
    #[must_use]
    pub fn count_owned(&self, owner: Option<&str>) -> usize {
        self.queued_calls.iter().filter(|c| c.owner.as_deref() == owner).count()
    }

    /// Indices stay monotonic while any call is queued (undo targets them),
    /// and restart at 1 once the queue is fully drained.
    const fn reset_index_if_empty(&mut self) {
        if self.queued_calls.is_empty() {
            self.next_index = 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{QueueState, QueuedToolCall};

    fn call(owner: Option<&str>) -> QueuedToolCall {
        QueuedToolCall::new("Edit".to_owned(), "id".to_owned(), serde_json::Value::Null).owned_by(owner)
    }

    #[test]
    fn reverie_clear_keeps_main_calls() {
        let mut qs = QueueState::new();
        let _a = qs.enqueue(call(None));
        let _b = qs.enqueue(call(Some("cleaner")));
        qs.clear(Some("cleaner"));
        assert_eq!(qs.count_owned(None), 1);
        assert_eq!(qs.queued_calls.len(), 1);
        assert_eq!(qs.next_index, 3);
    }

    #[test]
    fn reverie_flush_takes_only_its_calls() {
        let mut qs = QueueState::new();
        let _a = qs.enqueue(call(None));
        let _b = qs.enqueue(call(Some("cleaner")));
        let taken = qs.flush(Some("cleaner"));
        assert_eq!(taken.len(), 1);
        assert_eq!(taken.first().map(|c| c.index), Some(2));
        assert_eq!(qs.count_owned(None), 1);
    }

    #[test]
    fn main_flush_resets_index_when_drained() {
        let mut qs = QueueState::new();
        let _a = qs.enqueue(call(None));
        assert_eq!(qs.flush(None).len(), 1);
        assert!(qs.queued_calls.is_empty());
        assert_eq!(qs.next_index, 1);
    }

    #[test]
    fn legacy_call_without_owner_deserializes_as_main() {
        let json = r#"{"index":1,"tool_name":"Edit","tool_use_id":"x","input":null,"queued_at":0}"#;
        let parsed: QueuedToolCall = serde_json::from_str(json).expect("legacy shape parses");
        assert_eq!(parsed.owner, None);
    }
}
