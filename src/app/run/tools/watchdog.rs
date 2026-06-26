//! Interactive main-loop watchdog — wedge detection + automatic diagnostics.
//!
//! The interactive TUI runs on a **single thread**: one loop polls input, runs
//! a long list of background steps (bridge, stream drain, tool execution, panel
//! refresh, spine, reverie), redraws, and sleeps a few ms. Every step runs
//! *inline* on that one thread, so any synchronous block (a tool hitting a hung
//! dependency, a slow panel rehash, a lock wait, a stalled socket read) freezes
//! the **entire** UI for its full duration — and historically there was zero
//! visibility into *which* step wedged. This module ends the guessing.
//!
//! It is the interactive sibling of the headless deadman, but **purely
//! observational**: it NEVER terminates, re-execs, or signals the process — a
//! human is at the keyboard, so killing their session would be unacceptable. It
//! only writes a diagnostic to `.context-pilot/errors/` so a freeze that "had no
//! apparent reason" becomes "the log says it wedged in `<step>` for `<N>s` at
//! `<CPU%>`".
//!
//! ## Two cooperating detectors
//!
//! 1. **Heartbeat watchdog** — *is the loop alive?* The loop bumps
//!    [`beat`] at the top of every iteration. A healthy loop ticks at least
//!    every ~50 ms (even while idle it only parks on a 50 ms input poll), so if
//!    the heartbeat goes stale past [`WEDGE_STALL_MS`] the loop is genuinely
//!    wedged on a synchronous call.
//! 2. **Activity watchdog** — *what is it doing, and for how long?* The loop
//!    calls [`mark`] with the [`Step`] it is about to run. If the current step
//!    has been in flight past [`SLOW_STEP_MS`] the dump names the exact culprit
//!    step (the heartbeat freezes at the same instant since it's the same
//!    thread, so the two detectors corroborate each other).
//!
//! On a trip the dump auto-captures the single observation that cracks these
//! cases: **process CPU%** — high ⇒ a busy-loop/runaway computation, low ⇒ a
//! blocked syscall or deadlock — plus per-thread states (Linux `/proc`) or a
//! real backtrace of the wedged thread (macOS `sample`).
//!
//! ## Cost on the happy path
//!
//! [`beat`]/[`mark`] are two `Relaxed` atomic stores (~ns each); the monitor
//! thread sleeps [`POLL_SECS`] and does a few cheap atomic reads. Disk and
//! subprocesses are touched **only** when a wedge is detected (rare). There is
//! no behavioural change whatsoever while the loop is healthy.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::time::Duration;

use cp_base::panels::now_ms;

/// How often the monitor thread wakes to inspect the loop's liveness.
const POLL_SECS: u64 = 2;
/// Heartbeat staleness past which the loop is declared wedged (total freeze).
/// A healthy loop ticks every ≤50 ms, so 15 s is unambiguous.
const WEDGE_STALL_MS: u64 = 15_000;
/// A single step in flight longer than this is flagged as the wedge culprit.
/// Slightly below [`WEDGE_STALL_MS`] so the dump names the step a touch earlier.
const SLOW_STEP_MS: u64 = 12_000;
/// Window over which process CPU usage is sampled on Linux (busy-loop signal).
#[cfg(target_os = "linux")]
const CPU_SAMPLE_MS: u32 = 500;

/// Wall-clock ms of the loop's most recent iteration (0 = never ticked yet).
static LOOP_HEARTBEAT_MS: AtomicU64 = AtomicU64::new(0);
/// Discriminant of the [`Step`] the loop is currently executing.
static CURRENT_STEP: AtomicU8 = AtomicU8::new(0);
/// Wall-clock ms at which [`CURRENT_STEP`] was entered.
static STEP_SINCE_MS: AtomicU64 = AtomicU64::new(0);
/// Guards against spawning the monitor thread more than once.
static STARTED: AtomicBool = AtomicBool::new(false);

/// The coarse phase of one main-loop iteration, recorded so a wedge dump can
/// name the step that froze. Ordered to follow the loop's execution sequence.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Step {
    /// Parked on the input poll (the only legitimately blocking wait, ≤50 ms).
    Idle = 0,
    /// Handling a keyboard/mouse event.
    Input = 1,
    /// Servicing the orchestration bridge socket + self-heal.
    Bridge = 2,
    /// Emitting vitals / messages / thread status over the bridge.
    ThreadsEmit = 3,
    /// Draining LLM stream events + retry + typewriter.
    Stream = 4,
    /// Processing background cache-refresh results.
    Cache = 5,
    /// Processing file/GH watcher events.
    Watchers = 6,
    /// Executing a tool (search / `entity_sql` / git / file I/O — prime suspect).
    Tools = 7,
    /// Spine auto-continuation + `MY_TURN` thread detection.
    Spine = 8,
    /// Reverie sub-agent stream + tool dispatch.
    Reverie = 9,
    /// Refreshing all panels (tree rehash / git-status — can be heavy).
    PanelRefresh = 10,
    /// Drawing a frame.
    Render = 11,
    /// Persisting state + ownership check.
    Save = 12,
}

impl Step {
    /// Human-readable label for the diagnostic dump.
    const fn name(self) -> &'static str {
        match self {
            Self::Idle => "Idle (input poll)",
            Self::Input => "Input handling",
            Self::Bridge => "Bridge socket",
            Self::ThreadsEmit => "Thread/vitals emit",
            Self::Stream => "Stream event drain",
            Self::Cache => "Cache updates",
            Self::Watchers => "Watcher events",
            Self::Tools => "Tool execution",
            Self::Spine => "Spine / MY_TURN",
            Self::Reverie => "Reverie sub-agent",
            Self::PanelRefresh => "Panel refresh (tree/git)",
            Self::Render => "Render frame",
            Self::Save => "State persistence",
        }
    }

    /// Decode a stored discriminant, falling back to [`Step::Idle`] for any
    /// unknown byte (forward-compatible, never panics).
    const fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Input,
            2 => Self::Bridge,
            3 => Self::ThreadsEmit,
            4 => Self::Stream,
            5 => Self::Cache,
            6 => Self::Watchers,
            7 => Self::Tools,
            8 => Self::Spine,
            9 => Self::Reverie,
            10 => Self::PanelRefresh,
            11 => Self::Render,
            12 => Self::Save,
            _ => Self::Idle,
        }
    }
}

/// Record a fresh loop tick. Called at the top of every iteration. Two-`Relaxed`
/// atomic stores; effectively free on the happy path.
pub(crate) fn beat() {
    LOOP_HEARTBEAT_MS.store(now_ms(), Ordering::Relaxed);
}

/// Record the step the loop is about to execute, so a wedge dump can name it.
pub(crate) fn mark(step: Step) {
    CURRENT_STEP.store(step as u8, Ordering::Relaxed);
    STEP_SINCE_MS.store(now_ms(), Ordering::Relaxed);
}

/// Spawn the monitor thread (idempotent — repeat calls are no-ops). Detached;
/// it does not block process exit and is reset by the re-exec on a TUI reload.
pub(crate) fn spawn() {
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let builder = std::thread::Builder::new().name("cp-loop-watchdog".to_owned());
    // If the OS refuses the thread we simply forgo watchdog coverage rather than
    // crash the app — the watchdog is a diagnostic aid, never load-bearing.
    let _r = builder.spawn(monitor_loop);
}

/// Monitor thread body: poll the heartbeat + activity marker, dump once per
/// wedge episode. Never returns.
fn monitor_loop() -> ! {
    // The heartbeat value already dumped for — guards against re-dumping the
    // same frozen episode every POLL_SECS. A recovered-then-rewedged loop has a
    // different (advanced) heartbeat, so it dumps afresh.
    let mut last_episode_beat: u64 = 0;
    loop {
        std::thread::sleep(Duration::from_secs(POLL_SECS));
        let now = now_ms();
        let beat = LOOP_HEARTBEAT_MS.load(Ordering::Relaxed);
        if beat == 0 {
            continue; // loop hasn't started ticking yet
        }
        let beat_age = now.saturating_sub(beat);
        let step = Step::from_u8(CURRENT_STEP.load(Ordering::Relaxed));
        let step_age = now.saturating_sub(STEP_SINCE_MS.load(Ordering::Relaxed));

        let wedged = beat_age >= WEDGE_STALL_MS;
        let slow_step = step != Step::Idle && step_age >= SLOW_STEP_MS;
        if (wedged || slow_step) && beat != last_episode_beat {
            last_episode_beat = beat;
            dump_diagnostic(&Wedge { now, beat_age, step, step_age, wedged });
        }
    }
}

/// One wedge episode's observations, bundled so the diagnostic dumper takes a
/// single argument (the loop's frozen snapshot at trip time).
struct Wedge {
    /// Wall-clock ms at which the trip was detected.
    now: u64,
    /// How long the heartbeat had been stale (ms).
    beat_age: u64,
    /// The step in flight when the loop wedged.
    step: Step,
    /// How long that step had been running (ms).
    step_age: u64,
    /// `true` for a total heartbeat stall, `false` for a merely slow step.
    wedged: bool,
}

/// Write a wedge diagnostic to `.context-pilot/errors/watchdog-<ts>.log`.
///
/// Best-effort throughout: every I/O step is allowed to fail silently rather
/// than ever disturb the (already struggling) main process.
fn dump_diagnostic(w: &Wedge) {
    let Wedge { now, beat_age, step, step_age, wedged } = *w;
    let errors_dir = std::path::Path::new("./.context-pilot").join("errors");
    let _mkdir = std::fs::create_dir_all(&errors_dir);

    let cpu = cpu_busy_pct();
    let cpu_line = cpu.map_or_else(|| "CPU usage: (unavailable)".to_owned(), |pct| format!("CPU usage: {pct:.0}%"));
    let interpretation = match cpu {
        Some(p) if p >= 70.0 => {
            format!(
                "→ CPU is HIGH: a busy-loop / runaway computation in step '{}' \
                     (e.g. tree rehash, infinite loop).",
                step.name()
            )
        }
        Some(p) if p <= 15.0 => {
            format!(
                "→ CPU is LOW: a blocked syscall or lock (network/socket/disk/mutex) \
                     in step '{}'.",
                step.name()
            )
        }
        Some(_) => format!("→ CPU is MODERATE: partial stall in step '{}'.", step.name()),
        None => format!("→ inspect step '{}' (CPU sample unavailable).", step.name()),
    };

    let kind = if wedged { "HEARTBEAT_STALE (total wedge)" } else { "SLOW_STEP" };
    let pid = std::process::id();
    let threads = thread_states(&errors_dir, now);

    let body = format!(
        "Interactive main-loop watchdog — wedge detected\n\
         ================================================\n\
         timestamp_ms : {now}\n\
         pid          : {pid}\n\
         trip kind    : {kind}\n\
         current step : {step_name}\n\
         step age     : {step_age} ms\n\
         heartbeat age: {beat_age} ms (loop last ticked {beat_age} ms ago)\n\
         {cpu_line}\n\
         {interpretation}\n\
         \n\
         Per-thread states:\n\
         {threads}\n",
        step_name = step.name(),
    );

    let path = errors_dir.join(format!("watchdog-{now}.log"));
    let _w = std::fs::write(path, body);
}

/// Sample process-wide CPU utilisation as a percentage of one core. The single
/// observation that splits the suspect list in half: high ⇒ busy-loop, low ⇒
/// blocked/deadlock. Returns `None` if it cannot be measured.
#[cfg(target_os = "linux")]
fn cpu_busy_pct() -> Option<f64> {
    /// Linux scheduler tick rate assumed for `/proc` time accounting (the near
    /// universal value on `x86_64`; only used for a coarse diagnostic ratio).
    const CLK_TCK: f64 = 100.0;
    let ticks = || -> Option<u64> {
        let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
        // Fields after the "(comm)" group: utime is index 13, stime index 14.
        let rest = stat.rsplit_once(')').map(|(_, r)| r)?;
        let mut f = rest.split_whitespace();
        let utime: u64 = f.nth(11)?.parse().ok()?;
        let stime: u64 = f.next()?.parse().ok()?;
        Some(utime.saturating_add(stime))
    };
    let t0 = ticks()?;
    std::thread::sleep(Duration::from_millis(u64::from(CPU_SAMPLE_MS)));
    let t1 = ticks()?;
    let delta = u32::try_from(t1.saturating_sub(t0)).unwrap_or(u32::MAX);
    let dticks = f64::from(delta);
    let secs = f64::from(CPU_SAMPLE_MS) / 1000.0;
    Some((dticks / CLK_TCK / secs) * 100.0)
}

/// macOS CPU sample via `ps %cpu` (a recent decaying average — ample to
/// distinguish a pegged core from an idle/blocked one). Returns `None` on error.
#[cfg(target_os = "macos")]
fn cpu_busy_pct() -> Option<f64> {
    let pid = std::process::id().to_string();
    let out = std::process::Command::new("ps").args(["-o", "%cpu=", "-p", &pid]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()?.trim().parse::<f64>().ok()
}

/// Per-thread state lines for the dump.
///
/// On Linux, reads each `/proc/self/task/<tid>/{comm,stat,wchan}` so a thread
/// in state `R` (running — busy-loop) or `D` (uninterruptible sleep) and the
/// kernel function it's parked in (`wchan`) pinpoint the wedge. On macOS, where
/// `/proc` is absent, best-effort spawns `sample <pid>` to capture a real
/// backtrace of the wedged thread to a sibling file, and returns the thread
/// list from `ps -M`.
#[cfg(target_os = "linux")]
fn thread_states(_errors_dir: &std::path::Path, _now: u64) -> String {
    let Ok(entries) = std::fs::read_dir("/proc/self/task") else {
        return "(unavailable)".to_owned();
    };
    let mut lines = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let tid = entry.file_name().to_string_lossy().into_owned();
        let dir = entry.path();
        let comm = std::fs::read_to_string(dir.join("comm")).unwrap_or_default();
        let state = std::fs::read_to_string(dir.join("stat"))
            .ok()
            .and_then(|s| {
                let (_, r) = s.rsplit_once(')')?;
                r.split_whitespace().next().map(str::to_owned)
            })
            .unwrap_or_else(|| "?".to_owned());
        let wchan = std::fs::read_to_string(dir.join("wchan")).unwrap_or_default();
        lines.push(format!("  tid {tid} [{}] state={state} wchan={}", comm.trim(), wchan.trim()));
    }
    lines.join("\n")
}

/// macOS variant — see the Linux doc comment. Captures a `sample` backtrace
/// (works on a wedged process; that's its purpose) and returns `ps -M` output.
#[cfg(target_os = "macos")]
fn thread_states(errors_dir: &std::path::Path, now: u64) -> String {
    let pid = std::process::id().to_string();
    // Best-effort: a 2-second `sample` writes a full backtrace of every thread —
    // the single most useful artifact for a wedge — to a sibling file.
    let sample_path = errors_dir.join(format!("watchdog-sample-{now}.txt"));
    let _sample =
        std::process::Command::new("sample").args([&pid, "2", "-file", &sample_path.to_string_lossy()]).output();
    let note = format!("  (macOS: full backtrace written to {})", sample_path.display());

    let listing = std::process::Command::new("ps")
        .args(["-M", "-p", &pid])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map_or_else(|| "  (ps -M unavailable)".to_owned(), |s| s.trim_end().to_owned());

    format!("{note}\n{listing}")
}
