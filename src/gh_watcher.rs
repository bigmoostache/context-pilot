//! Background polling watcher for GithubResult panels.
//!
//! Uses HTTP ETags for `gh api` commands and output hashing for other `gh`
//! commands to efficiently detect changes. Respects the `X-Poll-Interval`
//! header from GitHub API responses to dynamically adjust per-watch polling
//! frequency. Sends `CacheUpdate::GithubResultContent` through the shared
//! `cache_tx` channel when content changes.

use std::collections::HashMap;
use std::process::Command;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use secrecy::{ExposeSecret, SecretBox};
use sha2::{Sha256, Digest};

use crate::cache::CacheUpdate;
use crate::constants::{GH_CMD_TIMEOUT_SECS, MAX_RESULT_CONTENT_BYTES};
use crate::core::panels::now_ms;
use crate::modules::{run_with_timeout, truncate_output};
use crate::state::estimate_tokens;

/// How often the background thread wakes to check if any watch is due (seconds)
const GH_WATCHER_TICK_SECS: u64 = 5;

/// Default polling interval when no X-Poll-Interval header is available (seconds).
/// GitHub's typical X-Poll-Interval is 60s; we use the same default.
const GH_DEFAULT_POLL_INTERVAL_SECS: u64 = 60;

/// Per-panel watch state
struct GhWatch {
    context_id: String,
    github_token: Arc<SecretBox<String>>,
    /// Pre-parsed args (excludes "gh" prefix)
    args: Vec<String>,
    /// true if args[0] == "api" && no --jq/--template flags
    is_api_command: bool,
    /// ETag from last 200 response (api commands only)
    etag: Option<String>,
    /// SHA-256 of last output (non-api commands)
    last_output_hash: Option<String>,
    /// Polling interval in seconds (from X-Poll-Interval header or default)
    poll_interval_secs: u64,
    /// Timestamp of last poll attempt (milliseconds)
    last_poll_ms: u64,
}

/// Background watcher that polls GithubResult panels for changes.
pub struct GhWatcher {
    watches: Arc<Mutex<HashMap<String, GhWatch>>>,
    _thread: JoinHandle<()>,
}

impl GhWatcher {
    /// Create a new GhWatcher with a background polling thread.
    pub fn new(cache_tx: Sender<CacheUpdate>) -> Self {
        let watches: Arc<Mutex<HashMap<String, GhWatch>>> = Arc::new(Mutex::new(HashMap::new()));
        let watches_clone = Arc::clone(&watches);

        let thread = thread::spawn(move || {
            poll_loop(watches_clone, cache_tx);
        });

        Self {
            watches,
            _thread: thread,
        }
    }

    /// Reconcile the watch list with current GithubResult panels.
    /// Args: `(context_id, command, github_token)`.
    /// Adds missing watches, removes stale ones, preserves etag/hash/interval state on existing.
    pub fn sync_watches(&self, panels: &[(String, String, String)]) {
        let mut watches = self.watches.lock().expect("gh_watcher watches lock poisoned");

        // Remove watches for panels that no longer exist
        let active_ids: std::collections::HashSet<&str> =
            panels.iter().map(|(id, _, _)| id.as_str()).collect();
        watches.retain(|id, _| active_ids.contains(id.as_str()));

        // Add watches for new panels
        for (context_id, command, github_token) in panels {
            if watches.contains_key(context_id) {
                continue; // Already watching — preserve etag/hash/interval state
            }

            let args = match crate::modules::github::classify::validate_gh_command(command) {
                Ok(a) => a,
                Err(_) => continue, // Skip invalid commands
            };

            let is_api_command = is_api_command(&args);

            let token = Arc::new(SecretBox::new(Box::new(github_token.clone())));
            watches.insert(context_id.clone(), GhWatch {
                context_id: context_id.clone(),
                github_token: token,
                args,
                is_api_command,
                etag: None,
                last_output_hash: None,
                poll_interval_secs: GH_DEFAULT_POLL_INTERVAL_SECS,
                last_poll_ms: 0, // Poll immediately on first sync; initial content comes from cache system but we want fast change detection
            });
        }
    }
}

/// Classify whether args represent a `gh api` command eligible for ETag polling.
/// Commands with `--jq`/`--template` post-process output, and `-i` would inject
/// headers into the jq input, so they fall back to hash-based polling.
fn is_api_command(args: &[String]) -> bool {
    args.first().map(|s| s.as_str()) == Some("api")
        && !args.iter().any(|a| a == "--jq" || a == "-q" || a == "--template" || a == "-t")
}

/// Background polling loop.
/// Wakes every `GH_WATCHER_TICK_SECS` and polls watches whose interval has elapsed.
///
/// # Why this sends CacheUpdate directly instead of setting cache_deprecated
///
/// The normal cache invalidation pattern is two-phase: mark a panel dirty
/// (`cache_deprecated = true`), then let the cache pipeline re-fetch data.
/// This watcher shortcuts that by sending `CacheUpdate::GithubResultContent`
/// directly through `cache_tx`.
///
/// This is intentional: both polling methods (ETag-based and hash-based)
/// inherently produce the new content as a byproduct of change detection.
/// Re-running the same `gh` command through the cache pipeline would be a
/// redundant second fetch. Since detection = fetching here, we send the
/// data we already have straight to `apply_cache_update`.
fn poll_loop(watches: Arc<Mutex<HashMap<String, GhWatch>>>, cache_tx: Sender<CacheUpdate>) {
    loop {
        thread::sleep(std::time::Duration::from_secs(GH_WATCHER_TICK_SECS));

        let current_ms = now_ms();

        // Snapshot only watches that are due for polling
        let due: Vec<(String, Vec<String>, Arc<SecretBox<String>>, bool, Option<String>, Option<String>)> = {
            let watches = watches.lock().expect("gh_watcher watches lock poisoned");
            watches.values()
                .filter(|w| current_ms.saturating_sub(w.last_poll_ms) >= w.poll_interval_secs * 1000)
                .map(|w| (
                    w.context_id.clone(),
                    w.args.clone(),
                    Arc::clone(&w.github_token),
                    w.is_api_command,
                    w.etag.clone(),
                    w.last_output_hash.clone(),
                ))
                .collect()
        };

        for (context_id, args, github_token, is_api, etag, last_hash) in due {
            let token_str = github_token.expose_secret();
            if is_api {
                let outcome = poll_api_command(&args, token_str, etag.as_deref());

                // Update watch state: always update last_poll_ms and poll_interval
                {
                    let mut watches = watches.lock().expect("gh_watcher watches lock poisoned");
                    if let Some(watch) = watches.get_mut(&context_id) {
                        watch.last_poll_ms = now_ms();
                        if let Some(interval) = outcome.poll_interval {
                            watch.poll_interval_secs = interval;
                        }
                        if let Some((ref new_etag, _)) = outcome.content {
                            watch.etag = new_etag.clone();
                        }
                    }
                }

                if let Some((_, body)) = outcome.content {
                    let body = redact_token(&body, token_str);
                    let body = truncate_output(&body, MAX_RESULT_CONTENT_BYTES);
                    let token_count = estimate_tokens(&body);

                    let _ = cache_tx.send(CacheUpdate::GithubResultContent {
                        context_id,
                        content: body,
                        token_count,
                        is_error: false,
                    });
                }
            } else {
                let result = poll_cli_command(&args, token_str, last_hash.as_deref());

                // Update watch state
                {
                    let mut watches = watches.lock().expect("gh_watcher watches lock poisoned");
                    if let Some(watch) = watches.get_mut(&context_id) {
                        watch.last_poll_ms = now_ms();
                        if let Some((ref new_hash, _)) = result {
                            watch.last_output_hash = Some(new_hash.clone());
                        }
                    }
                }

                if let Some((_, content)) = result {
                    let content = redact_token(&content, token_str);
                    let content = truncate_output(&content, MAX_RESULT_CONTENT_BYTES);
                    let token_count = estimate_tokens(&content);

                    let _ = cache_tx.send(CacheUpdate::GithubResultContent {
                        context_id,
                        content,
                        token_count,
                        is_error: false,
                    });
                }
            }
        }
    }
}

/// Outcome of an API poll attempt
struct ApiPollOutcome {
    /// New content if changed: (new_etag, body). None on 304 or error.
    content: Option<(Option<String>, String)>,
    /// X-Poll-Interval from response headers (seconds), if present
    poll_interval: Option<u64>,
}

/// Poll a `gh api` command using ETag-based conditional requests.
/// Extracts `X-Poll-Interval` from response headers on both 200 and 304.
fn poll_api_command(args: &[String], github_token: &str, current_etag: Option<&str>) -> ApiPollOutcome {
    // Build args: inject `-i` after "api" to include HTTP response headers
    let mut cmd_args = Vec::with_capacity(args.len() + 4);
    cmd_args.push(args[0].clone()); // "api"
    cmd_args.push("-i".to_string());
    cmd_args.extend_from_slice(&args[1..]);

    // If we have an etag, add conditional header
    if let Some(etag) = current_etag {
        cmd_args.push("-H".to_string());
        cmd_args.push(format!("If-None-Match: {}", etag));
    }

    let mut cmd = Command::new("gh");
    cmd.args(&cmd_args)
        .env("GITHUB_TOKEN", github_token)
        .env("GH_TOKEN", github_token)
        .env("GH_PROMPT_DISABLED", "1")
        .env("NO_COLOR", "1");

    let output = match run_with_timeout(cmd, GH_CMD_TIMEOUT_SECS) {
        Ok(o) => o,
        Err(_) => return ApiPollOutcome { content: None, poll_interval: None },
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // 304 Not Modified: gh exits non-zero with "304" in stderr
    if !output.status.success() && (stderr.contains("304") || stderr.contains("Not Modified")) {
        // Still extract X-Poll-Interval from stdout (gh may emit headers even on 304)
        let poll_interval = extract_poll_interval(&stdout);
        return ApiPollOutcome { content: None, poll_interval };
    }

    // Other errors — no usable data
    if !output.status.success() {
        return ApiPollOutcome { content: None, poll_interval: None };
    }

    // 200 OK: parse headers and body
    let (new_etag, poll_interval, body) = parse_api_response(&stdout);
    ApiPollOutcome {
        content: Some((new_etag, body)),
        poll_interval,
    }
}

/// Poll a non-API `gh` command using output hash comparison.
/// Returns `Some((new_hash, content))` on content change, `None` if unchanged or error.
fn poll_cli_command(args: &[String], github_token: &str, last_hash: Option<&str>) -> Option<(String, String)> {
    let mut cmd = Command::new("gh");
    cmd.args(args)
        .env("GITHUB_TOKEN", github_token)
        .env("GH_TOKEN", github_token)
        .env("GH_PROMPT_DISABLED", "1")
        .env("NO_COLOR", "1");

    let output = match run_with_timeout(cmd, GH_CMD_TIMEOUT_SECS) {
        Ok(o) => o,
        Err(_) => return None,
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let content = if stderr.trim().is_empty() {
        stdout.to_string()
    } else if stdout.trim().is_empty() {
        stderr.to_string()
    } else {
        format!("{}\n{}", stdout, stderr)
    };

    let new_hash = sha256_hex(&content);

    if last_hash == Some(new_hash.as_str()) {
        return None; // Content unchanged
    }

    Some((new_hash, content))
}

/// Parse a `gh api -i` response, splitting headers from body.
/// Returns `(etag, x_poll_interval, body)`.
fn parse_api_response(stdout: &str) -> (Option<String>, Option<u64>, String) {
    // Find the separator between headers and body
    // Try \r\n\r\n first, then \n\n
    let (headers, body) = if let Some(pos) = stdout.find("\r\n\r\n") {
        (&stdout[..pos], &stdout[pos + 4..])
    } else if let Some(pos) = stdout.find("\n\n") {
        (&stdout[..pos], &stdout[pos + 2..])
    } else {
        // No separator found — treat entire output as body
        return (None, None, stdout.to_string());
    };

    let etag = extract_header(headers, "etag");
    let poll_interval = extract_header(headers, "x-poll-interval")
        .and_then(|v| v.parse::<u64>().ok());

    (etag, poll_interval, body.to_string())
}

/// Extract a header value by name (case-insensitive) from raw header text.
fn extract_header(headers: &str, name: &str) -> Option<String> {
    let prefix = format!("{}:", name);
    headers.lines().find_map(|line| {
        if line.to_lowercase().starts_with(&prefix) {
            Some(line[prefix.len()..].trim().to_string())
        } else {
            None
        }
    })
}

/// Try to extract X-Poll-Interval from raw output (for 304 responses where
/// gh may still emit response headers to stdout).
fn extract_poll_interval(stdout: &str) -> Option<u64> {
    extract_header(stdout, "x-poll-interval")
        .and_then(|v| v.parse::<u64>().ok())
}

/// Compute SHA-256 hex hash of a string
fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:064x}", hasher.finalize())
}

/// Redact a GitHub token from command output if accidentally leaked.
fn redact_token(output: &str, token: &str) -> String {
    if token.len() >= 8 && output.contains(token) {
        output.replace(token, "[REDACTED]")
    } else {
        output.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse_api_response ---

    #[test]
    fn test_parse_api_response_crlf_separator() {
        let input = "HTTP/2.0 200 OK\r\nETag: \"abc123\"\r\nX-Poll-Interval: 60\r\n\r\n{\"id\": 1}";
        let (etag, poll_interval, body) = parse_api_response(input);
        assert_eq!(etag, Some("\"abc123\"".to_string()));
        assert_eq!(poll_interval, Some(60));
        assert_eq!(body, "{\"id\": 1}");
    }

    #[test]
    fn test_parse_api_response_lf_separator() {
        let input = "HTTP/2.0 200 OK\nEtag: W/\"xyz789\"\nX-Poll-Interval: 120\n\n[{\"name\": \"test\"}]";
        let (etag, poll_interval, body) = parse_api_response(input);
        assert_eq!(etag, Some("W/\"xyz789\"".to_string()));
        assert_eq!(poll_interval, Some(120));
        assert_eq!(body, "[{\"name\": \"test\"}]");
    }

    #[test]
    fn test_parse_api_response_no_separator() {
        let input = "{\"id\": 1, \"name\": \"test\"}";
        let (etag, poll_interval, body) = parse_api_response(input);
        assert_eq!(etag, None);
        assert_eq!(poll_interval, None);
        assert_eq!(body, input);
    }

    #[test]
    fn test_parse_api_response_case_insensitive_etag() {
        let input = "HTTP/2.0 200 OK\r\nETAG: \"UPPER\"\r\n\r\nbody";
        let (etag, _poll_interval, body) = parse_api_response(input);
        assert_eq!(etag, Some("\"UPPER\"".to_string()));
        assert_eq!(body, "body");
    }

    #[test]
    fn test_parse_api_response_no_etag_header() {
        let input = "HTTP/2.0 200 OK\r\nContent-Type: text/plain\r\n\r\nhello";
        let (etag, poll_interval, body) = parse_api_response(input);
        assert_eq!(etag, None);
        assert_eq!(poll_interval, None);
        assert_eq!(body, "hello");
    }

    #[test]
    fn test_parse_api_response_no_poll_interval() {
        let input = "HTTP/2.0 200 OK\r\nETag: \"e1\"\r\n\r\ndata";
        let (etag, poll_interval, body) = parse_api_response(input);
        assert_eq!(etag, Some("\"e1\"".to_string()));
        assert_eq!(poll_interval, None);
        assert_eq!(body, "data");
    }

    #[test]
    fn test_parse_api_response_poll_interval_case_insensitive() {
        let input = "HTTP/2.0 200 OK\r\nx-poll-interval: 90\r\n\r\ndata";
        let (_etag, poll_interval, _body) = parse_api_response(input);
        assert_eq!(poll_interval, Some(90));
    }

    // --- extract_poll_interval ---

    #[test]
    fn test_extract_poll_interval_from_304_headers() {
        let stdout = "HTTP/2.0 304 Not Modified\r\nX-Poll-Interval: 60\r\nETag: \"abc\"\r\n";
        assert_eq!(extract_poll_interval(stdout), Some(60));
    }

    #[test]
    fn test_extract_poll_interval_missing() {
        let stdout = "HTTP/2.0 304 Not Modified\r\nETag: \"abc\"\r\n";
        assert_eq!(extract_poll_interval(stdout), None);
    }

    #[test]
    fn test_extract_poll_interval_empty() {
        assert_eq!(extract_poll_interval(""), None);
    }

    // --- is_api_command ---

    #[test]
    fn test_is_api_command_basic() {
        let args: Vec<String> = vec!["api", "/repos/foo/bar"].iter().map(|s| s.to_string()).collect();
        assert!(is_api_command(&args));
    }

    #[test]
    fn test_is_api_command_with_jq() {
        let args: Vec<String> = vec!["api", "/repos/foo/bar", "--jq", ".x"].iter().map(|s| s.to_string()).collect();
        assert!(!is_api_command(&args));
    }

    #[test]
    fn test_is_api_command_with_short_jq() {
        let args: Vec<String> = vec!["api", "/repos/foo/bar", "-q", ".x"].iter().map(|s| s.to_string()).collect();
        assert!(!is_api_command(&args));
    }

    #[test]
    fn test_is_api_command_with_template() {
        let args: Vec<String> = vec!["api", "/repos/foo/bar", "--template", "{{.name}}"].iter().map(|s| s.to_string()).collect();
        assert!(!is_api_command(&args));
    }

    #[test]
    fn test_is_api_command_with_short_template() {
        let args: Vec<String> = vec!["api", "/repos/foo/bar", "-t", "{{.name}}"].iter().map(|s| s.to_string()).collect();
        assert!(!is_api_command(&args));
    }

    #[test]
    fn test_is_api_command_non_api() {
        let args: Vec<String> = vec!["pr", "list"].iter().map(|s| s.to_string()).collect();
        assert!(!is_api_command(&args));
    }

    #[test]
    fn test_is_api_command_empty() {
        let args: Vec<String> = vec![];
        assert!(!is_api_command(&args));
    }
}
