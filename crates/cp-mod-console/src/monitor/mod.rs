//! `Monitor` tool: asynchronous, labelled notifications from a console's output.
//!
//! A monitor attaches to an existing console (`type=console`), a command it
//! spawns into a new console (`type=command`), or a websocket relayed by a
//! `websocat` console (`type=ws`). It fires on exit, on matching lines, or on
//! every line, once or ongoing. See `spec` for the rules, `engine` for
//! batching / flood / spill, `watcher` for the lifecycle.

/// Pure line, batch, flood and spill mechanics.
pub mod engine;
/// Fixed Monitor panel.
pub(crate) mod panel;
/// Parameters, validation and the panel record.
pub mod spec;
/// Unit tests.
#[cfg(test)]
mod tests;
/// The persistent spine watcher.
pub mod watcher;

use std::sync::{Arc, Mutex};

use cp_base::panels::now_ms;
use cp_base::state::runtime::State;
use cp_base::state::watchers::WatcherRegistry;
use cp_base::tools::{ToolResult, ToolUse};

use self::spec::{Mode, Record, Request, Source, Status};
use self::watcher::LiveMonitor;
use crate::tools;
use crate::types::ConsoleState;

/// Console a monitor reads, and where it starts reading.
struct Target {
    /// Console panel id.
    panel_id: String,
    /// Console session key.
    session: String,
    /// Ring-buffer offset to start from: current end for existing consoles
    /// (only new output counts), 0 for freshly spawned ones.
    cursor: u64,
}

/// Handle `Monitor`.
pub fn execute(tool: &ToolUse, state: &mut State) -> ToolResult {
    let _fg = cp_base::flame!("monitor");
    match run(tool, state) {
        Ok(msg) => ToolResult::new(tool.id.clone(), msg, false),
        Err(e) => ToolResult::new(tool.id.clone(), e, true),
    }
}

/// Validate, attach, then register the watcher and its panel record.
fn run(tool: &ToolUse, state: &mut State) -> Result<String, String> {
    let req = spec::parse(&tool.input)?;
    let (target, created) = attach(tool, state, &req)?;
    if req.mode == Mode::Exit
        && let Some(done) = already_exited(state, &target)
    {
        return Ok(done);
    }
    let agent = state.executing_agent_id().map(str::to_owned);
    let cs = ConsoleState::get_mut(state);
    let monitor_id = cs.alloc_monitor_id();
    let summary = format!(
        "{created}Monitor {monitor_id} registered on {} (type={}, mode={}, frequency={}). Notifications are labelled \
         '[Monitor {monitor_id} \u{b7} {}]'. Close {} to stop it.",
        target.panel_id,
        spec::type_name(req.source),
        mode_name(req.mode),
        if req.frequency == spec::Frequency::Once { "once" } else { "ongoing" },
        req.description,
        target.panel_id,
    );
    let record = Arc::new(Mutex::new(Record {
        monitor_id,
        params: req,
        panel_id: target.panel_id,
        session: target.session,
        started_at_ms: now_ms(),
        events: 0,
        status: Status::Alive,
    }));
    cs.monitors.push(Arc::clone(&record));
    cs.prune_monitors();
    WatcherRegistry::get_mut(state).register(Box::new(LiveMonitor::new(record, target.cursor, agent)));
    Ok(summary)
}

/// Lowercase mode name, as written in the tool call.
const fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Exit => "exit",
        Mode::Pattern => "pattern",
        Mode::Line => "line",
    }
}

/// Resolve or spawn the console. The string is a prefix for the tool result
/// ("Console created in Pn. ") when a console was spawned.
fn attach(tool: &ToolUse, state: &mut State, req: &Request) -> Result<(Target, String), String> {
    match req.source {
        Source::Console => {
            let panel_id = req.id.clone().unwrap_or_default();
            let session = tools::resolve_session_key(state, &panel_id)?;
            let cursor = ConsoleState::get(state)
                .sessions
                .get(&session)
                .map(|h| h.buffer.total_written())
                .ok_or_else(|| format!("Session for '{panel_id}' not found"))?;
            Ok((Target { panel_id, session, cursor }, String::new()))
        }
        Source::Command => {
            let command = req.command.clone().unwrap_or_default();
            if let Some(msg) = tools::check_git_gh_guardrail(&command) {
                return Err(msg);
            }
            spawn(tool, state, &command, &req.description)
        }
        Source::Ws => {
            let command = ws_command(req)?;
            spawn(tool, state, &command, &req.description)
        }
    }
}

/// Spawn `command` into a new console panel titled `description`.
fn spawn(tool: &ToolUse, state: &mut State, command: &str, description: &str) -> Result<(Target, String), String> {
    let cwd = tools::resolve_cwd(tool, state)?;
    let (panel_id, session) = tools::spawn_console(state, command, Some(cwd), Some(description));
    let created = format!("Console created in {panel_id}. ");
    Ok((Target { panel_id, session, cursor: 0 }, created))
}

/// `mode=exit` on a console whose process already ended: answer now, register nothing.
fn already_exited(state: &State, target: &Target) -> Option<String> {
    let handle = ConsoleState::get(state).sessions.get(&target.session)?;
    let status = handle.get_status();
    status.is_terminal().then(|| {
        format!(
            "Process in {} already exited ({}); no monitor registered. Last output:\n{}",
            target.panel_id,
            status.label(),
            handle.buffer.last_n_lines(5)
        )
    })
}

/// Single-quote `s` for `sh`.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Whether `bin` is an executable file on `PATH`.
fn on_path(bin: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(bin).is_file()))
}

/// Shell command relaying a websocket to stdout, one message per line.
///
/// Without `ws_send` the relay is unidirectional (`-U`), so a closed stdin
/// does not drop the connection. With it, the frame is written once and
/// stdin is then held open by `tail -f /dev/null`.
fn ws_command(req: &Request) -> Result<String, String> {
    let url = req.ws_url.as_deref().unwrap_or_default();
    if !(url.starts_with("ws://") || url.starts_with("wss://")) {
        return Err(format!("'ws_url' must start with ws:// or wss:// (got '{url}')"));
    }
    if !on_path("websocat") {
        return Err("type='ws' needs `websocat` on PATH (e.g. `brew install websocat`)".to_owned());
    }
    let url_q = sh_quote(url);
    Ok(req.ws_send.as_deref().map_or_else(
        || format!("websocat -t -U {url_q}"),
        |frame| format!("{{ printf '%s\\n' {}; tail -f /dev/null; }} | websocat -t {url_q}", sh_quote(frame)),
    ))
}
