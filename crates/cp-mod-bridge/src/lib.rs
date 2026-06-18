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

use cp_wire::types::stream::{Frame as StreamFrame, Kind as StreamKind};

use crate::body::Store;
use crate::boot::Boot;
use crate::command::Intake;
use crate::tee::Tee;

pub mod body;
pub mod boot;
pub mod command;
pub mod error;
pub mod heartbeat;
pub mod register;
pub mod tee;

/// Runtime state for the bridge (stored in [`State`]'s `TypeMap`).
///
/// Not serializable — [`Boot`] holds OS resources (folder lock, stream socket,
/// oplog commit thread, heartbeat beacon) that are created fresh each session.
/// `save_module_data` / `load_module_data` return `Null`.
#[derive(Debug, Default)]
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

    /// Whether [`thread_msg_counts`](Self::thread_msg_counts) has been seeded
    /// from the threads already present at boot. Until it is, the first
    /// chokepoint pass records existing message counts **without** emitting, so
    /// a (re)started agent does not replay its entire backlog onto the oplog —
    /// only messages created after boot become deltas (the cold backlog is
    /// served from tier-② disk on the frontend's initial load).
    pub msg_memo_seeded: bool,

    /// Last per-thread turn-status emitted as a `ThreadStatusChanged` delta,
    /// keyed by thread id. The status chokepoint diffs each thread's live
    /// status against this memo every tick and emits only on an actual flip
    /// (the same observe-on-change discipline as the message and vitals
    /// chokepoints), so a `MyTurn`↔`TheirTurn` transition from *any* source —
    /// a web `SendMessage`, the agent's `Send` tool, a TUI reply, the agent
    /// finishing a turn — reaches the backend view (and the web roster) in
    /// milliseconds, not on the next disk flush.
    pub thread_statuses: std::collections::HashMap<String, cp_wire::types::ThreadTurn>,

    /// Whether [`thread_statuses`](Self::thread_statuses) has been seeded from
    /// the threads present at boot. Until it is, the first chokepoint pass
    /// records existing statuses **without** emitting, so a (re)started agent
    /// does not replay every thread's status as a spurious "change".
    pub status_memo_seeded: bool,
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
pub struct BridgeModule;

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
        let active = std::env::var("CP_BRIDGE").as_deref() == Ok("1");
        if !active {
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
            Ok(boot) => {
                log::info!(
                    "bridge: activated for {} ({})",
                    boot.id(),
                    folder.display(),
                );




                // Bind a dedicated tee socket for live token streaming
                // (separate from the command socket in Boot).
                let tee = match setup_tee(boot.entry()) {
                    Ok(t) => {
                        log::info!("bridge: stream tee ready");
                        Some(t)
                    }
                    Err(e) => {
                        log::error!("bridge: tee setup failed: {e}");
                        None
                    }
                };

                // Set the command listener to non-blocking so the main-loop
                // poll never stalls when no commander is connected.
                let _nb = boot.listener().set_nonblocking(true);

                // Seed the command intake from the oplog replay (populates
                // the SeenSet for dedup across deadman re-exec).
                let intake = match Intake::new(
                    std::path::Path::new(&boot.entry().oplog_path),
                    boot.cap_token().to_owned(),
                ) {
                    Ok(i) => {
                        log::info!("bridge: command intake ready");
                        Some(i)
                    }
                    Err(e) => {
                        log::error!("bridge: intake setup failed: {e:?}");
                        None
                    }
                };

                // Open the content-addressed body store under the oplog dir,
                // so the message chokepoint can durably stage bodies before
                // referencing them (I13). A failure here only disables live
                // message deltas (the disk path still carries the messages).
                let store = match Store::open(std::path::Path::new(&boot.entry().oplog_path)) {
                    Ok(s) => Some(s),
                    Err(e) => {
                        log::error!("bridge: body store open failed: {e:?}");
                        None
                    }
                };

                state.set_ext(BridgeState {
                    boot: Some(boot),
                    tee,
                    intake,
                    store,
                    ..Default::default()
                });
            }
            Err(e) => {
                log::error!("bridge: boot failed: {e:?}");
                state.set_ext(BridgeState::default());
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
        publish_frame(state, StreamKind::PhaseHint {
            phase: cp_wire::types::Phase::Idle,
        });
    }

    fn on_stream_chunk(&self, text: &str, state: &mut State) {
        publish_frame(state, StreamKind::Token { text: text.to_owned() });
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

/// Name of the dedicated tee socket inside the agent folder.
///
/// Separate from `stream.sock` (which Boot binds for command intake in Phase
/// 10). The tee socket carries only outbound [`StreamFrame`]s to an observing
/// backend.
const TEE_SOCKET: &str = "tee.sock";

/// Bind `tee.sock` in the agent folder and spawn the [`Tee`] publisher.
fn setup_tee(entry: &cp_wire::types::registry::Entry) -> std::io::Result<Tee> {
    let tee_path = std::path::Path::new(&entry.folder).join(TEE_SOCKET);
    let _ignored = std::fs::remove_file(&tee_path);
    let listener = std::os::unix::net::UnixListener::bind(&tee_path)?;
    Ok(Tee::spawn(listener))
}

/// Build a [`StreamFrame`] from the given `kind` and publish it to the tee.
///
/// Silently returns if the bridge is OFF or the tee is absent.
///
/// The frame is tagged with the **active streaming message's id** — the id of
/// the last message in the conversation, which is precisely the assistant
/// message being built while tokens stream (the streaming pipeline appends each
/// chunk to `state.messages.last_mut()`). Carrying that id lets the frontend
/// route live `Token` frames to the right conversation bubble and reconcile the
/// streamed text against the durable `MessageCreated` entry (which references
/// the same `Message::id`). `thread_id` stays empty: main-loop streaming is the
/// agent's own conversation, not a thread reply (thread replies go through the
/// `Send` tool, not the token stream).
fn publish_frame(state: &mut State, kind: StreamKind) {
    // Read the active streaming message id BEFORE the mutable `ext_mut` borrow
    // (a short clone, negligible against the LLM/network cost of a chunk).
    let message_id = state
        .messages
        .last()
        .map(|m| m.id.clone())
        .unwrap_or_default();

    let bs = state.ext_mut::<BridgeState>();

    let active = bs.tee.is_some() && bs.boot.is_some();
    if !active {
        return;
    }

    let seq = bs.tee_seq;
    bs.tee_seq = seq.wrapping_add(1);

    let agent_id = bs
        .boot
        .as_ref()
        .map(|b| b.id().to_owned())
        .unwrap_or_default();

    let frame = StreamFrame {
        schema_version: 1,
        agent_id,
        worker_id: String::new(),
        thread_id: String::new(),
        message_id,
        seq,
        kind,
    };

    if let Some(tee) = &bs.tee {
        let _outcome = tee.publish(frame);
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
        assert!(bs.store.is_none());
        assert!(bs.thread_msg_counts.is_empty());
        assert!(!bs.msg_memo_seeded);
        assert!(bs.thread_statuses.is_empty());
        assert!(!bs.status_memo_seeded);
    }
}
