//! Agent-side orchestration bridge — the **entire** agent-side footprint.
//!
//! This module is **behaviorally inert when the bridge is OFF** (the default):
//! the agent reasons and acts identically whether or not this module is
//! registered.  When ON, it owns:
//!
//! - **Oplog** — append-only, `fdatasync`'d WAL for durable command effects,
//!   `rev` assignment, `seen`-marks, phase transitions, lifecycle, and cost.
//! - **Stream tee** — lock-free SPSC enqueue of each `StreamEvent`; dedicated
//!   publisher thread writes `stream.sock`.
//! - **Command intake** — bearer-auth'd, journal-then-ack, injected via the
//!   existing user-message entry (never the spine).
//! - **Heartbeat** — dedicated thread, aligned in-place write.
//!
//! The module does **not** touch `writer.rs` — tier-② persistence is unchanged.
//! See `docs/design-orchestration-backend.md` §11 / §24.

use cp_base::modules::Module;
use cp_base::panels::Panel;
use cp_base::state::context::Kind;
use cp_base::state::data::model_helpers::ModelPricing as _;
use cp_base::state::runtime::State;
use cp_base::tools::pre_flight::Verdict;
use cp_base::tools::{ToolDefinition, ToolResult, ToolUse};

use cp_wire::types::stream::Kind as StreamKind;

use crate::body::Store;
use crate::boot::Boot;
use crate::command::Intake;
use crate::tee::Tee;

/// CLI-driven bridge activation flag — set by `cpilot --bridge` before any
/// threads spawn. Complements the `CP_BRIDGE=1` env-var check so the binary
/// can activate the bridge without `unsafe` `set_var` (Rust 2024 edition).
static BRIDGE_REQUESTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

/// Request bridge activation from a CLI flag.  Call **once**, early in
/// `main()`, before module init.  Idempotent — second calls are no-ops.
pub fn request_bridge() {
    let _already_set = BRIDGE_REQUESTED.set(true);
}

/// Whether bridge mode was requested (env var *or* CLI flag).
fn bridge_active() -> bool {
    std::env::var("CP_BRIDGE").as_deref() == Ok("1") || BRIDGE_REQUESTED.get().copied().unwrap_or(false)
}

pub mod body;
pub mod boot;
pub mod command;
pub mod error;
pub mod heartbeat;
pub mod register;
pub mod tee;

/// Background bridge-boot recovery — the self-healing retry the main loop
/// invokes (throttled) when the startup boot lost the `flock` race.
///
/// A no-op unless the bridge is pending; delegates to
/// [`boot::activate::try_recover`], which owns the fail-fast, non-blocking
/// re-boot. A thin wrapper (rather than a `pub use` re-export, which the
/// project's lint policy disallows) keeps the stable `cp_mod_bridge::try_recover`
/// path for external callers.
pub fn try_recover(state: &mut State) {
    boot::activate::try_recover(state);
}

/// The change-detection memo for the context-window occupancy emit.
///
/// `(used, threshold, budget, hit, miss)` tokens. Carrying `hit`/`miss` (not
/// just `used`) means a panel flipping cache hit↔miss at an unchanged total
/// still re-emits the split (named alias to keep the field off the
/// `clippy::type_complexity` lint).
pub type ContextMemo = (u64, u64, u64, u64, u64);

/// Seed flags for observe-on-change memos.
///
/// Each flag is `false` until the corresponding chokepoint runs its first
/// pass (seed without emit), then `true` for the remainder of the session.
/// Stored as a compact bitfield to stay under the `struct_excessive_bools`
/// lint (5 independent seed flags).
#[derive(Debug, Default, Clone, Copy)]
pub struct MemoSeeds(u8);

impl MemoSeeds {
    /// Bit position: messages memo.
    const MESSAGES: u8 = 1 << 0;
    /// Bit position: statuses memo.
    const STATUSES: u8 = 1 << 1;
    /// Bit position: focus memo.
    const FOCUS: u8 = 1 << 2;
    /// Bit position: archived memo.
    const ARCHIVED: u8 = 1 << 3;
    /// Bit position: paused memo.
    const PAUSED: u8 = 1 << 4;

    /// Messages memo seeded (`emit_messages`).
    #[must_use]
    pub const fn messages(self) -> bool {
        self.0 & Self::MESSAGES != 0
    }
    /// Thread-status memo seeded (`emit_thread_status`).
    #[must_use]
    pub const fn statuses(self) -> bool {
        self.0 & Self::STATUSES != 0
    }
    /// Focus memo seeded (`emit_thread_focus`).
    #[must_use]
    pub const fn focus(self) -> bool {
        self.0 & Self::FOCUS != 0
    }
    /// Archived memo seeded (`emit_thread_archived`).
    #[must_use]
    pub const fn archived(self) -> bool {
        self.0 & Self::ARCHIVED != 0
    }
    /// Paused memo seeded (`emit_thread_paused`).
    #[must_use]
    pub const fn paused(self) -> bool {
        self.0 & Self::PAUSED != 0
    }

    /// Mark messages as seeded.
    pub const fn seed_messages(&mut self) {
        self.0 |= Self::MESSAGES;
    }
    /// Mark statuses as seeded.
    pub const fn seed_statuses(&mut self) {
        self.0 |= Self::STATUSES;
    }
    /// Mark focus as seeded.
    pub const fn seed_focus(&mut self) {
        self.0 |= Self::FOCUS;
    }
    /// Mark archived as seeded.
    pub const fn seed_archived(&mut self) {
        self.0 |= Self::ARCHIVED;
    }
    /// Mark paused as seeded.
    pub const fn seed_paused(&mut self) {
        self.0 |= Self::PAUSED;
    }
}

/// Runtime state for the bridge (stored in [`State`]'s `TypeMap`).
///
/// Not serializable — [`Boot`] holds OS resources (folder lock, stream socket,
/// oplog commit thread, heartbeat beacon) that are created fresh each session.
/// `save_module_data` / `load_module_data` return `Null`.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct BridgeState {
    /// The held boot resources, or `None` when the bridge is OFF or boot failed.
    pub boot: Option<Boot>,

    /// The stream tee publisher, or `None` when the bridge is OFF or tee
    /// setup failed. Publishes live [`StreamFrame`]s to an observing backend
    /// (design doc tier ③).
    pub tee: Option<Tee>,

    /// The command intake processor, or `None` when the bridge is OFF.
    /// Handles bearer-auth, journal-then-ack, and dedup for inbound commands.
    pub intake: Option<Intake>,

    /// Per-stream monotonic frame sequence counter for gap detection.
    pub tee_seq: u64,

    /// Last [`Phase`](cp_wire::types::Phase) emitted to the oplog, so the
    /// main-loop vitals chokepoint emits a `PhaseTransition` only on an actual
    /// transition (not every tick). `None` until the first emission.
    pub last_phase: Option<cp_wire::types::Phase>,

    /// Last cumulative spend (USD) emitted as a `CostAggregate`, so a
    /// `CostAggregate` is emitted only when the dollar total moves.
    pub last_cost_usd: f64,

    /// Last context-window occupancy `(used, threshold, budget, hit, miss)`
    /// emitted as a
    /// [`ContextUsage`](cp_wire::types::oplog::OpEntryKind::ContextUsage) delta,
    /// so the main-loop vitals chokepoint emits one only when the figure
    /// actually moves (the same observe-on-change discipline as phase/cost).
    /// `hit`/`miss` are carried in the memo too, so a panel flipping cache
    /// hit↔miss at an unchanged total still re-emits the split. `None` until
    /// the first emission.
    pub last_context: Option<ContextMemo>,

    /// Content-addressed body store for thread-message bodies (I13). `None`
    /// when the bridge is OFF or the store could not be opened. The main-loop
    /// message chokepoint writes each new message's body here (inline-small /
    /// spill-large, durable) **before** referencing it from a `MessageCreated`
    /// oplog delta.
    pub store: Option<Store>,

    /// Per-thread count of messages already emitted as `MessageCreated`
    /// deltas, keyed by thread id. The message chokepoint diffs the live
    /// thread message vectors against this memo each tick and emits only the
    /// newly-appended messages (the same observe-on-change discipline as the
    /// phase/cost vitals).
    pub thread_msg_counts: std::collections::HashMap<String, usize>,

    /// Last per-thread turn-status emitted as a `ThreadStatusChanged` delta,
    /// keyed by thread id. The status chokepoint diffs each thread's live
    /// status against this memo every tick and emits only on an actual flip
    /// (the same observe-on-change discipline as the message and vitals
    /// chokepoints), so a `MyTurn`↔`TheirTurn` transition from *any* source —
    /// a web `SendMessage`, the agent's `Send` tool, a TUI reply, the agent
    /// finishing a turn — reaches the backend view (and the web roster) in
    /// milliseconds, not on the next disk flush.
    pub thread_statuses: std::collections::HashMap<String, cp_wire::types::ThreadTurn>,

    /// Last focused-thread id emitted as a `ThreadFocusChanged` delta. The
    /// focus chokepoint diffs the live `FocusState.focused_thread_id` against
    /// this memo every tick and emits only on an actual change (the same
    /// observe-on-change discipline as the status/message/vitals chokepoints),
    /// so the agent focusing a thread — from *any* source (an idle `MY_TURN`
    /// auto-`Read`, a manual `Read`, focus release on archive/`Send`) — reaches
    /// the backend view (and the web UI's focused-thread highlight) in
    /// milliseconds, not on the next debounced disk flush + poll.
    pub last_focus: Option<String>,

    /// Per-thread archived flag as last emitted/seeded, keyed by thread id.
    /// The archived chokepoint diffs each thread's live `archived` against this
    /// memo every tick and emits `ThreadArchived`/`ThreadRestored` only on an
    /// actual change — the same observe-on-change discipline as the status and
    /// message chokepoints.
    pub thread_archived_memo: std::collections::HashMap<String, bool>,

    /// Per-thread paused flag as last emitted/seeded, keyed by thread id.
    /// The paused chokepoint diffs each thread's live `paused` against this
    /// memo every tick and emits `ThreadPaused`/`ThreadResumed` only on an
    /// actual change — mirrors the archived chokepoint (T371).
    pub thread_paused_memo: std::collections::HashMap<String, bool>,

    /// Flags tracking which observe-on-change memos have been seeded from the
    /// oplog roster on the first tick after boot. See [`MemoSeeds`].
    pub seeded: MemoSeeds,

    /// The bridge is **pending recovery**: `CP_BRIDGE=1` was set but
    /// [`Boot::start`] failed (most commonly an `AlreadyRunning` `flock` race
    /// against a still-dying predecessor on a fast relaunch). Rather than give
    /// up for the whole session — leaving the agent silently unreachable to web
    /// sends — the main loop periodically retries boot via
    /// [`try_recover`](crate::try_recover) until it succeeds. `false` once the
    /// bridge is live (or when the bridge is OFF entirely).
    pub pending: bool,

    /// The model name to advertise when a pending boot finally succeeds,
    /// captured at the failed startup attempt (the model is fixed for the
    /// session). Only meaningful while [`pending`](Self::pending) is `true`.
    pub pending_model: String,
}

/// Agent-side orchestration bridge module.
///
/// Inert when `CP_BRIDGE` is unset or not `"1"`.  When active, [`init_state`]
/// acquires the folder lock, opens the oplog, binds the stream socket, starts
/// the heartbeat beacon, and writes a registry record so the backend discovers
/// this agent.
///
/// [`init_state`]: Module::init_state
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct BridgeModule;

impl Default for BridgeModule {
    fn default() -> Self {
        Self::new()
    }
}

impl BridgeModule {
    /// Construct the module marker (funnels cross-crate construction of this
    /// `non_exhaustive` unit struct through an associated fn).
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Module for BridgeModule {
    fn id(&self) -> &'static str {
        "bridge"
    }

    fn name(&self) -> &'static str {
        "Bridge"
    }

    fn description(&self) -> &'static str {
        "Agent-side orchestration bridge (oplog, stream tee, command intake)"
    }

    fn dependencies(&self) -> &[&'static str] {
        &[]
    }

    fn is_core(&self) -> bool {
        false
    }

    fn is_global(&self) -> bool {
        true
    }

    fn init_state(&self, state: &mut State) {
        if !bridge_active() {
            state.set_ext(BridgeState::default());
            return;
        }

        let folder = match std::env::current_dir() {
            Ok(f) => f,
            Err(e) => {
                log::error!("bridge: cannot determine working directory: {e}");
                state.set_ext(BridgeState::default());
                return;
            }
        };

        let model = state.current_model();

        match Boot::start(&folder, &model) {
            Ok(boot) => boot::activate::activate(boot, state),
            Err(e) => {
                // Boot failed — most often an `AlreadyRunning` `flock` race on a
                // fast relaunch (the dying predecessor still holds the lock).
                // Instead of running inert for the whole session (silent web
                // 502s until a manual relaunch), enter a PENDING state: the main
                // loop re-attempts boot every couple of seconds via
                // [`try_recover`], so the bridge self-heals the moment the lock
                // frees. Loud `error!` (best-effort — the file logger may filter
                // a non-`cp_base` target; the recovery loop is the real safety
                // net, not the log).
                log::error!("bridge: boot failed ({e:?}); entering recovery — will retry until the lock frees");
                state.set_ext(BridgeState { pending: true, pending_model: model, ..Default::default() });
            }
        }
    }

    fn reset_state(&self, _state: &mut State) {}

    fn save_module_data(&self, _state: &State) -> serde_json::Value {
        serde_json::Value::Null
    }

    fn load_module_data(&self, _data: &serde_json::Value, _state: &mut State) {}

    fn save_worker_data(&self, _state: &State) -> serde_json::Value {
        serde_json::Value::Null
    }

    fn load_worker_data(&self, _data: &serde_json::Value, _state: &mut State) {}

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        // No tools exposed to the LLM — the bridge is infrastructure,
        // not an interactive surface.
        Vec::new()
    }

    fn execute_tool(&self, _tool: &ToolUse, _state: &mut State) -> Option<ToolResult> {
        None
    }

    fn pre_flight(&self, _tool: &ToolUse, _state: &State) -> Option<Verdict> {
        None
    }

    fn create_panel(&self, _context_type: &Kind) -> Option<Box<dyn Panel>> {
        None
    }

    fn fixed_panel_types(&self) -> Vec<Kind> {
        vec![]
    }

    fn dynamic_panel_types(&self) -> Vec<Kind> {
        vec![]
    }

    fn fixed_panel_defaults(&self) -> Vec<(Kind, &'static str, bool)> {
        vec![]
    }

    fn context_type_metadata(&self) -> Vec<cp_base::state::context::TypeMeta> {
        vec![]
    }

    fn tool_visualizers(&self) -> Vec<(&'static str, cp_base::modules::ToolVisualizer)> {
        vec![]
    }

    fn context_display_name(&self, _context_type: &str) -> Option<&'static str> {
        None
    }

    fn context_detail(&self, _ctx: &cp_base::state::context::Entry) -> Option<String> {
        None
    }

    fn overview_context_section(&self, _state: &State) -> Option<String> {
        None
    }

    fn overview_render_sections(&self, _state: &State) -> Vec<(u8, Vec<cp_render::Block>)> {
        vec![]
    }

    fn on_close_context(
        &self,
        _ctx: &cp_base::state::context::Entry,
        _state: &mut State,
    ) -> Option<Result<String, String>> {
        None
    }

    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![]
    }

    fn on_user_message(&self, _state: &mut State) {}

    fn on_stream_stop(&self, state: &mut State) {
        boot::activate::publish_frame(state, StreamKind::PhaseHint { phase: cp_wire::types::Phase::Idle });
    }

    fn on_stream_chunk(&self, text: &str, state: &mut State) {
        boot::activate::publish_frame(state, StreamKind::Token { text: text.to_owned() });
    }

    fn on_tool_progress(&self, _tool_name: &str, _input_so_far: &str, _state: &mut State) {}

    fn on_tool_complete(&self, _tool_name: &str, _state: &mut State) {}

    fn watch_paths(&self, _state: &State) -> Vec<cp_base::panels::WatchSpec> {
        vec![]
    }

    fn should_invalidate_on_fs_change(
        &self,
        _ctx: &cp_base::state::context::Entry,
        _changed_path: &str,
        _is_dir_event: bool,
    ) -> bool {
        false
    }

    fn watcher_immediate_refresh(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_module_identity() {
        let m = BridgeModule;
        assert_eq!(m.id(), "bridge");
        assert_eq!(m.name(), "Bridge");
        assert!(!m.is_core());
        assert!(m.is_global());
    }

    #[test]
    fn bridge_module_no_tools() {
        let m = BridgeModule;
        assert!(m.tool_definitions().is_empty());
    }

    #[test]
    fn bridge_state_default_is_none() {
        let bs = BridgeState::default();
        assert!(bs.boot.is_none());
        assert!(bs.tee.is_none());
        assert!(bs.intake.is_none());
        assert_eq!(bs.tee_seq, 0);
        assert_eq!(bs.last_phase, None);
        assert!((bs.last_cost_usd - 0.0).abs() < f64::EPSILON);
        assert!(bs.last_context.is_none());
        assert!(bs.store.is_none());
        assert!(bs.thread_msg_counts.is_empty());
        assert!(bs.thread_statuses.is_empty());
        assert!(bs.last_focus.is_none());
        assert!(bs.thread_archived_memo.is_empty());
        assert!(bs.thread_paused_memo.is_empty());
        assert!(!bs.seeded.messages());
        assert!(!bs.seeded.statuses());
        assert!(!bs.seeded.focus());
        assert!(!bs.seeded.archived());
        assert!(!bs.seeded.paused());
        assert!(!bs.pending);
        assert!(bs.pending_model.is_empty());
    }
}
