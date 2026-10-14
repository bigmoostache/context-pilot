//! Monitor parameters: parsing, validation, and the shared record shown in the Monitor panel.

use serde::Serialize;
use serde_json::Value;

/// What a monitor is attached to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// An existing console panel (`id`).
    Console,
    /// A command spawned into a new console panel (`command`).
    Command,
    /// A websocket stream relayed through a `websocat` console (`ws_url`).
    Ws,
}

/// What makes a monitor fire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// The process exits.
    Exit,
    /// A new output line matches `pattern`.
    Pattern,
    /// Any new output line.
    Line,
}

/// Whether a monitor survives its first event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Frequency {
    /// Ends after the first event.
    Once,
    /// Keeps firing until its console dies or it floods.
    Ongoing,
}

/// Validated `Monitor` call, before any console is resolved or spawned.
#[derive(Debug, Clone, Serialize)]
pub struct Request {
    /// Attachment kind.
    #[serde(rename = "type")]
    pub source: Source,
    /// Short label repeated in every notification.
    pub description: String,
    /// Firing condition.
    pub mode: Mode,
    /// Once or ongoing.
    pub frequency: Frequency,
    /// Regex (`mode=pattern` only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    /// Console panel id (`type=console` only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Shell command (`type=command` only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Working directory (`type=command`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Websocket URL (`type=ws` only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ws_url: Option<String>,
    /// Text sent once after connecting, e.g. a subscribe frame (`type=ws` only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ws_send: Option<String>,
}

/// Read an optional non-empty string parameter.
fn opt_str(input: &Value, key: &str) -> Option<String> {
    input.get(key).and_then(Value::as_str).filter(|s| !s.trim().is_empty()).map(str::to_owned)
}

/// Read a required string parameter.
fn req_str(input: &Value, key: &str) -> Result<String, String> {
    opt_str(input, key).ok_or_else(|| format!("Missing required '{key}' parameter"))
}

/// Parse an enum parameter against its accepted spellings.
fn parse_enum<T>(input: &Value, key: &str, table: &[(&str, T)]) -> Result<T, String>
where
    T: Copy,
{
    let raw = req_str(input, key)?;
    table.iter().find(|entry| entry.0 == raw).map(|entry| entry.1).ok_or_else(|| {
        let names: Vec<&str> = table.iter().map(|entry| entry.0).collect();
        format!("Invalid {key} '{raw}'. Must be one of: {}", names.join(", "))
    })
}

/// Params only one `type` accepts: (param, owning type).
const TYPE_PARAMS: [(&str, Source); 5] = [
    ("id", Source::Console),
    ("command", Source::Command),
    ("cwd", Source::Command),
    ("ws_url", Source::Ws),
    ("ws_send", Source::Ws),
];

/// Parse and cross-validate the raw tool input.
///
/// # Errors
///
/// Returns a message naming the bad parameter or combination.
pub fn parse(input: &Value) -> Result<Request, String> {
    let source =
        parse_enum(input, "type", &[("console", Source::Console), ("command", Source::Command), ("ws", Source::Ws)])?;
    let mode = parse_enum(input, "mode", &[("exit", Mode::Exit), ("pattern", Mode::Pattern), ("line", Mode::Line)])?;
    let frequency = parse_enum(input, "frequency", &[("once", Frequency::Once), ("ongoing", Frequency::Ongoing)])?;
    let description = req_str(input, "description")?;
    let req = Request {
        source,
        description,
        mode,
        frequency,
        pattern: opt_str(input, "pattern"),
        id: opt_str(input, "id"),
        command: opt_str(input, "command"),
        cwd: opt_str(input, "cwd"),
        ws_url: opt_str(input, "ws_url"),
        ws_send: opt_str(input, "ws_send"),
    };
    check_combination(&req)?;
    check_type_params(input, &req)?;
    Ok(req)
}

/// Reject incompatible mode / frequency / type combinations.
fn check_combination(req: &Request) -> Result<(), String> {
    if req.mode == Mode::Exit && req.frequency == Frequency::Ongoing {
        return Err("mode='exit' fires only once: use frequency='once'".to_owned());
    }
    if req.source == Source::Ws && req.mode != Mode::Line {
        return Err("type='ws' only supports mode='line'".to_owned());
    }
    match (req.mode, req.pattern.as_deref()) {
        (Mode::Pattern, None) => Err("mode='pattern' requires a 'pattern' parameter".to_owned()),
        (Mode::Pattern, Some(p)) => {
            regex::Regex::new(p).map(|_| ()).map_err(|e| format!("Invalid regex 'pattern': {e}"))
        }
        (Mode::Exit | Mode::Line, Some(_)) => Err("'pattern' is only valid with mode='pattern'".to_owned()),
        (Mode::Exit | Mode::Line, None) => Ok(()),
    }
}

/// Require the type's own params and reject the other types' params.
fn check_type_params(input: &Value, req: &Request) -> Result<(), String> {
    let required = match req.source {
        Source::Console => "id",
        Source::Command => "command",
        Source::Ws => "ws_url",
    };
    if opt_str(input, required).is_none() {
        return Err(format!("type='{}' requires a '{required}' parameter", type_name(req.source)));
    }
    for (param, owner) in TYPE_PARAMS {
        if owner != req.source && input.get(param).is_some() {
            return Err(format!("'{param}' is only valid with type='{}'", type_name(owner)));
        }
    }
    Ok(())
}

/// Lowercase name of a source, as written in the tool call.
#[must_use]
pub const fn type_name(source: Source) -> &'static str {
    match source {
        Source::Console => "console",
        Source::Command => "command",
        Source::Ws => "ws",
    }
}

/// Lifecycle of a monitor, as shown in the Monitor panel.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "lowercase", tag = "state")]
pub enum Status {
    /// Watching.
    Alive,
    /// Stopped; `reason` says why.
    Ended {
        /// Why it stopped (once fired, flooded, console closed, process exited…).
        reason: String,
        /// When it stopped (ms since epoch).
        at_ms: u64,
    },
}

/// One monitor as tracked by `ConsoleState` and rendered by the Monitor panel.
#[derive(Debug, Clone, Serialize)]
pub struct Record {
    /// Monitor id (`M<n>`).
    pub monitor_id: String,
    /// The validated call.
    pub params: Request,
    /// Console panel the monitor reads.
    pub panel_id: String,
    /// Console session key behind `panel_id`.
    pub session: String,
    /// Registration time (ms since epoch).
    pub started_at_ms: u64,
    /// Notifications sent so far.
    pub events: u32,
    /// Current lifecycle state.
    pub status: Status,
}

impl Record {
    /// Whether the monitor is still watching.
    #[must_use]
    pub const fn is_alive(&self) -> bool {
        matches!(self.status, Status::Alive)
    }

    /// When the monitor ended, if it has.
    #[must_use]
    pub const fn ended_at_ms(&self) -> Option<u64> {
        match self.status {
            Status::Alive => None,
            Status::Ended { at_ms, .. } => Some(at_ms),
        }
    }
}
