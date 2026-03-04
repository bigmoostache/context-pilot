//! Parsing utilities for GitHub API responses, PR JSON, and helpers.

use std::process::Command;

use sha2::{Digest, Sha256};

use cp_base::modules::run_with_timeout;

use crate::GH_CMD_TIMEOUT_SECS;

/// Parse a `gh api -i` response, splitting headers from body.
pub fn parse_api_response(stdout: &str) -> (Option<String>, Option<u64>, String) {
    let (headers, body) = if let Some(pos) = stdout.find("\r\n\r\n") {
        (&stdout[..pos], &stdout[pos + 4..])
    } else if let Some(pos) = stdout.find("\n\n") {
        (&stdout[..pos], &stdout[pos + 2..])
    } else {
        return (None, None, stdout.to_string());
    };

    let etag = extract_header(headers, "etag");
    let poll_interval = extract_header(headers, "x-poll-interval").and_then(|v| v.parse::<u64>().ok());

    (etag, poll_interval, body.to_string())
}

/// Extract a specific HTTP header value (case-insensitive key match).
pub fn extract_header(headers: &str, name: &str) -> Option<String> {
    let prefix = format!("{}:", name);
    headers.lines().find_map(|line| {
        if line.to_lowercase().starts_with(&prefix) { Some(line[prefix.len()..].trim().to_string()) } else { None }
    })
}

/// Try to extract X-Poll-Interval from raw output.
pub fn extract_poll_interval(stdout: &str) -> Option<u64> {
    extract_header(stdout, "x-poll-interval").and_then(|v| v.parse::<u64>().ok())
}

/// SHA-256 hex digest of a string — used for change detection.
pub fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:064x}", hasher.finalize())
}

/// Replace a GitHub token in output with `[REDACTED]` for safe display.
pub fn redact_token(output: &str, token: &str) -> String {
    if token.len() >= 8 && output.contains(token) { output.replace(token, "[REDACTED]") } else { output.to_string() }
}

/// Poll for a PR associated with the given branch.
/// Returns `Some((hash, pr_info))` if output changed; `pr_info` is `None` when no PR exists.
pub fn poll_branch_pr(
    branch: &str,
    github_token: &str,
    last_hash: Option<&str>,
) -> Option<(String, Option<crate::types::BranchPrInfo>)> {
    let mut cmd = Command::new("gh");
    let _r = cmd
        .args([
            "pr",
            "view",
            branch,
            "--json",
            "number,title,state,url,additions,deletions,reviewDecision,statusCheckRollup",
        ])
        .env("GITHUB_TOKEN", github_token)
        .env("GH_TOKEN", github_token)
        .env("GH_PROMPT_DISABLED", "1")
        .env("NO_COLOR", "1");

    let Ok(output) = run_with_timeout(cmd, GH_CMD_TIMEOUT_SECS) else { return None };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // No PR for this branch
    if !output.status.success() || stderr.contains("no pull requests found") || stdout.trim().is_empty() {
        let hash = sha256_hex("no_pr");
        if last_hash == Some(hash.as_str()) {
            return None; // unchanged
        }
        return Some((hash, None));
    }

    let content = stdout.to_string();
    let new_hash = sha256_hex(&content);
    if last_hash == Some(new_hash.as_str()) {
        return None; // unchanged
    }

    // Parse JSON response
    let pr_info = parse_pr_json(&content);
    Some((new_hash, pr_info))
}

/// Parse the JSON from `gh pr view --json ...`
fn parse_pr_json(json_str: &str) -> Option<crate::types::BranchPrInfo> {
    let number = extract_json_u64(json_str, "number")?;
    let title = extract_json_string(json_str, "title")?;
    let state = extract_json_string(json_str, "state").unwrap_or_else(|| "OPEN".to_string());
    let url = extract_json_string(json_str, "url").unwrap_or_default();
    let additions = extract_json_u64(json_str, "additions");
    let deletions = extract_json_u64(json_str, "deletions");
    let review_decision = extract_json_string(json_str, "reviewDecision");
    let checks_status = parse_checks_status(json_str);

    Some(crate::types::BranchPrInfo { number, title, state, url, additions, deletions, review_decision, checks_status })
}

/// Extract a string value from JSON by key (simple parser, no serde dependency)
fn extract_json_string(json: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{}\":\"", key);
    if let Some(start) = json.find(&pattern) {
        let value_start = start + pattern.len();
        let rest = &json[value_start..];
        let mut end = 0;
        let mut escaped = false;
        for ch in rest.chars() {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                break;
            }
            end += ch.len_utf8();
        }
        return Some(rest[..end].to_string());
    }
    None
}

/// Extract a u64 value from JSON by key
fn extract_json_u64(json: &str, key: &str) -> Option<u64> {
    let pattern = format!("\"{}\":", key);
    if let Some(start) = json.find(&pattern) {
        let value_start = start + pattern.len();
        let rest = json[value_start..].trim_start();
        let num_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        return num_str.parse().ok();
    }
    None
}

/// Parse the overall checks status from statusCheckRollup
fn parse_checks_status(json: &str) -> Option<String> {
    if !json.contains("statusCheckRollup") {
        return None;
    }
    let success = json.matches("\"conclusion\":\"SUCCESS\"").count()
        + json.matches("\"conclusion\":\"NEUTRAL\"").count()
        + json.matches("\"conclusion\":\"SKIPPED\"").count();
    let failure = json.matches("\"conclusion\":\"FAILURE\"").count()
        + json.matches("\"conclusion\":\"TIMED_OUT\"").count()
        + json.matches("\"conclusion\":\"CANCELLED\"").count();
    let pending = json.matches("\"conclusion\":\"\"").count()
        + json.matches("\"conclusion\":null").count()
        + json.matches("\"status\":\"IN_PROGRESS\"").count()
        + json.matches("\"status\":\"QUEUED\"").count()
        + json.matches("\"status\":\"PENDING\"").count();

    if failure > 0 {
        Some("failing".to_string())
    } else if pending > 0 {
        Some("pending".to_string())
    } else if success > 0 {
        Some("passing".to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_is_api_command_basic() {
        let args: Vec<String> = ["api", "/repos/foo/bar"].iter().map(|s| s.to_string()).collect();
        assert!(crate::watcher::is_api_command(&args));
    }

    #[test]
    fn test_is_api_command_with_jq() {
        let args: Vec<String> = ["api", "/repos/foo/bar", "--jq", ".x"].iter().map(|s| s.to_string()).collect();
        assert!(!super::super::watcher::is_api_command(&args));
    }

    #[test]
    fn test_is_api_command_with_short_jq() {
        let args: Vec<String> = ["api", "/repos/foo/bar", "-q", ".x"].iter().map(|s| s.to_string()).collect();
        assert!(!super::super::watcher::is_api_command(&args));
    }

    #[test]
    fn test_is_api_command_with_template() {
        let args: Vec<String> =
            ["api", "/repos/foo/bar", "--template", "{{.name}}"].iter().map(|s| s.to_string()).collect();
        assert!(!super::super::watcher::is_api_command(&args));
    }

    #[test]
    fn test_is_api_command_with_short_template() {
        let args: Vec<String> = ["api", "/repos/foo/bar", "-t", "{{.name}}"].iter().map(|s| s.to_string()).collect();
        assert!(!super::super::watcher::is_api_command(&args));
    }

    #[test]
    fn test_is_api_command_non_api() {
        let args: Vec<String> = ["pr", "list"].iter().map(|s| s.to_string()).collect();
        assert!(!super::super::watcher::is_api_command(&args));
    }

    #[test]
    fn test_is_api_command_empty() {
        let args: Vec<String> = vec![];
        assert!(!super::super::watcher::is_api_command(&args));
    }
}
