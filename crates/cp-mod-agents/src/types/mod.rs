use serde::{Deserialize, Serialize};

use cp_base::state::runtime::State;

/// Per-thread message files (`agents/<id>.json`), written only when dirty.
pub mod persist;

/// Per-agent tool-call traces, interleaved with the human group for display.
pub mod activity;
/// Agent messages = its human group's messages.
pub mod msgs;
pub mod roster;
pub mod tree;
/// Turn derived from group social state (no stored flag).
pub mod turn;

// =============================================================================
// Enums
// =============================================================================

/// Agent turn status — who needs to act next. Derived, never stored: see [`turn`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentStatus {
    /// The AI's turn — agent has user input awaiting response.
    MyTurn,
    /// The user's turn — AI has responded, waiting for user.
    #[default]
    TheirTurn,
}

impl std::fmt::Display for AgentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::MyTurn => write!(f, "MY_TURN"),
            Self::TheirTurn => write!(f, "THEIR_TURN"),
        }
    }
}

// Messages now belong to groups (T860 phase C); the old names stay as aliases.
/// Alias of [`cp_groups::chat::GroupAuthor`].
pub type AgentAuthor = cp_groups::chat::GroupAuthor;
/// Alias of [`cp_groups::chat::GroupMessage`].
pub type AgentMessage = cp_groups::chat::GroupMessage;

// =============================================================================
// Structs
// =============================================================================

/// Where a branched agent came from: the parent agent and the message it was
/// cut at (inclusive). Set only on agents created by [`AgentsState::branch`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentOrigin {
    /// Id of the parent agent at branch time (may since have been deleted).
    #[serde(alias = "thread_id")]
    pub agent_id: String,
    /// Epoch-ms timestamp of the last parent message copied into the branch.
    pub message_ts: u64,
}

/// A parallel discussion/work topic thread.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Agent {
    /// Short unique identifier (e.g. "T1", "T2").
    pub id: String,
    /// Free-text label chosen by the user.
    pub name: String,
    /// Group holding this agent's conversation with the human (its messages,
    /// see [`msgs`]). Empty only for pre-groups data, until the boot migration.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub human_group_id: String,
    /// Creation timestamp (epoch ms).
    pub created_at: u64,
    /// Soft-delete flag — archived agents are hidden from the active list
    /// but retained in state so the web frontend can display and restore them.
    #[serde(default)]
    pub archived: bool,
    /// Pause flag — paused agents suppress `MY_TURN` idle notifications so
    /// the AI does not nag about them, but remain visible and fully functional.
    /// The user sets this when they are still composing input and do not want
    /// the agent to act yet.
    #[serde(default)]
    pub paused: bool,
    /// Parent agent + branch point when this agent was branched out of
    /// another one (`None` for an agent created from scratch). Defaults to
    /// `None` (back-compat with pre-feature data).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<AgentOrigin>,
    /// Managing agent (`None` = root agent, created by the user). The parent
    /// links form a forest: see [`tree`] for the cycle and depth checks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_agent_id: Option<String>,
    /// One-line purpose shown next to the name in the agents tree.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Tool-call traces of this agent (`auto` messages). Persisted in
    /// `activity/<id>.json`, never in `config.json` (see [`activity`]).
    #[serde(skip)]
    pub activity: Vec<AgentMessage>,
    /// Bumped on every [`Self::activity`] change (only [`activity`]'s fns
    /// mutate it), so the saver spots dirty files without hashing traces.
    #[serde(skip)]
    pub activity_rev: u64,
}

impl Agent {
    /// A fresh empty agent: not archived, not paused, stamped now; it
    /// holds no turn until one is given (the user types first).
    #[must_use]
    pub fn new(id: String, name: String) -> Self {
        Self {
            id,
            name,
            human_group_id: String::new(),
            created_at: cp_base::panels::now_ms(),
            archived: false,
            paused: false,
            origin: None,
            parent_agent_id: None,
            description: String::new(),
            activity: Vec::new(),
            activity_rev: 0,
        }
    }
}

// =============================================================================
// Module State — shared (is_global=true)
// =============================================================================

/// Shared agent state, persisted via `save_module_data`.
#[derive(Debug)]
pub struct AgentsState {
    /// All active agents.
    pub agents: Vec<Agent>,
    /// Counter for generating unique agent IDs (T1, T2, ...).
    pub next_id: u32,
    /// Pre-rendered panel content for the LLM. Only updated by `Read`.
    /// Contains the agent list summary + focused agent's full conversation.
    pub panel_content: String,
}

impl Default for AgentsState {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentsState {
    /// Create an empty agents state with ID counter at 1.
    #[must_use]
    pub const fn new() -> Self {
        Self { agents: vec![], next_id: 1, panel_content: String::new() }
    }

    /// Get shared ref from State's `TypeMap`.
    ///
    /// # Panics
    ///
    /// Panics if `AgentsState` was never inserted into state.
    #[must_use]
    pub fn get(state: &State) -> &Self {
        state.ext::<Self>()
    }

    /// Get mutable ref from State's `TypeMap`.
    ///
    /// # Panics
    ///
    /// Panics if `AgentsState` was never inserted into state.
    pub fn get_mut(state: &mut State) -> &mut Self {
        state.ext_mut::<Self>()
    }

    /// Returns true if any *non-archived*, non-paused agent is runnable in `groups`.
    ///
    /// Archived agents are invisible to the LLM (T9): they never trigger
    /// `MY_TURN` idle nudges, never appear in context. Restoring an agent
    /// makes it count again.
    #[must_use]
    pub fn has_my_turn_agents(&self, groups: &[cp_groups::types::Group]) -> bool {
        self.agents.iter().any(|t| !t.archived && !t.paused && turn::status_of(groups, &t.id) == AgentStatus::MyTurn)
    }

    /// Indices into [`Self::agents`] of the agents whose `archived` flag
    /// matches `archived`, in tree order (sub-agents right
    /// under their parent, see [`tree::preorder`]; storage order among siblings).
    ///
    /// The TUI thread-centered view shows one subset at a time — the active
    /// list (`archived = false`) or the archived list (`archived = true`,
    /// toggled by Ctrl+U). Selection (`selected_agent_idx`) is a position
    /// **into this filtered slice**, resolved back to a real index here so a
    /// soft-deleted agent keeps its place in storage without polluting the
    /// visible list.
    #[must_use]
    pub fn visible_indices(&self, archived: bool) -> Vec<usize> {
        let matching: Vec<usize> =
            self.agents.iter().enumerate().filter(|entry| entry.1.archived == archived).map(|entry| entry.0).collect();
        tree::preorder(&self.agents, &matching).into_iter().map(|(i, _)| i).collect()
    }

    /// Branch a new agent out of `source_id`: a fresh agent named `name`
    /// whose history is a copy of the parent's messages up to and including the
    /// one stamped `at_ts`. Returns the new agent's id.
    ///
    /// Copied messages keep their original timestamps (still unique within the
    /// branch, so `DeleteMessage` works on it) and are all marked acknowledged —
    /// the branch starts from context the agent has already seen. The branch
    /// holds no turn (never archived/paused, even if the parent
    /// is) recording its parent in [`Agent::origin`]. Nothing else the parent
    /// owns (todos, scratchpad) is copied: those only exist as a *current*
    /// snapshot, which would leak whatever happened after the branch point.
    ///
    /// # Errors
    ///
    /// Returns a human-readable message when the source agent does not exist
    /// or holds no message stamped `at_ts`; the state is left untouched.
    pub fn branch(
        &mut self,
        groups: &mut cp_groups::types::GroupsState,
        (source_id, at_ts): (&str, u64),
        name: &str,
    ) -> Result<String, String> {
        if !self.agents.iter().any(|t| t.id == source_id) {
            return Err(format!("agent {source_id} not found"));
        }
        let history = msgs::of(self, groups, source_id);
        let cut = history
            .iter()
            .position(|m| m.timestamp == at_ts)
            .ok_or_else(|| format!("no message with ts={at_ts} in agent {source_id}"))?;
        let messages: Vec<AgentMessage> = history
            .iter()
            .take(cut.saturating_add(1))
            .map(|m| AgentMessage { acknowledged: true, ..m.clone() })
            .collect();

        let id = self.create(groups, name);
        let gid = msgs::human_group_id(self, &id).unwrap_or_default().to_owned();
        if let Some(g) = groups.find_mut(&gid) {
            g.messages = messages;
        }
        if let Some(agent) = self.agents.iter_mut().find(|a| a.id == id) {
            agent.origin = Some(AgentOrigin { agent_id: source_id.to_owned(), message_ts: at_ts });
        }
        Ok(id)
    }

    /// Create a fresh agent named `name` with its human group; returns its id.
    pub fn create(&mut self, groups: &mut cp_groups::types::GroupsState, name: &str) -> String {
        let id = format!("A{}", self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        let mut agent = Agent::new(id.clone(), name.to_owned());
        agent.human_group_id = groups.create_human_group(&id, name);
        self.agents.push(agent);
        id
    }
}

// =============================================================================
// Focus State — per-worker (save_worker_data / load_worker_data)
// =============================================================================

/// Per-worker focus tracking for agent enforcement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FocusState {
    /// Which agent the AI is currently focused on (None = unfocused).
    #[serde(alias = "focused_thread_id")]
    pub focused_agent_id: Option<String>,
    /// Which agent the human has *drilled into* in the TUI (`None` = showing the
    /// agent list). Pure view state (G3): it selects whose full panel view is
    /// painted, and never moves execution — the renderer makes the drilled
    /// agent executing only for the duration of one paint, then
    /// restores the previous one (Model 2: a human glance must not disturb the
    /// agent's work). Defaults to `None` (back-compat; byte-identical until set).
    #[serde(default)]
    #[serde(alias = "drilled_thread_id")]
    pub drilled_agent_id: Option<String>,
    /// Index of the currently selected agent in the TUI agents view.
    /// Used for navigation (Tab/Shift+Tab) and message area display.
    #[serde(default)]
    #[serde(alias = "selected_thread_idx")]
    pub selected_agent_idx: usize,
    /// When true, the input field is being used to name a new agent.
    /// Set by pressing 'n' in Agents view, cleared on Enter or Esc.
    #[serde(default)]
    #[serde(alias = "creating_thread")]
    pub creating_agent: bool,
    /// When true, an archive/restore is *armed*: a first Ctrl+X was pressed and
    /// the TUI is waiting for a confirming second Ctrl+X. Cleared on confirm, on
    /// any other (non-Ctrl) key, or implicitly once the 2-second window lapses.
    #[serde(default)]
    pub confirming_archive: bool,
    /// Wall-clock ms ([`now_ms`](cp_base::panels::now_ms)) of the first Ctrl+X
    /// that armed the pending archive. The confirming second Ctrl+X only counts
    /// while `now - archive_armed_at_ms <= 2000`; a later press re-arms instead.
    #[serde(default)]
    pub archive_armed_at_ms: u64,
    /// When true, the TUI thread-centered view shows the *archived* agents
    /// instead of the active ones (toggled by Ctrl+U). Selection indexes into
    /// the matching filtered slice ([`AgentsState::visible_indices`]); the
    /// virtual "+ New Agent" entry only appears in the active (non-archived)
    /// view.
    #[serde(default)]
    pub viewing_archived: bool,
    /// Per-thread last-read message count, keyed by agent ID.
    /// Used for unread indicators — an agent is "unread" when
    /// `messages.len() > last_read_count[agent_id]`.
    #[serde(default)]
    pub last_read_count: std::collections::BTreeMap<String, usize>,
    /// Agent currently under the list cursor and the ms it got there. An agent
    /// is only marked read after [`READ_DWELL_MS`] of continuous selection, so
    /// arrowing past rows leaves their unread marker intact. Transient.
    #[serde(skip)]
    pub read_dwell: Option<(String, u64)>,
    /// Draft name typed on the virtual "+ New Agent" row. Its own textarea
    /// (not an agent's composer) so it survives navigation and reloads.
    #[serde(default)]
    #[serde(alias = "new_thread_title")]
    pub new_agent_title: cp_base::state::runtime::textarea::TextArea,
}

impl Default for FocusState {
    fn default() -> Self {
        Self::new()
    }
}

impl FocusState {
    /// Initial focus state: unfocused, no escalation.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            focused_agent_id: None,
            drilled_agent_id: None,
            selected_agent_idx: 0,
            creating_agent: false,
            confirming_archive: false,
            archive_armed_at_ms: 0,
            viewing_archived: false,
            last_read_count: std::collections::BTreeMap::new(),
            read_dwell: None,
            new_agent_title: cp_base::state::runtime::textarea::TextArea::new(),
        }
    }

    /// Get shared ref from State's `TypeMap`.
    ///
    /// # Panics
    ///
    /// Panics if `FocusState` was never inserted into state.
    #[must_use]
    pub fn get(state: &State) -> &Self {
        state.ext::<Self>()
    }

    /// Get mutable ref from State's `TypeMap`.
    ///
    /// # Panics
    ///
    /// Panics if `FocusState` was never inserted into state.
    pub fn get_mut(state: &mut State) -> &mut Self {
        state.ext_mut::<Self>()
    }

    /// Mark the currently selected agent as fully read.
    /// Updates `last_read_count` to the agent's current message count.
    ///
    /// `selected_agent_idx` is a position into the visible slice for the
    /// current view, so it is resolved through [`AgentsState::visible_indices`]
    /// to the real storage index before marking.
    pub fn mark_selected_read(state: &mut State) {
        let agents = AgentsState::get(state);
        let focus = Self::get(state);
        let visible = agents.visible_indices(focus.viewing_archived);
        let Some(&real_idx) = visible.get(focus.selected_agent_idx) else {
            return;
        };
        if let Some(agent) = agents.agents.get(real_idx) {
            let tid = agent.id.clone();
            let count = msgs::messages(state, &tid).len();
            let prev = Self::get_mut(state).last_read_count.insert(tid, count);
            // Surgical repaint: the unread marker may have just cleared with no
            // input event to trigger a draw. Only on an actual change, so the
            // per-tick dwell re-mark never forces a redraw on its own.
            if prev != Some(count) {
                state.flags.ui.dirty = true;
            }
        }
    }

    /// Per-tick read tracking for the Agents view: restart the dwell clock
    /// when the selected agent changes; once it has stayed selected for
    /// [`READ_DWELL_MS`], mark it read (and keep doing so while it stays, so
    /// replies arriving under the cursor count as seen).
    pub fn tick_read_dwell(state: &mut State, now_ms: u64) {
        let selected = (state.view_mode == cp_base::state::data::config::ViewMode::Agents)
            .then(|| {
                let focus = Self::get(state);
                let agents = AgentsState::get(state);
                let visible = agents.visible_indices(focus.viewing_archived);
                let real_idx = *visible.get(focus.selected_agent_idx)?;
                agents.agents.get(real_idx).map(|t| t.id.clone())
            })
            .flatten();
        let dwell = &mut Self::get_mut(state).read_dwell;
        let since = dwell.as_ref().filter(|d| selected.as_deref() == Some(d.0.as_str())).map(|d| d.1);
        let dwelled = since.map_or_else(
            || {
                *dwell = selected.map(|id| (id, now_ms));
                false
            },
            |start| now_ms.saturating_sub(start) >= READ_DWELL_MS,
        );
        if dwelled {
            Self::mark_selected_read(state);
        }
    }
}

/// Continuous selection time before an agent's messages count as user-read.
pub const READ_DWELL_MS: u64 = 2_000;

#[cfg(test)]
mod tests;
