use serde::{Deserialize, Serialize};

/// Incoming JSON request from the TUI client.
#[derive(Deserialize)]
pub(crate) struct Request {
    /// Command name (e.g. `"create"`, `"send"`, `"kill"`, `"status"`, `"list"`).
    pub cmd: String,
    /// Session key that identifies the target child process.
    pub key: Option<String>,
    /// Shell command to execute (used by `"create"`).
    pub command: Option<String>,
    /// Working directory for the spawned process (used by `"create"`).
    pub cwd: Option<String>,
    /// Raw input string to write to stdin (used by `"send"`).
    pub input: Option<String>,
    /// Path of the log file for stdout/stderr redirection (used by `"create"`).
    pub log_path: Option<String>,
}

/// Outgoing JSON response sent back to the TUI client.
#[derive(Serialize)]
pub(crate) struct Response {
    /// Whether the request succeeded.
    pub ok: bool,
    /// Human-readable error message, present only on failure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// PID of the newly spawned process, present after a successful `"create"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    /// Human-readable status string, present after a `"status"` query.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// Exit code of the process, present when the process has terminated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// List of all active sessions, present after a `"list"` query.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sessions: Option<Vec<SessionInfo>>,
}

/// Snapshot of a single session returned in `"list"` responses.
#[derive(Serialize)]
pub(crate) struct SessionInfo {
    /// Session key assigned by the TUI.
    pub key: String,
    /// PID of the child process.
    pub pid: u32,
    /// Human-readable status string (e.g. `"running"` or `"exited(0)"`).
    pub status: String,
    /// Exit code, present when the process has terminated.
    pub exit_code: Option<i32>,
}

impl Response {
    /// Construct a plain success response with no extra payload.
    pub(crate) const fn ok() -> Self {
        Self { ok: true, error: None, pid: None, status: None, exit_code: None, sessions: None }
    }
    /// Construct a success response carrying the PID of a newly spawned process.
    pub(crate) const fn ok_pid(pid: u32) -> Self {
        Self { ok: true, error: None, pid: Some(pid), status: None, exit_code: None, sessions: None }
    }
    /// Construct a success response carrying process status and optional exit code.
    pub(crate) const fn ok_status(status: String, exit_code: Option<i32>) -> Self {
        Self { ok: true, error: None, pid: None, status: Some(status), exit_code, sessions: None }
    }
    /// Construct a success response carrying the full list of session snapshots.
    pub(crate) const fn ok_sessions(sessions: Vec<SessionInfo>) -> Self {
        Self { ok: true, error: None, pid: None, status: None, exit_code: None, sessions: Some(sessions) }
    }
    /// Construct a failure response with the given error message.
    pub(crate) fn err(msg: impl Into<String>) -> Self {
        Self { ok: false, error: Some(msg.into()), pid: None, status: None, exit_code: None, sessions: None }
    }
}

/// Interpret escape sequences in input strings.
/// Handles: \n, \r, \t, \\, \e, \0, \xHH
pub(crate) fn interpret_escapes(input: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let Some(cur) = bytes.get(i).copied() else { break };
        if cur == b'\\' {
            match i.checked_add(1).and_then(|j| bytes.get(j)).copied() {
                Some(b'n') => {
                    out.push(0x0A);
                    i = i.saturating_add(2);
                }
                Some(b'r') => {
                    out.push(0x0D);
                    i = i.saturating_add(2);
                }
                Some(b't') => {
                    out.push(0x09);
                    i = i.saturating_add(2);
                }
                Some(b'\\') => {
                    out.push(b'\\');
                    i = i.saturating_add(2);
                }
                Some(b'e') => {
                    out.push(0x1B);
                    i = i.saturating_add(2);
                }
                Some(b'0') => {
                    out.push(0x00);
                    i = i.saturating_add(2);
                }
                Some(b'x') => {
                    let hi = i.checked_add(2).and_then(|j| bytes.get(j)).copied();
                    let lo = i.checked_add(3).and_then(|j| bytes.get(j)).copied();
                    match (hi, lo) {
                        (Some(hi), Some(lo)) if i.saturating_add(3) < bytes.len() => {
                            if let (Some(h), Some(l)) = (hex_digit(hi), hex_digit(lo)) {
                                out.push((h << 4) | l);
                                i = i.saturating_add(4);
                            } else {
                                out.push(b'\\');
                                i = i.saturating_add(1);
                            }
                        }
                        _ => {
                            out.push(b'\\');
                            i = i.saturating_add(1);
                        }
                    }
                }
                _ => {
                    out.push(b'\\');
                    i = i.saturating_add(1);
                }
            }
        } else {
            out.push(cur);
            i = i.saturating_add(1);
        }
    }
    out
}

/// Convert a single ASCII hex digit byte to its numeric value.
const fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b.wrapping_sub(b'0')),
        b'a'..=b'f' => Some(b.wrapping_sub(b'a').wrapping_add(10)),
        b'A'..=b'F' => Some(b.wrapping_sub(b'A').wrapping_add(10)),
        _ => None,
    }
}
