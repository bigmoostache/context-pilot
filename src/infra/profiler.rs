//! Simple profiler for identifying slow operations.
//!
//! Usage:
//!   let _guard = `profile!("operation_name`");
//!   // ... code to measure ...
//!   // automatically logs when guard drops if > threshold
//!
//! View results: tail -f .context-pilot/perf.log
//!
//! ## Hierarchical names
//!
//! While perf monitoring is on, every guard's perf key is its full nesting
//! path: `<parent>.<leaf>`. The parent is the innermost open guard on this
//! thread, else the main-loop step in flight (`loop.<step>`, from the
//! watchdog). So `ui::render` opened during `loop.input` records as
//! `loop.input.ui_render`, and a guard nested inside it as
//! `loop.input.ui_render.<leaf>`. A name with N dots is therefore included in
//! the name with N-1 dots that prefixes it: only siblings may be summed.

use cp_base::cast::Safe as _;
use cp_base::panels::time_arith;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// Minimum duration (ms) before an operation is logged to disk.
const THRESHOLD_MS: u128 = 5;
/// Path to the on-disk performance log file.
const LOG_FILE: &str = ".context-pilot/perf.log";

thread_local! {
    /// Full perf keys of the guards currently open on this thread, innermost last.
    static PATH: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
}

/// Interned full path names. The set is bounded (static leaves × loop steps,
/// plus one leaf per panel kind), so leaking each distinct name once is fine
/// and keeps `record_op`'s `&'static str` key. Also used by callers that build
/// a dynamic leaf (e.g. `refresh_<kind>`) for [`profile!`](crate::profile!).
pub(crate) fn intern(full: String) -> &'static str {
    static NAMES: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();
    let mut names =
        NAMES.get_or_init(|| Mutex::new(HashMap::new())).lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(&name) = names.get(&full) {
        return name;
    }
    let leaked: &'static str = Box::leak(full.clone().into_boxed_str());
    let _prev = names.insert(full, leaked);
    leaked
}

/// Guard for a runtime-built leaf `<prefix><id>` (e.g. `save_mod_tree`).
///
/// Formats and interns the name only while perf monitoring is on; otherwise
/// falls back to the bare `prefix`, so the hot path stays allocation-free.
pub(crate) fn dyn_guard(prefix: &'static str, id: &str) -> ProfileGuard {
    let leaf = if crate::ui::perf::PERF.enabled.load(std::sync::atomic::Ordering::Relaxed) {
        intern(format!("{prefix}{id}"))
    } else {
        prefix
    };
    ProfileGuard::new(leaf)
}

/// Memo for [`nested_name`]: `(parent addr, leaf addr, leaf len)` → full key.
type NestedMemo = RefCell<HashMap<(usize, usize, usize), &'static str>>;

/// `<parent>.<leaf>` as an interned key, memoised per thread by the two
/// `&'static str` addresses.
///
/// Every guard used to `format!` + lock the global [`intern`] map on creation,
/// costing several µs per guard and showing up as phantom "uncovered" parent
/// time (e.g. `agents_emit`, 9 guards per tick). A hit is now one
/// thread-local hash lookup with no allocation.
fn nested_name(parent: &'static str, leaf: &'static str) -> &'static str {
    thread_local! {
        static NESTED: NestedMemo = RefCell::new(HashMap::new());
    }
    let key = (parent.as_ptr().addr(), leaf.as_ptr().addr(), leaf.len());
    if let Some(name) = NESTED.with(|m| m.borrow().get(&key).copied()) {
        return name;
    }
    let name = intern(format!("{parent}.{}", leaf.replace("::", "_")));
    let _prev = NESTED.with(|m| m.borrow_mut().insert(key, name));
    name
}

/// RAII guard that records elapsed time on drop.
pub(crate) struct ProfileGuard {
    /// Leaf name, as written at the call site (used for the slow-op file log).
    leaf: &'static str,
    /// Full hierarchical perf key (`leaf` itself when not nested).
    name: &'static str,
    /// Whether `name` was pushed on [`PATH`] (popped on drop).
    pushed: bool,
    /// Whether the drop records into PERF (false for rootless off-main spans).
    record: bool,
    /// Instant when the guard was created.
    start: Instant,
}

impl ProfileGuard {
    /// Create a new profile guard for the given operation name.
    pub(crate) fn new(leaf: &'static str) -> Self {
        let mut guard = Self { leaf, name: leaf, pushed: false, record: true, start: Instant::now() };
        if !crate::ui::perf::PERF.enabled.load(std::sync::atomic::Ordering::Relaxed) {
            return guard;
        }
        let parent_key =
            PATH.with(|p| p.borrow().last().copied()).or_else(crate::app::run::tools::watchdog::current_perf_step);
        match parent_key {
            Some(parent) => guard.name = nested_name(parent, leaf),
            // Spans in shared code (e.g. the console client) also run on
            // pollers/workers; a rootless off-main span would pollute the
            // main-loop profile, so it records nothing.
            None if std::thread::current().name() != Some("main") => {
                guard.record = false;
                return guard;
            }
            None => {}
        }
        PATH.with(|p| p.borrow_mut().push(guard.name));
        guard.pushed = true;
        guard.start = Instant::now();
        guard
    }
}

impl Drop for ProfileGuard {
    fn drop(&mut self) {
        if !self.record {
            return;
        }
        let elapsed = self.start.elapsed();
        let us = elapsed.as_micros().to_u64();
        let ms = time_arith::us_to_ms(us);

        if self.pushed {
            let _popped = PATH.with(|p| p.borrow_mut().pop());
        }

        // Always record to in-memory perf system
        crate::ui::perf::PERF.record_op(self.name, us);

        // Log to file only for slow operations (written by the bg `perf-log` thread)
        if u128::from(ms) >= THRESHOLD_MS {
            log_slow(format!("{:>6}ms  {}", ms, self.leaf));
        }
    }
}

/// Size past which `perf.log` is rotated to [`LOG_ROTATED`] (it once grew to
/// 566 MB with no rotation).
const LOG_MAX_BYTES: u64 = 32 * 1024 * 1024;
/// Previous `perf.log` generation, overwritten at each rotation.
const LOG_ROTATED: &str = ".context-pilot/perf.log.1";

/// Queue a slow-op line for the background `perf-log` writer, so the main loop
/// never opens or writes the file itself. Dropped if the thread failed to spawn.
fn log_slow(line: String) {
    static TX: OnceLock<Option<std::sync::mpsc::Sender<String>>> = OnceLock::new();
    let tx = TX.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        std::thread::Builder::new().name("perf-log".to_owned()).spawn(move || write_loop(&rx)).ok().map(|_h| tx)
    });
    if let Some(sender) = tx.as_ref() {
        let _r = sender.send(line);
    }
}

/// Append queued lines to `perf.log`, keeping the file open between lines and
/// rotating it once it passes [`LOG_MAX_BYTES`]. No `profile!` here: this runs
/// off the main thread and must not feed the profiler it serves.
fn write_loop(rx: &std::sync::mpsc::Receiver<String>) {
    let mut file: Option<std::fs::File> = None;
    for line in rx {
        if file.is_none() {
            file = OpenOptions::new().create(true).append(true).open(LOG_FILE).ok();
        }
        let Some(f) = file.as_mut() else { continue };
        let _r = writeln!(f, "{line}");
        if f.metadata().is_ok_and(|m| m.len() > LOG_MAX_BYTES) {
            file = None;
            let _r2 = std::fs::rename(LOG_FILE, LOG_ROTATED);
        }
    }
}

/// Path to the on-disk tool execution log directory.
const TOOL_LOG_DIR: &str = ".context-pilot/logs";
/// Path to the on-disk tool execution log.
const TOOL_LOG_FILE: &str = ".context-pilot/logs/tool-times.log";

/// Log a tool execution's elapsed time.
///
/// Appends a line to `.context-pilot/logs/tool-times.log` with the local datetime,
/// elapsed milliseconds, and tool name(s). Creates the directory if needed.
/// Silently no-ops on any I/O error.
pub(crate) fn log_tool_time(tool_name: &str, elapsed: std::time::Duration) {
    let ms = elapsed.as_millis();
    drop(std::fs::create_dir_all(TOOL_LOG_DIR));
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(TOOL_LOG_FILE) {
        let ts = cp_mod_utilities::time::now_local_ymd_hms();
        let _r = writeln!(file, "{ts}  {ms:>6}ms  {tool_name}");
    }
}

/// Create a profiling guard that logs slow operations on drop.
///
/// Records timing to the in-memory perf system under its hierarchical name
/// (see the module docs), and writes to `.context-pilot/perf.log` if the
/// operation exceeds 5 ms.
#[macro_export]
macro_rules! profile {
    ($name:expr) => {
        $crate::infra::profiler::ProfileGuard::new($name)
    };
}

thread_local! {
    /// Interned `act_<Variant>` span names (one leak per distinct variant).
    static VARIANT_NAMES: RefCell<HashMap<String, &'static str>> = RefCell::new(HashMap::new());
}

/// Span named `act_<Variant>` for an enum value, from its `Debug` output up to
/// the first `(`/`{`/space. `None` (no cost beyond one atomic load) unless
/// monitoring is on.
pub(crate) fn variant_span<T>(value: &T) -> Option<ProfileGuard>
where
    T: std::fmt::Debug,
{
    if !crate::ui::perf::PERF.enabled.load(std::sync::atomic::Ordering::Relaxed) {
        return None;
    }
    let dbg = format!("{value:?}");
    let variant = dbg.split(['(', '{', ' ']).next().unwrap_or("unknown");
    let name = VARIANT_NAMES.with(|cell| {
        let mut names = cell.borrow_mut();
        if let Some(&known) = names.get(variant) {
            return known;
        }
        let leaked: &'static str = Box::leak(format!("act_{variant}").into_boxed_str());
        let _prev = names.insert(variant.to_owned(), leaked);
        leaked
    });
    Some(ProfileGuard::new(name))
}
