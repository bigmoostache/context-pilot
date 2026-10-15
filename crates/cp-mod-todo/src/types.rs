use cp_base::config::accessors::icons;
use cp_base::state::runtime::State;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Todo item status.
///
/// Four states (thread-owned tasks rework): `Planned` (replaces the old
/// `Pending`), `InProgress`, `Done`, and `Cancelled` — the soft-delete. A
/// cancelled item is hidden from the panel and excluded from every count and
/// from `check_done_allowed`; it is never hard-removed except when its owning
/// agent is deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    #[default]
    /// Not started. Accepts the legacy `pending` wire name so old saves load.
    #[serde(alias = "pending")]
    Planned, // ' '
    /// Work in progress.
    InProgress, // '~'
    /// Completed.
    Done, // 'x'
    /// Soft-deleted — hidden from the panel, excluded from all counts.
    Cancelled, // '/'
}

impl TodoStatus {
    /// Theme icon for this status (e.g., "○ ", "◐ ", "● ").
    ///
    /// `Cancelled` uses a literal `✕` — there is no theme glyph for it (it is
    /// never rendered in the panel, so a config-schema entry would be dead).
    #[must_use]
    pub fn icon(self) -> String {
        match self {
            Self::Planned => icons::todo_pending(),
            Self::InProgress => icons::todo_in_progress(),
            Self::Done => icons::todo_done(),
            Self::Cancelled => "\u{2715}".to_owned(),
        }
    }
}

impl FromStr for TodoStatus {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            " " | "planned" | "pending" => Ok(Self::Planned),
            "~" | "in_progress" => Ok(Self::InProgress),
            "x" | "X" | "done" => Ok(Self::Done),
            "/" | "cancelled" => Ok(Self::Cancelled),
            _ => Err(()),
        }
    }
}

/// A todo item
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    /// Todo ID (X1, X2, ...)
    pub id: String,
    /// Owning agent id (compulsory, thread-owned tasks rework). An item with
    /// no owning agent cannot exist; legacy items lacking one are purged on
    /// load. Serialized unconditionally (no skip) — it is a foreign key.
    #[serde(default)]
    #[serde(alias = "thread_id")]
    pub agent_id: String,
    /// Parent todo ID (for nesting). Its parent MUST share the same `agent_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    /// Todo name/title
    pub name: String,
    /// Detailed description
    #[serde(default)]
    pub description: String,
    /// Lifecycle status (planned / in progress / done / cancelled).
    #[serde(default)]
    pub status: TodoStatus,
    /// Sibling sort key within the same parent group.
    ///
    /// **Pure backend** — never exposed in the `Todo` tool payload. New tasks
    /// append at the end of their group; [`crate::upsert::renumber_orders`]
    /// re-packs every group to `0..n` with cancelled (soft-deleted) items
    /// last. Defaults to `0` so pre-rework persisted items load.
    #[serde(default)]
    pub order: i32,
}

/// Module-owned state for the Todo module
#[derive(Debug)]
pub struct TodoState {
    /// All todo items (top-level + nested children).
    pub todos: Vec<TodoItem>,
    /// Counter for generating unique IDs (X1, X2, ...).
    pub next_todo_id: usize,
    /// Injected focused-thread id for panel scoping (thread-owned tasks
    /// rework). The main crate stamps the current focused agent id here on
    /// focus change; the panel renders only items whose `agent_id` matches.
    /// **Transient** — never serialized (see `save_module_data`).
    pub focus_filter: Option<String>,
    /// The agent most recently work-hygiene-nudged (FR11 fire-once tracking).
    /// The main crate sets this after emitting a nudge so it does not re-fire on
    /// every tool call; it clears when the condition clears or focus moves.
    /// **Transient** — never serialized.
    pub nudged_agent: Option<String>,
    /// Change stamp, refreshed on every [`TodoState::get_mut`]. Lets per-tick
    /// observers skip their diff while it is unchanged. Read-only by
    /// convention — only `get_mut` writes it. **Transient**.
    pub revision: u64,
}

/// Process-wide stamp source: unique across `set_ext` replacements, so a
/// fresh state never reuses a stamp an observer already cached.
static REVISION_SOURCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

impl Default for TodoState {
    fn default() -> Self {
        Self::new()
    }
}

impl TodoState {
    /// Create an empty todo state with ID counter at 1.
    #[must_use]
    pub const fn new() -> Self {
        Self { todos: vec![], next_todo_id: 1, focus_filter: None, nudged_agent: None, revision: 0 }
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

    /// Get mutable ref from State's `TypeMap`, refreshing the change stamp.
    ///
    /// # Panics
    ///
    /// Panics if an internal invariant is violated.
    pub fn get_mut(state: &mut State) -> &mut Self {
        let ts = state.ext_mut::<Self>();
        ts.revision = REVISION_SOURCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        ts
    }

    /// Open tasks (planned or in progress) owned by `agent_id`. Done and
    /// cancelled items never count. `None` agent = no focus = zero.
    #[must_use]
    pub fn open_count(&self, agent_id: Option<&str>) -> usize {
        let Some(agent) = agent_id else {
            return 0;
        };
        self.todos
            .iter()
            .filter(|t| t.agent_id == agent && matches!(t.status, TodoStatus::Planned | TodoStatus::InProgress))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::{TodoItem, TodoState, TodoStatus};

    #[test]
    fn open_count_scopes_to_agent_and_skips_done_and_cancelled() {
        let mk = |id: &str, agent: &str, status: TodoStatus| TodoItem {
            id: id.to_owned(),
            agent_id: agent.to_owned(),
            parent_id: None,
            name: id.to_owned(),
            description: String::new(),
            status,
            order: 0,
        };
        let mut ts = TodoState::new();
        ts.todos = vec![
            mk("X1", "A1", TodoStatus::Planned),
            mk("X2", "A1", TodoStatus::InProgress),
            mk("X3", "A1", TodoStatus::Done),
            mk("X4", "A1", TodoStatus::Cancelled),
            mk("X5", "A2", TodoStatus::Planned),
        ];
        assert_eq!(ts.open_count(Some("A1")), 2);
        assert_eq!(ts.open_count(Some("A2")), 1);
        assert_eq!(ts.open_count(None), 0);
    }

    #[test]
    fn legacy_items_still_load() {
        let item: TodoItem =
            serde_json::from_str(r#"{"id":"X1","thread_id":"T1","name":"n","status":"pending"}"#).unwrap();
        assert_eq!((item.agent_id.as_str(), item.status, item.order), ("T1", TodoStatus::Planned, 0i32));
    }
}
