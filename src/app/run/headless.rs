//! Headless mode — autonomous agent loop with no terminal rendering.
//!
//! Entry: `tui --headless "<instruction>"`. Boots the full app, injects the
//! instruction as the first user message, then drives [`App::background_tick`]
//! (the same orchestration the interactive loop uses) until the task reaches
//! quiescence or a guard rail fires. Writes a JSONL trajectory for Harbor
//! artifact collection.
//!
//! Design: `benchmarks/terminal-bench/HEADLESS_DESIGN.md` (decisions D1–D9).

use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::time::{Duration, Instant};

use cp_base::config::llm_types::{AnthropicModel, ClaudeCodeV2Model, LlmProvider};
use cp_base::state::context::estimate_tokens;
use cp_mod_spine::types::{NotificationType, SpineState};

use crate::app::App;
use crate::state::persistence::save_state;
use crate::state::{Kind, Message};

use super::lifecycle::{EventChannels, LAST_PROGRESS_MS, LOOP_HEARTBEAT_MS, TickStatus};

/// Settle window: the run must stay quiescent this long before we declare the
/// task done — lets async watchers / callbacks / coucou timers fire first.
const SETTLE_WINDOW: Duration = Duration::from_millis(2500);

/// Poll interval between background ticks (matches the interactive streaming poll).
const TICK_SLEEP: Duration = Duration::from_millis(8);

/// Deadman: max time the main loop may go without completing a `background_tick`
/// iteration before the watchdog declares it **wedged** and force-exits. A healthy
/// loop ticks every ~8ms ([`TICK_SLEEP`]); 30s of silence means the loop is stuck
/// inside a single iteration (a synchronous block the on-loop guards can't break).
/// This is the case all four streaming-layer watchdogs (idle / send-header /
/// first-content / catch-all) silently fail to recover: each works by injecting a
/// `StreamEvent::Error` that must be *drained* by `process_stream_events` — which
/// runs inside the very `background_tick` that is wedged. A process-level deadman
/// on a dedicated thread is the only guard immune to a wedged main loop.
const DEADMAN_TICK_STALL_MS: u64 = 30_000;

/// Deadman: max time with no *stream activity* (no received `StreamEvent`, no
/// finalized message, no retry launch) before the watchdog force-exits, even if
/// the loop keeps ticking. Catches a stream that produces zero events while the
/// loop drains an empty channel forever. v0.2.10: [`LAST_PROGRESS_MS`] is bumped
/// on any stream event / retry — so this no longer preempts v0.2.9's in-process
/// retries (which dribble then retry for up to 4×90s without finalizing a
/// message); it now fires only on TRUE silence. Set generously (3 min) so a
/// legitimately long single step never trips it, but well under the harness wall
/// (~900s) so a true hang fails fast with a diagnostic instead of burning budget.
const DEADMAN_NO_PROGRESS_MS: u64 = 180_000;

/// Deadman: poll cadence of the watchdog thread.
const DEADMAN_POLL: Duration = Duration::from_secs(5);

/// Deadman: max number of in-place re-execs before the watchdog hard-terminates.
/// Each re-exec is a fresh task attempt on a new connection (resumes from
/// persisted state); bounding them stops a deterministically-recurring wedge from
/// looping forever. Tracked across re-execs via `CP_DEADMAN_REEXEC_COUNT`.
const MAX_DEADMAN_REEXEC: u32 = 2;

/// Autonomous task-solving guidance prepended to headless instructions.
/// Encourages systematic planning via todos and proactive use of callbacks.
const HEADLESS_GUIDANCE: &str = "\
🤖 **Autonomous Mode Instructions**

To maximize your success on this task:

1. **Create a roadmap**: Use `todo_create` to break down the task into clear steps
2. **Track progress**: Mark todos done as you complete them - this helps the system know you're making progress
3. **Enable callbacks**: Use `Callback_upsert` to auto-run relevant checks (e.g., tests, linters) on file changes
4. **Think before acting**: Use the `Think` tool to plan your approach and reason through complex problems
5. **Work systematically**: Complete todos in order, don't jump around randomly

Available callbacks you can enable:
- `rust-check`: Auto-runs cargo check + clippy on Rust file edits (already available)
- Create custom callbacks for your specific task (e.g., run tests, validate output)

Begin by analyzing the task below, creating a todo list, then execute step by step.

---

**Your Task:**

";

/// Default per-task guard rails (see D3). CLI-overridable.
const DEFAULT_MAX_COST_USD: f64 = 5.0;
/// Default max conversation messages before the guard rail blocks.
const DEFAULT_MAX_MESSAGES: usize = 150;

/// Parsed `--headless` invocation options.
pub(crate) struct HeadlessOpts {
    /// The task instruction, submitted verbatim as the first user message.
    pub instruction: String,
    /// Where the JSONL trajectory is written.
    pub trajectory_path: String,
    /// LLM provider backend (default `ClaudeCodeApiKey` — env-only auth, no
    /// Keychain, container-friendly).
    pub provider: LlmProvider,
    /// API model string (`None` = provider's container default). Applied to the
    /// `AnthropicModel` family for the Anthropic-compatible providers.
    pub model: Option<String>,
    /// Guard rail: max session cost in USD (`None` = disabled).
    pub max_cost: Option<f64>,
    /// Guard rail: max conversation messages (`None` = disabled).
    pub max_messages: Option<usize>,
    /// Guard rail: max autonomous duration in seconds (`None` = defer to harness).
    pub max_duration_secs: Option<u64>,
}

/// Parse a provider name (CLI string) into [`LlmProvider`]. Accepts both the
/// `snake_case` and squashed serde spellings. Unknown values fall back to the
/// container-friendly `ClaudeCodeApiKey`.
fn parse_provider(s: &str) -> LlmProvider {
    match s {
        "anthropic" => LlmProvider::Anthropic,
        "claude_code" | "claudecode" => LlmProvider::ClaudeCode,
        "grok" => LlmProvider::Grok,
        "groq" => LlmProvider::Groq,
        "deepseek" => LlmProvider::DeepSeek,
        "minimax" => LlmProvider::MiniMax,
        "claude_code_v2" | "claudecodev2" => LlmProvider::ClaudeCodeV2,
        // "claude_code_api_key" / "claudecodeapikey" / anything else.
        _ => LlmProvider::ClaudeCodeApiKey,
    }
}

/// Final outcome of a headless run, mapped to a process exit code by `main`.
pub(crate) enum HeadlessOutcome {
    /// Task reached quiescence — exit 0.
    Done,
    /// A guard rail blocked auto-continuation — exit 2.
    GuardRail,
    /// `system_reload` requested — caller re-execs (preserving `--headless`).
    Reload,
    /// Fatal condition (ownership lost, boot failure) — exit 1.
    Error,
}

/// Parse CLI args into [`HeadlessOpts`]. Returns `None` if `--headless` is absent.
///
/// Flags: `--headless <instruction>` (or `--instruction-file <path>` for large
/// inputs), `--provider <name>` (default `claude_code_api_key`), `--model
/// <api-name>` (default container Sonnet), `--trajectory <path>`, `--max-cost
/// <usd>`, `--max-messages <n>`, `--max-duration-secs <n>`. The instruction may
/// also be the bare positional arg following `--headless`.
pub(crate) fn parse_args(args: &[String]) -> Option<HeadlessOpts> {
    let pos = args.iter().position(|a| a == "--headless")?;

    let mut instruction: Option<String> = None;
    let mut instruction_file: Option<String> = None;
    let mut trajectory_path = String::from(".context-pilot/trajectory.jsonl");
    // Default to the env-only API-key provider so containers never touch the
    // macOS Keychain or an OAuth credential file.
    let mut provider = LlmProvider::ClaudeCodeApiKey;
    let mut model: Option<String> = None;
    let mut max_cost = Some(DEFAULT_MAX_COST_USD);
    let mut max_messages = Some(DEFAULT_MAX_MESSAGES);
    let mut max_duration_secs: Option<u64> = None;

    // The instruction may be the token right after --headless (if not a flag).
    if let Some(next) = args.get(pos.saturating_add(1))
        && !next.starts_with("--")
    {
        instruction = Some(next.clone());
    }

    let mut i = 0;
    while i < args.len() {
        let Some(arg) = args.get(i) else { break };
        match arg.as_str() {
            "--instruction-file" => {
                instruction_file = args.get(i.saturating_add(1)).cloned();
                i = i.saturating_add(1);
            }
            "--provider" => {
                if let Some(v) = args.get(i.saturating_add(1)) {
                    provider = parse_provider(v);
                }
                i = i.saturating_add(1);
            }
            "--model" => {
                model = args.get(i.saturating_add(1)).cloned();
                i = i.saturating_add(1);
            }
            "--trajectory" => {
                if let Some(v) = args.get(i.saturating_add(1)) {
                    trajectory_path.clone_from(v);
                }
                i = i.saturating_add(1);
            }
            "--max-cost" => {
                max_cost = args.get(i.saturating_add(1)).and_then(|v| v.parse::<f64>().ok());
                i = i.saturating_add(1);
            }
            "--max-messages" => {
                max_messages = args.get(i.saturating_add(1)).and_then(|v| v.parse::<usize>().ok());
                i = i.saturating_add(1);
            }
            "--max-duration-secs" => {
                max_duration_secs = args.get(i.saturating_add(1)).and_then(|v| v.parse::<u64>().ok());
                i = i.saturating_add(1);
            }
            _ => {}
        }
        i = i.saturating_add(1);
    }

    // instruction-file wins if provided (handles huge instructions safely).
    if let Some(path) = instruction_file
        && let Ok(contents) = std::fs::read_to_string(&path)
    {
        instruction = Some(contents);
    }

    Some(HeadlessOpts {
        instruction: instruction.unwrap_or_default(),
        trajectory_path,
        provider,
        model,
        max_cost,
        max_messages,
        max_duration_secs,
    })
}

/// Append-only JSONL trajectory writer. Flushed after every event so artifacts
/// survive a crash or a hard kill by the harness.
struct TrajectoryWriter {
    /// Open file handle (append mode). `None` if the path could not be opened.
    file: Option<File>,
    /// UIDs of messages already emitted, to avoid duplicate `assistant` events.
    emitted: std::collections::HashSet<String>,
}

impl TrajectoryWriter {
    /// Open (or create) the trajectory file, truncating any prior run's content.
    fn new(path: &str) -> Self {
        if let Some(parent) = std::path::Path::new(path).parent() {
            let _r = std::fs::create_dir_all(parent);
        }
        let file = OpenOptions::new().create(true).write(true).truncate(true).open(path).ok();
        Self { file, emitted: std::collections::HashSet::new() }
    }

    /// Write one JSON value as a line, flushing immediately.
    fn emit(&mut self, value: &serde_json::Value) {
        if let Some(f) = self.file.as_mut() {
            let _w = writeln!(f, "{value}");
            let _f = f.flush();
        }
        // Mirror a condensed line to stdout for docker/Harbor log capture.
        let kind = value.get("event").and_then(serde_json::Value::as_str).unwrap_or("?");
        let mut out = std::io::stdout();
        let _w = writeln!(out, "[cp-headless] {kind}");
        let _f = out.flush();
    }

    /// Emit the `start` event.
    fn start(&mut self, instruction: &str, model: &str) {
        self.emit(&serde_json::json!({
            "ts": now_unix_ms(),
            "event": "start",
            "instruction": instruction,
            "model": model,
        }));
    }

    /// Emit `assistant` events for any newly-completed assistant messages.
    fn sync(&mut self, app: &App) {
        for msg in &app.state.messages {
            if msg.role != "assistant" {
                continue;
            }
            let uid = msg.uid.clone().unwrap_or_else(|| msg.id.clone());
            if self.emitted.contains(&uid) {
                continue;
            }
            // Skip the in-flight (streaming) assistant message — wait until it settles.
            if app.state.flags.stream.phase.is_streaming()
                && app.state.messages.last().map(|m| m.uid.as_ref().unwrap_or(&m.id)) == Some(&uid)
            {
                continue;
            }
            let _existed = self.emitted.insert(uid.clone());
            let tool_calls: Vec<serde_json::Value> = msg
                .tool_uses
                .iter()
                .map(|t| {
                    let intent = t.input.get("intent").and_then(serde_json::Value::as_str).unwrap_or("");
                    serde_json::json!({ "name": t.name, "intent": intent })
                })
                .collect();
            self.emit(&serde_json::json!({
                "ts": now_unix_ms(),
                "event": "assistant",
                "id": uid,
                "text": msg.content,
                "tool_calls": tool_calls,
            }));
        }
    }

    /// Emit the terminal `final` event with status + totals.
    fn finalize(&mut self, status: &str, app: &App, duration: Duration) {
        let total_cost = app.state.cost_hit_usd + app.state.cost_miss_usd + app.state.cost_output_usd;
        self.emit(&serde_json::json!({
            "ts": now_unix_ms(),
            "event": "final",
            "status": status,
            "messages": app.state.messages.len(),
            "output_tokens": app.state.total_output_tokens,
            "total_cost_usd": total_cost,
            "duration_secs": duration.as_secs(),
        }));
    }
}

/// Wall-clock UNIX milliseconds (for trajectory timestamps).
fn now_unix_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

#[expect(clippy::multiple_inherent_impl, reason = "App methods split across run/ submodules for readability")]
impl App {
    /// Apply the CLI-selected provider + model to state before streaming.
    /// Container default: `ClaudeCodeApiKey` + Sonnet 4.5 (plain `ANTHROPIC_API_KEY`,
    /// no Keychain/OAuth). The Anthropic-compatible providers all read
    /// `state.anthropic_model`, so we map the `--model` string onto that enum.
    fn apply_provider_model(&mut self, opts: &HeadlessOpts) {
        self.state.llm_provider = opts.provider;
        // Map the requested model onto the AnthropicModel enum (the model field
        // used by Anthropic / ClaudeCode / ClaudeCodeApiKey). Unknown or absent
        // → Sonnet 4.5, the container-friendly default that works with a plain
        // API key. Non-Anthropic providers keep their own default model.
        let is_anthropic_family = matches!(
            opts.provider,
            LlmProvider::Anthropic | LlmProvider::ClaudeCode | LlmProvider::ClaudeCodeApiKey
        );
        if is_anthropic_family {
            self.state.anthropic_model = match opts.model.as_deref() {
                Some("claude-opus-4-6" | "claude-opus-4-5" | "opus") => AnthropicModel::ClaudeOpus45,
                Some("claude-haiku-4-5" | "claude-haiku-4-5-20251001" | "haiku") => AnthropicModel::ClaudeHaiku45,
                // "claude-sonnet-4-5" / "...-20250929" / "sonnet" / None → Sonnet 4.5.
                _ => AnthropicModel::ClaudeSonnet45,
            };
        } else if matches!(opts.provider, LlmProvider::ClaudeCodeV2) {
            // V2 (OAuth) is the only family exposing Sonnet 4.6 — map the
            // `--model` string onto state.claude_code_v2_model. Unknown/absent →
            // Sonnet 4.6 (the leaderboard-submission default).
            self.state.claude_code_v2_model = match opts.model.as_deref() {
                Some("claude-opus-4-8" | "opus") => ClaudeCodeV2Model::ClaudeOpus48,
                Some("claude-fable-5" | "fable") => ClaudeCodeV2Model::ClaudeFable5,
                // "claude-sonnet-4-6" / "sonnet" / None → Sonnet 4.6.
                _ => ClaudeCodeV2Model::ClaudeSonnet46,
            };
        }
    }

    /// Configure the spine for autonomous headless operation: persist-until-done
    /// with guard rails as the safety harness (D2/D3).
    fn configure_headless_spine(&mut self, opts: &HeadlessOpts) {
        let cfg = &mut SpineState::get_mut(&mut self.state).config;
        cfg.continue_until_todos_done = true;
        cfg.max_cost = opts.max_cost;
        cfg.max_messages = opts.max_messages;
        cfg.max_duration_secs = opts.max_duration_secs;
        cfg.user_stopped = false;
        cfg.autonomous_start_ms = Some(crate::app::panels::now_ms());
    }

    /// Inject the task instruction as the first user message and create the
    /// `UserMessage` spine notification — the same trigger the interactive
    /// `InputSubmit` handler uses, so `check_spine` launches the stream.
    fn inject_instruction(&mut self, instruction: &str) {
        // Prepend autonomous guidance to encourage systematic task completion
        let enhanced_instruction = format!("{HEADLESS_GUIDANCE}{instruction}");
        
        let tokens = estimate_tokens(&enhanced_instruction);
        let user_id = format!("U{}", self.state.next_user_id);
        let msg_uid = format!("UID_{}_U", self.state.global_next_uid);
        self.state.next_user_id = self.state.next_user_id.saturating_add(1);
        self.state.global_next_uid = self.state.global_next_uid.saturating_add(1);

        let preview: String = enhanced_instruction.chars().take(80).collect();
        let msg = Message::new_user(user_id.clone(), msg_uid, enhanced_instruction, tokens);
        crate::state::persistence::save_message(&msg);

        if let Some(ctx) = self.state.context.iter_mut().find(|c| c.context_type.as_str() == Kind::CONVERSATION) {
            ctx.token_count = ctx.token_count.saturating_add(tokens);
        }
        self.state.messages.push(msg);

        let _r = SpineState::create_notification(
            &mut self.state,
            NotificationType::UserMessage,
            user_id,
            preview,
        );

        for module in crate::modules::all_modules() {
            module.on_user_message(&mut self.state);
        }
    }

    /// True when nothing is in flight: stream idle, typewriter drained, no
    /// pending tools / deferred sleep / reverie, and no unprocessed spine
    /// notifications. The settle window in [`run_headless`] guards against
    /// premature exit from a transient lull.
    fn is_quiescent(&self) -> bool {
        !self.state.flags.stream.phase.is_streaming()
            && self.typewriter.pending_chars.is_empty()
            && self.pending_done.is_none()
            && self.pending_tools.is_empty()
            && self.reverie_streams.is_empty()
            && !self.deferred_tool_sleeping
            && !SpineState::has_unprocessed_notifications(&self.state)
    }

    /// The headless run loop. Drives `background_tick` until quiescence (Done),
    /// a guard rail (`GuardRail`), a reload request (`Reload`), or a fatal
    /// condition (Error). Writes the trajectory throughout.
    pub(crate) fn run_headless(&mut self, ch: &EventChannels<'_>, opts: &HeadlessOpts) -> HeadlessOutcome {
        // Set provider + model first so the trajectory records the real model.
        self.apply_provider_model(opts);
        let model = {
            use cp_base::state::data::model_helpers::ModelPricing as _;
            self.state.current_model()
        };
        let mut traj = TrajectoryWriter::new(&opts.trajectory_path);
        traj.start(&opts.instruction, &model);

        self.configure_headless_spine(opts);
        self.inject_instruction(&opts.instruction);

        // Arm the deadman heartbeats and spawn the watchdog thread. The watchdog
        // is independent of this loop — it force-exits if the loop wedges or the
        // task stops progressing, the only guard immune to a wedged main loop.
        {
            use std::sync::atomic::Ordering;
            let armed = now_ms_u64();
            LOOP_HEARTBEAT_MS.store(armed, Ordering::Relaxed);
            LAST_PROGRESS_MS.store(armed, Ordering::Relaxed);
        }
        spawn_deadman(opts.trajectory_path.clone());

        let start = Instant::now();
        let mut idle_since: Option<Instant> = None;
        let mut prev_msg_count = self.state.messages.len();

        loop {
            match self.background_tick(ch) {
                TickStatus::Continue => {}
                TickStatus::Reload => {
                    traj.finalize("reload", self, start.elapsed());
                    return HeadlessOutcome::Reload;
                }
                TickStatus::OwnershipLost => {
                    traj.finalize("ownership_lost", self, start.elapsed());
                    return HeadlessOutcome::Error;
                }
            }

            // Bump the progress heartbeat when a message is finalized. This is one
            // of several progress signals — v0.2.10 also bumps LAST_PROGRESS_MS on
            // any received StreamEvent / retry launch (in run/streaming.rs), so the
            // deadman tolerates v0.2.9's dribble-then-retry cycling and trips only on
            // true silence. (LOOP_HEARTBEAT_MS is bumped at the top of background_tick.)
            let msg_count = self.state.messages.len();
            if msg_count != prev_msg_count {
                prev_msg_count = msg_count;
                LAST_PROGRESS_MS.store(now_ms_u64(), std::sync::atomic::Ordering::Relaxed);
            }

            traj.sync(self);

            if let Some(reason) = self.state.guard_rail_blocked.clone() {
                traj.emit(&serde_json::json!({
                    "ts": now_unix_ms(), "event": "guard_rail", "reason": reason,
                }));
                traj.finalize("guard_rail", self, start.elapsed());
                save_state(&self.state);
                return HeadlessOutcome::GuardRail;
            }

            if self.is_quiescent() {
                match idle_since {
                    None => idle_since = Some(Instant::now()),
                    Some(t) if t.elapsed() >= SETTLE_WINDOW => {
                        traj.sync(self);
                        traj.finalize("done", self, start.elapsed());
                        save_state(&self.state);
                        return HeadlessOutcome::Done;
                    }
                    Some(_) => {}
                }
            } else {
                idle_since = None;
            }

            std::thread::sleep(TICK_SLEEP);
        }
    }
}

/// Disable interactive-only tools that cannot work without a terminal (D4).
/// Only `ask_user_question` (blocks forever on the question form) is removed;
/// `system_reload` is kept — the self-exec re-execs with `--headless` intact.
pub(crate) fn disable_interactive_tools(state: &mut crate::state::State) {
    for tool in &mut state.tools {
        if tool.id == "ask_user_question" {
            tool.enabled = false;
        }
    }
}

/// Spawn the process-level **deadman watchdog** on a dedicated thread (D-safety).
///
/// This thread is wholly independent of the main loop: it only sleeps, reads two
/// atomic heartbeats, and — on a stall — writes a forensic diagnostic and
/// force-exits the process. It is the backstop for two failure modes the on-loop
/// streaming guards provably cannot recover (observed across v0.2.4–v0.2.7, four
/// identical gpt2 freezes):
///
/// 1. **Wedged loop** — `background_tick` stops completing iterations
///    ([`LOOP_HEARTBEAT_MS`] stops advancing). Every streaming watchdog injects a
///    `StreamEvent::Error` that must be *drained* by `process_stream_events`, which
///    runs *inside* the wedged tick — so the retry never happens. Trips after
///    [`DEADMAN_TICK_STALL_MS`].
/// 2. **Hung stream, live loop** — the loop keeps ticking but no message is ever
///    finalized ([`LAST_PROGRESS_MS`] stops advancing). Trips after
///    [`DEADMAN_NO_PROGRESS_MS`].
///
/// On trip it appends a `deadman` + `final` event to the trajectory (so Harbor
/// records a clean terminal status instead of a 900s wall timeout) plus a
/// `/proc/self` thread-state dump to `.context-pilot/errors/deadman.log` (so the
/// exact wedge point is captured for the root-cause fix), then `exit(2)`.
fn spawn_deadman(trajectory_path: String) {
    use std::sync::atomic::Ordering;

    let _handle = std::thread::spawn(move || -> ! {
        loop {
            std::thread::sleep(DEADMAN_POLL);
            let now = now_ms_u64();
            let last_tick = LOOP_HEARTBEAT_MS.load(Ordering::Relaxed);
            let last_prog = LAST_PROGRESS_MS.load(Ordering::Relaxed);

            // Only evaluate once the heartbeats have been armed (non-zero).
            let tick_stalled = last_tick != 0 && now.saturating_sub(last_tick) > DEADMAN_TICK_STALL_MS;
            let no_progress = last_prog != 0 && now.saturating_sub(last_prog) > DEADMAN_NO_PROGRESS_MS;

            if tick_stalled || no_progress {
                let reason = if tick_stalled {
                    format!(
                        "main loop WEDGED: no background_tick iteration for {}ms (limit {}ms)",
                        now.saturating_sub(last_tick),
                        DEADMAN_TICK_STALL_MS
                    )
                } else {
                    format!(
                        "no task progress for {}ms (limit {}ms) — stream hung / not recovered",
                        now.saturating_sub(last_prog),
                        DEADMAN_NO_PROGRESS_MS
                    )
                };
                dump_deadman_diagnostic(&reason, &trajectory_path);
                terminate_wedged_process();
            }
        }
    });
}

/// Escape a wedged/hung headless process from the deadman thread.
///
/// `std::process::exit` is forbidden project-wide (and cannot run cleanly from a
/// side thread while the main loop is wedged anyway). Instead we **re-exec** the
/// binary in place via `exec()` — this replaces the entire process image (tearing
/// down every wedged thread at the OS level) with a fresh `--headless` run that
/// resumes from persisted state. That doubles as the recovery path: a fresh
/// process retries the task with the remaining harness budget on a brand-new
/// connection, which often clears a connection-specific hang.
///
/// Re-execs are bounded by `CP_DEADMAN_REEXEC_COUNT` (max [`MAX_DEADMAN_REEXEC`])
/// so a deterministically-recurring wedge cannot loop forever. Once the budget is
/// spent — or if `exec()` fails — we `abort()` (SIGABRT), the one whole-process
/// terminator not on the project's disallowed list, leaving the already-written
/// `final`/`deadman_timeout` trajectory event for the harness to score.
fn terminate_wedged_process() -> ! {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;

        let count: u32 =
            std::env::var("CP_DEADMAN_REEXEC_COUNT").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
        if count < MAX_DEADMAN_REEXEC
            && std::env::var_os("CP_RUN_SH").is_none()
            && let Ok(exe) = std::env::current_exe()
        {
            let mut exec_args: Vec<String> = std::env::args().skip(1).collect();
            if !exec_args.iter().any(|a| a == "--resume-stream") {
                exec_args.push("--resume-stream".to_string());
            }
            let mut out = std::io::stdout();
            let _w = {
                use std::io::Write as _;
                writeln!(out, "[cp-headless] deadman re-exec (attempt {})", count.saturating_add(1))
            };
            // exec replaces this image and never returns on success.
            let _err = std::process::Command::new(exe)
                .args(&exec_args)
                .env("CP_DEADMAN_REEXEC_COUNT", (count.saturating_add(1)).to_string())
                .exec();
        }
    }
    // Re-exec budget spent, exec failed, or non-unix: hard-terminate. SIGABRT is
    // the sole whole-process terminator not banned by the lint config.
    std::process::abort();
}

/// Write the deadman forensic dump and a terminal trajectory event before exit.
///
/// Captures every thread's `state`/`wchan`/`syscall` from `/proc/self/task/*` so
/// the next run pinpoints exactly where the loop wedges. Best-effort: any read or
/// write failure is ignored (the process is exiting regardless).
fn dump_deadman_diagnostic(reason: &str, trajectory_path: &str) {
    use std::fmt::Write as _;
    use std::io::Write as _;

    // 1. Append a terminal event to the trajectory so Harbor sees a clean finish.
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(trajectory_path) {
        let _w = writeln!(
            f,
            "{}",
            serde_json::json!({
                "ts": now_unix_ms(),
                "event": "final",
                "status": "deadman_timeout",
                "reason": reason,
            })
        );
        let _f = f.flush();
    }

    // 2. Forensic /proc dump to errors/deadman.log.
    let dir = std::path::Path::new(".context-pilot").join("errors");
    let _r = std::fs::create_dir_all(&dir);
    let pid = std::process::id();
    let read = |p: String| std::fs::read_to_string(&p).unwrap_or_default();

    let mut threads = String::new();
    if let Ok(entries) = std::fs::read_dir(format!("/proc/{pid}/task")) {
        for entry in entries.flatten() {
            let tid = entry.file_name().to_string_lossy().into_owned();
            let stat = read(format!("/proc/{pid}/task/{tid}/stat"));
            let state = stat.split_whitespace().nth(2).unwrap_or("?").to_string();
            let wchan = read(format!("/proc/{pid}/task/{tid}/wchan"));
            let syscall = read(format!("/proc/{pid}/task/{tid}/syscall"));
            let _w = writeln!(threads, "  tid {tid} state={state} wchan={} syscall={}", wchan.trim(), syscall.trim());
        }
    }
    let main_stack = read(format!("/proc/{pid}/task/{pid}/stack"));

    let ts = now_unix_ms();
    let entry = format!(
        "[{ts}] DEADMAN TRIP\nReason: {reason}\npid: {pid}\nThreads:\n{threads}\nMain kernel stack:\n{main_stack}\n---\n"
    );
    let _rw = OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("deadman.log"))
        .and_then(|mut f| f.write_all(entry.as_bytes()));

    // Mirror to stdout for docker/Harbor log capture.
    let mut out = std::io::stdout();
    let _w = writeln!(out, "[cp-headless] DEADMAN TRIP: {reason}");
    let _f = out.flush();
}

/// Wall-clock UNIX milliseconds as `u64` (for the deadman heartbeat comparison).
fn now_ms_u64() -> u64 {
    crate::app::panels::now_ms()
}
