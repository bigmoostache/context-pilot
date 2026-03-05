use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub(crate) struct Request {
    pub cmd: String,
    pub key: Option<String>,
    pub command: Option<String>,
    pub cwd: Option<String>,
    pub input: Option<String>,
    pub log_path: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct Response {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sessions: Option<Vec<SessionInfo>>,
}

#[derive(Serialize)]
pub(crate) struct SessionInfo {
    pub key: String,
    pub pid: u32,
    pub status: String,
    pub exit_code: Option<i32>,
}

impl Response {
    pub(crate) const fn ok() -> Self {
        Self { ok: true, error: None, pid: None, status: None, exit_code: None, sessions: None }
    }
    pub(crate) const fn ok_pid(pid: u32) -> Self {
        Self { ok: true, error: None, pid: Some(pid), status: None, exit_code: None, sessions: None }
    }
    pub(crate) const fn ok_status(status: String, exit_code: Option<i32>) -> Self {
        Self { ok: true, error: None, pid: None, status: Some(status), exit_code, sessions: None }
    }
    pub(crate) const fn ok_sessions(sessions: Vec<SessionInfo>) -> Self {
        Self { ok: true, error: None, pid: None, status: None, exit_code: None, sessions: Some(sessions) }
    }
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
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            match bytes[i + 1] {
                b'n' => {
                    out.push(0x0A);
                    i += 2;
                }
                b'r' => {
                    out.push(0x0D);
                    i += 2;
                }
                b't' => {
                    out.push(0x09);
                    i += 2;
                }
                b'\\' => {
                    out.push(b'\\');
                    i += 2;
                }
                b'e' => {
                    out.push(0x1B);
                    i += 2;
                }
                b'0' => {
                    out.push(0x00);
                    i += 2;
                }
                b'x' if i + 3 < bytes.len() => {
                    let hi = bytes[i + 2];
                    let lo = bytes[i + 3];
                    if let (Some(h), Some(l)) = (hex_digit(hi), hex_digit(lo)) {
                        out.push((h << 4) | l);
                        i += 4;
                    } else {
                        out.push(b'\\');
                        i += 1;
                    }
                }
                _ => {
                    out.push(b'\\');
                    i += 1;
                }
            }
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

const fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
