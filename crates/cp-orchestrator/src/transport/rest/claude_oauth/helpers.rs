//! Stateless helpers for the Claude Code OAuth flow.
//!
//! Extracted from `mod.rs` to keep it under the 500-line cap. These are pure
//! free functions (`pub(super)` so the parent module's handlers can call them);
//! keeping them here — rather than the response structs — avoids the
//! `field_scoped_visibility_modifiers` / `unreachable_pub` deadlock that a
//! cross-module struct with buildable fields would hit.

use std::time::Duration;

/// Extract the authorization code from user input.
///
/// Accepts:
/// - Raw code string
/// - `code#state` format (Anthropic's callback page output)
/// - Full callback URL (`http://…/callback?code=XXXX&state=YYYY`)
pub(super) fn extract_code(input: &str) -> &str {
    // If it looks like a URL with `code=`, pull out the code value.
    if let Some(qs) = input.split('?').nth(1) {
        for pair in qs.split('&') {
            if let Some(val) = pair.strip_prefix("code=") {
                return val;
            }
        }
    }
    // Anthropic's callback page returns `code#state` — strip the state part.
    if let Some(hash_pos) = input.find('#') {
        return input.get(..hash_pos).unwrap_or(input);
    }
    input
}

/// Minimal percent-encoding for URL query parameters.
pub(super) fn urlencoded(s: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(char::from(b)),
            // `write!` on a String is infallible; `%{b:02X}` is the exact
            // percent-escape (uppercase hex), no hand-rolled nibble table.
            _ => _ = write!(out, "%{b:02X}"),
        }
    }
    out
}

/// Read random bytes from `/dev/urandom`.
pub(super) fn read_random(buf: &mut [u8]) -> Result<(), std::io::Error> {
    use std::io::Read as _;
    std::fs::File::open("/dev/urandom")?.read_exact(buf)
}

/// Fetch the account email from Anthropic's OAuth profile endpoint.
pub(super) fn fetch_account_email(token: &str) -> Option<String> {
    let client = reqwest::blocking::Client::new();
    let resp = client
        .get("https://api.anthropic.com/api/oauth/profile")
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", "claude-code/2.1.196")
        .header("anthropic-beta", "oauth-2025-04-20")
        .timeout(Duration::from_secs(5))
        .send()
        .ok()?;
    let val: serde_json::Value = resp.json().ok()?;
    val.get("account")?.get("email")?.as_str().map(str::to_owned)
}
