//! Web frontend (Nestor): the second `InputSource`/`OutputSink` pair.
//!
//! - [`WebSource`] drains `WebEvent`s coming from the axum thread and turns
//!   them into `Action`s / direct mutations (incoming face of the contract);
//! - [`WebSink`] builds the `WebState` view-model when the state is dirty
//!   and broadcasts section deltas (outgoing face).
//!
//! Both sit on the same generic loop as the TUI — the core cannot tell who
//! is driving.

/// `build_web_state()` — WebState section builders.
pub(crate) mod build;
/// `WebCommand` → `Action` mapping + query answering.
pub(crate) mod commands;

use std::collections::HashMap;
use std::hash::{Hash as _, Hasher as _};
use std::io;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use cp_web_server::WebHandle;
use cp_web_server::protocol::{WebEvent, WireFrame};
use serde_json::{Value, json};

use crate::app::App;
use crate::app::frontend::{InputSource, OutputSink, PumpFlow};
use crate::app::run::lifecycle::EventChannels;

/// Name of the active project (workspace system), set once at boot.
static PROJECT_NAME: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Record the active project name (read by the `meta` section).
pub(crate) fn set_project_name(name: &str) {
    let _r = PROJECT_NAME.set(name.to_string());
}

/// The active project name, if the workspace system is enabled.
pub(crate) fn project_name() -> Option<&'static str> {
    PROJECT_NAME.get().map(String::as_str)
}

/// Hash a JSON value's canonical string form.
fn value_hash(value: &Value) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.to_string().hash(&mut hasher);
    hasher.finish()
}

// ─── Input ──────────────────────────────────────────────────────────────────

/// Web input source: drains commands/queries/connections from the server.
pub(crate) struct WebSource {
    /// Events from the axum thread.
    events_rx: Receiver<WebEvent>,
    /// Event buffered by [`InputSource::wait`] for the next pump.
    buffered: Option<WebEvent>,
    /// Outbound frames (snapshots and query results are sent from here).
    handle: WebHandle,
    /// Projects root (`--projects-dir`) — needed to validate switches.
    projects_dir: Option<std::path::PathBuf>,
}

impl WebSource {
    /// Create a web input source from the server's channels.
    pub(crate) const fn new(
        events_rx: Receiver<WebEvent>,
        handle: WebHandle,
        projects_dir: Option<std::path::PathBuf>,
    ) -> Self {
        Self { events_rx, buffered: None, handle, projects_dir }
    }

    /// Dispatch one event into the app.
    fn dispatch(&self, app: &mut App, ch: &EventChannels<'_>, event: WebEvent) {
        match event {
            WebEvent::Command(cmd) => commands::apply_command(app, ch.tx, cmd),
            WebEvent::Query { conn_id, req_id, query } => {
                let frame = commands::answer_query(&app.state, &req_id, query);
                self.handle.send(WireFrame::to_conn(conn_id, frame));
            }
            WebEvent::Connected { conn_id } => {
                let snapshot = build::snapshot_json(&app.state);
                self.handle.send(WireFrame::to_conn(conn_id, snapshot));
            }
            WebEvent::SwitchProject { name } => self.switch_project(app, &name),
        }
    }

    /// Switch the active project: persist the pointer, warn the clients,
    /// then let the existing reload path exec-restart into the new cwd.
    fn switch_project(&self, app: &mut App, name: &str) {
        let Some(root) = &self.projects_dir else { return };
        if !cp_web_server::projects::valid_name(name) || !root.join(name).is_dir() {
            self.handle.send(WireFrame::broadcast(
                serde_json::json!({ "t": "error", "message": format!("projet inconnu : {name}") }).to_string(),
            ));
            return;
        }
        if let Err(e) = cp_web_server::projects::write_current(root, name) {
            log::error!("[web] switch: cannot write pointer: {e:?}");
            return;
        }
        self.handle.send(WireFrame::broadcast(
            serde_json::json!({ "t": "bye", "reason": "switch", "project": name }).to_string(),
        ));
        app.switch_pending = true;
        app.state.flags.lifecycle.reload_pending = true;
    }
}

impl InputSource for WebSource {
    fn pump(&mut self, app: &mut App, ch: &EventChannels<'_>) -> io::Result<PumpFlow> {
        let mut handled = self.buffered.take().is_some_and(|event| {
            self.dispatch(app, ch, event);
            true
        });
        while let Ok(event) = self.events_rx.try_recv() {
            self.dispatch(app, ch, event);
            handled = true;
        }
        Ok(if handled { PumpFlow::Handled } else { PumpFlow::Idle })
    }

    fn wait(&mut self, ms: u64) {
        // Sleep on the channel: wakes early when a web event arrives.
        if self.buffered.is_none()
            && let Ok(event) = self.events_rx.recv_timeout(Duration::from_millis(ms))
        {
            self.buffered = Some(event);
        }
    }
}

// ─── Output ─────────────────────────────────────────────────────────────────

/// Per-section content hashes from the previous broadcast.
#[derive(Default)]
struct SectionHashes {
    /// `status` section.
    status: u64,
    /// `panels` section.
    panels: u64,
    /// `active_panel` section.
    active_panel: u64,
    /// `question_form` section.
    question_form: u64,
    /// `input_draft` string.
    input_draft: u64,
}

/// Web output sink: diffs the `WebState` sections and broadcasts deltas.
pub(crate) struct WebSink {
    /// Outbound frame channel.
    handle: WebHandle,
    /// Previous section hashes.
    sections: SectionHashes,
    /// Previous per-message fingerprints, keyed by message ID.
    msg_prints: HashMap<String, u64>,
    /// Previous message ID order (for removal detection).
    msg_order: Vec<String>,
    /// `(id, content)` of the last streamed message, for the append fast-path.
    tail: Option<(String, String)>,
}

impl WebSink {
    /// Create a web sink writing to the server's broadcast channel.
    pub(crate) fn new(handle: WebHandle) -> Self {
        Self { handle, sections: SectionHashes::default(), msg_prints: HashMap::new(), msg_order: Vec::new(), tail: None }
    }

    /// Diff a section; insert it into the delta when it changed.
    fn diff_section(delta: &mut serde_json::Map<String, Value>, prev: &mut u64, key: &str, value: Value) {
        let next = value_hash(&value);
        if next != *prev {
            *prev = next;
            drop(delta.insert(key.to_string(), value));
        }
    }

    /// Try the streaming fast-path: if the only conversation change is the
    /// last message's content growing by a suffix, emit `append` instead of
    /// re-sending the whole message. Returns `true` when it applied.
    fn try_append_fast_path(&mut self, app: &App, changed_ids: &[String]) -> bool {
        let [only_id] = changed_ids else { return false };
        let Some(last) = app.state.messages.last() else { return false };
        if &last.id != only_id {
            return false;
        }
        let Some((tail_id, tail_content)) = &self.tail else { return false };
        if tail_id != only_id || !last.content.starts_with(tail_content.as_str()) {
            return false;
        }
        let Some(suffix) = last.content.get(tail_content.len()..) else { return false };
        if suffix.is_empty() {
            return false; // content unchanged — some other field moved, full upsert
        }
        let frame = json!({ "t": "append", "id": last.id, "text": suffix }).to_string();
        self.handle.send(WireFrame::broadcast(frame));
        self.tail = Some((last.id.clone(), last.content.clone()));
        true
    }

    /// Compute conversation upserts/removals and update the fingerprints.
    fn diff_conversation(&mut self, app: &App) -> (Vec<String>, Vec<String>) {
        let mut changed: Vec<String> = Vec::new();
        let mut next_prints = HashMap::with_capacity(app.state.messages.len());
        let mut next_order = Vec::with_capacity(app.state.messages.len());
        for msg in &app.state.messages {
            let print = build::message_fingerprint(msg);
            if self.msg_prints.get(&msg.id) != Some(&print) {
                changed.push(msg.id.clone());
            }
            _ = next_prints.insert(msg.id.clone(), print);
            next_order.push(msg.id.clone());
        }
        let removed: Vec<String> =
            self.msg_order.iter().filter(|id| !next_prints.contains_key(*id)).cloned().collect();
        self.msg_prints = next_prints;
        self.msg_order = next_order;
        (changed, removed)
    }

    /// Refresh the append-tail cache from the current last message.
    fn refresh_tail(&mut self, app: &App) {
        self.tail = app.state.messages.last().map(|msg| (msg.id.clone(), msg.content.clone()));
    }
}

impl OutputSink for WebSink {
    fn present(&mut self, app: &mut App) -> io::Result<()> {
        // No client connected → skip all serialization work.
        if self.handle.frames.receiver_count() == 0 {
            return Ok(());
        }

        let mut delta = serde_json::Map::new();
        Self::diff_section(&mut delta, &mut self.sections.status, "status", build::status_value(&app.state));
        Self::diff_section(&mut delta, &mut self.sections.panels, "panels", build::panels_value(&app.state));
        Self::diff_section(
            &mut delta,
            &mut self.sections.active_panel,
            "active_panel",
            build::active_panel_value(&app.state),
        );
        Self::diff_section(
            &mut delta,
            &mut self.sections.question_form,
            "question_form",
            build::question_form_value(&app.state),
        );
        Self::diff_section(&mut delta, &mut self.sections.input_draft, "input_draft", json!(app.state.input));

        let (changed, removed) = self.diff_conversation(app);
        let appended = removed.is_empty() && delta.is_empty() && self.try_append_fast_path(app, &changed);
        if !appended && (!changed.is_empty() || !removed.is_empty()) {
            let upserts: Vec<Value> = app
                .state
                .messages
                .iter()
                .filter(|msg| changed.contains(&msg.id))
                .map(build::message_value)
                .collect();
            if !upserts.is_empty() {
                drop(delta.insert("conversation_upsert".to_string(), json!(upserts)));
            }
            if !removed.is_empty() {
                drop(delta.insert("conversation_remove".to_string(), json!(removed)));
            }
            self.refresh_tail(app);
        }

        if !delta.is_empty() {
            drop(delta.insert("t".to_string(), json!("delta")));
            self.handle.send(WireFrame::broadcast(Value::Object(delta).to_string()));
        }
        Ok(())
    }
}
