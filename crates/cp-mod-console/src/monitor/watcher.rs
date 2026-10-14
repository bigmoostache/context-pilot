//! `LiveMonitor`: the persistent spine watcher behind one `Monitor` call.
//!
//! It stays registered after firing (`is_persistent`) and removes itself
//! through `suicide` once it has ended: fired once, flooded, process exited,
//! or its console vanished. Each ending is announced in the last notification.

use std::fmt::Write as _;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use cp_base::panels::now_ms;
use cp_base::state::runtime::State;
use cp_base::state::watchers::Watcher;
use cp_base::state::watchers::carriers::WatcherResult;

use super::engine::{Budget, Engine, FLOOD_MAX_EVENTS, Filter, render_payload};
use super::spec::{Frequency, Mode, Record, Status};
use crate::types::{ConsoleState, ProcessStatus};

/// Wait after process exit before the final flush, so the file poller's
/// last read (300 ms grace) has landed in the ring buffer (ms).
const EXIT_GRACE_MS: u64 = 600;

/// Ending reason for `frequency=once`.
const ONCE_REASON: &str = "fired once (frequency=once)";

/// Mutable per-monitor state, behind a mutex because `check` takes `&self`.
#[derive(Debug, Default)]
struct Runtime {
    /// Line splitting, batching and flood accounting.
    engine: Engine,
    /// Ring-buffer offset already consumed.
    cursor: u64,
    /// First tick that saw the process terminal.
    exited_at_ms: Option<u64>,
    /// Set once ended; `suicide` then drops the watcher.
    dead: bool,
    /// Notifications sent.
    deliveries: u32,
}

/// Spine watcher for one monitor.
#[derive(Debug)]
pub struct LiveMonitor {
    /// Registry id (`monitor_<id>`).
    watcher_id: String,
    /// Spine-panel label.
    desc: String,
    /// Notification prefix: `[Monitor M<n> · <description>]`.
    label: String,
    /// Spill-file stem prefix: `<session>_<monitor id>`.
    stem: String,
    /// Record shared with the Monitor panel.
    record: Arc<Mutex<Record>>,
    /// Console session key read.
    session: String,
    /// Console panel id, for messages.
    panel_id: String,
    /// Firing condition.
    mode: Mode,
    /// Once or ongoing.
    frequency: Frequency,
    /// Compiled `pattern` (`mode=pattern`).
    regex: Option<regex::Regex>,
    /// Agent that registered the monitor; notifications go to it.
    agent: Option<String>,
    /// Registration time (ms since epoch).
    registered_at_ms: u64,
    /// Mutable state.
    rt: Mutex<Runtime>,
    /// Notifications built so far; makes each spine source unique.
    notified: AtomicU32,
}

/// Lock ignoring poison: monitor state stays usable after a panic elsewhere.
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl LiveMonitor {
    /// Build the watcher for `record`, reading output past `cursor`.
    #[must_use]
    pub fn new(record: Arc<Mutex<Record>>, cursor: u64, agent: Option<String>) -> Self {
        let snapshot = lock(&record).clone();
        let id = &snapshot.monitor_id;
        let regex = snapshot.params.pattern.as_deref().and_then(|p| regex::Regex::new(p).ok());
        Self {
            watcher_id: format!("monitor_{id}"),
            desc: format!("\u{1f4e1} Monitor {id} \u{b7} {}", snapshot.params.description),
            label: format!("[Monitor {id} \u{b7} {}]", snapshot.params.description),
            stem: format!("{}_{id}", snapshot.session),
            record,
            session: snapshot.session.clone(),
            panel_id: snapshot.panel_id.clone(),
            mode: snapshot.params.mode,
            frequency: snapshot.params.frequency,
            regex,
            agent,
            registered_at_ms: snapshot.started_at_ms,
            rt: Mutex::new(Runtime { cursor, ..Runtime::default() }),
            notified: AtomicU32::new(0),
        }
    }

    /// Line filter for this monitor.
    const fn filter(&self) -> Filter<'_> {
        Filter { mode: self.mode, pattern: self.regex.as_ref() }
    }

    /// Mark the monitor ended, in the runtime and in the panel record.
    fn end(&self, rt: &mut Runtime, reason: &str) {
        rt.dead = true;
        lock(&self.record).status = Status::Ended { reason: reason.to_owned(), at_ms: now_ms() };
    }

    /// Publish the delivery count to the panel record.
    fn set_events(&self, n: u32) {
        lock(&self.record).events = n;
    }

    /// Notification labelled with the monitor id and description; `ending`
    /// appends the reason the monitor stopped.
    fn notify(&self, body: &str, ending: Option<&str>) -> WatcherResult {
        let mut text = format!("{} {body}", self.label);
        if let Some(reason) = ending {
            let _w = write!(text, "\nThis monitor has ended: {reason}.");
        }
        let seq = self.notified.fetch_add(1, Ordering::Relaxed);
        let res =
            WatcherResult::new(text).panel_id(self.panel_id.clone()).source(format!("{} #{seq}", self.watcher_id));
        match self.agent.as_ref() {
            Some(a) => res.agent_id(a.clone()),
            None => res,
        }
    }

    /// Count a delivery, decide whether it ends the monitor, and build its notification.
    fn deliver(&self, rt: &mut Runtime, lines: &[String], exit_reason: Option<String>) -> WatcherResult {
        rt.deliveries = rt.deliveries.saturating_add(1);
        let seq = rt.deliveries;
        self.set_events(seq);
        let ending = if self.frequency == Frequency::Once {
            Some(ONCE_REASON.to_owned())
        } else if rt.engine.record_fire(now_ms()) == Budget::Flooded {
            Some(format!("it flooded the notification system (more than {FLOOD_MAX_EVENTS} events within one hour)"))
        } else {
            exit_reason
        };
        if let Some(reason) = ending.as_deref() {
            self.end(rt, reason);
        }
        let noun = if lines.len() == 1 { "line" } else { "lines" };
        let payload = render_payload(&lines.join("\n"), &format!("{}_{seq}", self.stem));
        self.notify(&format!("{} new {noun} in {}:\n{payload}", lines.len(), self.panel_id), ending.as_deref())
    }

    /// `mode=exit`: fire once the process is terminal.
    fn check_exit(&self, rt: &mut Runtime, status: ProcessStatus, last: &str) -> Option<WatcherResult> {
        if !status.is_terminal() {
            return None;
        }
        // Let the file poller's final read land before quoting the last output.
        let now = now_ms();
        let since = *rt.exited_at_ms.get_or_insert(now);
        if now.saturating_sub(since) < EXIT_GRACE_MS {
            return None;
        }
        rt.deliveries = 1;
        self.set_events(1);
        self.end(rt, ONCE_REASON);
        let body = render_payload(
            &format!("Process in {} exited ({}). Last output:\n{last}", self.panel_id, status.label()),
            &format!("{}_exit", self.stem),
        );
        Some(self.notify(&body, Some(ONCE_REASON)))
    }

    /// One poll with the runtime already locked.
    fn step(&self, rt: &mut Runtime, state: &State) -> Option<WatcherResult> {
        if rt.dead {
            return None;
        }
        let Some(handle) = ConsoleState::get(state).sessions.get(&self.session) else {
            let reason = format!("its console {} was closed or killed", self.panel_id);
            self.end(rt, &reason);
            return Some(
                self.notify("Console gone: monitor destroyed. Re-create it if that was not intended.", Some(&reason)),
            );
        };
        let status = handle.get_status();
        if self.mode == Mode::Exit {
            return self.check_exit(rt, status, &handle.buffer.last_n_lines(5));
        }

        let now = now_ms();
        let (bytes, cursor) = handle.buffer.read_since(rt.cursor);
        rt.cursor = cursor;
        rt.engine.ingest(&bytes, self.filter(), now);
        let exit_final = status.is_terminal() && {
            let since = *rt.exited_at_ms.get_or_insert(now);
            now.saturating_sub(since) >= EXIT_GRACE_MS
        };
        if exit_final {
            rt.engine.flush_partial(self.filter(), now);
        }
        let batch = rt.engine.take_batch(now, exit_final);
        let exit_reason = exit_final.then(|| format!("the process in {} exited ({})", self.panel_id, status.label()));
        match (batch, exit_reason) {
            (None, None) => None,
            (None, Some(reason)) => {
                self.end(rt, &reason);
                Some(self.notify("No further output.", Some(&reason)))
            }
            (Some(lines), reason) => Some(self.deliver(rt, &lines, reason)),
        }
    }
}

impl Watcher for LiveMonitor {
    fn id(&self) -> &str {
        &self.watcher_id
    }

    fn description(&self) -> &str {
        &self.desc
    }

    fn is_blocking(&self) -> bool {
        false
    }

    fn tool_use_id(&self) -> Option<&str> {
        None
    }

    fn check(&self, state: &State) -> Option<WatcherResult> {
        self.step(&mut lock(&self.rt), state)
    }

    fn check_timeout(&self) -> Option<WatcherResult> {
        None
    }

    fn registered_ms(&self) -> u64 {
        self.registered_at_ms
    }

    fn source_tag(&self) -> &'static str {
        "monitor"
    }

    fn suicide(&self, _state: &State) -> bool {
        lock(&self.rt).dead
    }

    fn is_easy_bash(&self) -> bool {
        false
    }

    fn is_persistent(&self) -> bool {
        true
    }

    fn fire_at_ms(&self) -> Option<u64> {
        None
    }

    fn message(&self) -> Option<&str> {
        None
    }

    fn agent_id(&self) -> Option<&str> {
        None
    }

    fn interval_ms(&self) -> u64 {
        0
    }

    fn recurrence_label(&self) -> Option<&str> {
        None
    }
}
