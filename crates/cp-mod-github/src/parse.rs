//! Parsing utilities for GitHub API responses, PR JSON, and helpers.

use std::process::Command;

use sha2::{Digest as _, Sha256};

use cp_base::modules::run_with_timeout;

use crate::GH_CMD_TIMEOUT_SECS;

/// Parse a `gh api -i` response, splitting headers from body.
#[must_use]
pub fn api_response(stdout: &str) -> (Option<String>, Option<u64>, String) {
    let (headers, body) = if let Some(pos) = stdout.find("\r\n\r\n") {
        (stdout.get(..pos).unwrap_or(""), stdout.get(pos.saturating_add(4)..).unwrap_or(""))
    } else if let Some(pos) = stdout.find("\n\n") {
        (stdout.get(..pos).unwrap_or(""), stdout.get(pos.saturating_add(2)..).unwrap_or(""))
    } else {
        return (None, None, stdout.to_string());
    };

    let etag = extract_header(headers, "etag");
    let poll_interval = extract_header(headers, "x-poll-interval").and_then(|v| v.parse::<u64>().ok());

    (etag, poll_interval, body.to_string())
}

/// Extract a specific HTTP header value (case-insensitive key match).
#[must_use]
pub fn extract_header(headers: &str, name: &str) -> Option<String> {
    let prefix = format!("{name}:");
    headers.lines().find_map(|line| {
        line.to_lowercase().starts_with(&prefix).then(|| line.get(prefix.len()..).unwrap_or("").trim().to_string())
    })
}

/// Try to extract X-Poll-Interval from raw output.
#[must_use]
pub fn extract_poll_interval(stdout: &str) -> Option<u64> {
    let v = extract_header(stdout, "x-poll-interval")?;
    v.parse::<u64>().ok()
}

/// SHA-256 hex digest of a string — used for change detection.
#[must_use]
pub fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:064x}", hasher.finalize())
}

/// Replace a GitHub token in output with `[REDACTED]` for safe display.
#[must_use]
pub fn redact_token(output: &str, token: &str) -> String {
    if token.len() >= 8 && output.contains(token) { output.replace(token, "[REDACTED]") } else { output.to_string() }
}

/// Poll for a PR associated with the given branch.
/// Returns `Some((hash, pr_info))` if output changed; `pr_info` is `None` when no PR exists.
#[must_use]
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
    let pattern = format!("\"{key}\":\"");
    if let Some(start) = json.find(&pattern) {
        let value_start = start.saturating_add(pattern.len());
        let rest = json.get(value_start..).unwrap_or("");
        let mut end = 0usize;
        let mut escaped = false;
        for ch in rest.chars() {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                break;
            }
            end = end.saturating_add(ch.len_utf8());
        }
        return Some(rest.get(..end).unwrap_or("").to_string());
    }
    None
}

/// Extract a u64 value from JSON by key
fn extract_json_u64(json: &str, key: &str) -> Option<u64> {
    let pattern = format!("\"{key}\":");
    if let Some(start) = json.find(&pattern) {
        let value_start = start.saturating_add(pattern.len());
        let rest = json.get(value_start..).unwrap_or("").trim_start();
        let num_str: String = rest.chars().take_while(char::is_ascii_digit).collect();
        return num_str.parse().ok();
    }
    None
}

/// Parse the overall checks status from statusCheckRollup
fn parse_checks_status(json: &str) -> Option<String> {
    if !json.contains("statusCheckRollup") {
        return None;
    }
    let success = json
        .matches("\"conclusion\":\"SUCCESS\"")
        .count()
        .saturating_add(json.matches("\"conclusion\":\"NEUTRAL\"").count())
        .saturating_add(json.matches("\"conclusion\":\"SKIPPED\"").count());
    let failure = json
        .matches("\"conclusion\":\"FAILURE\"")
        .count()
        .saturating_add(json.matches("\"conclusion\":\"TIMED_OUT\"").count())
        .saturating_add(json.matches("\"conclusion\":\"CANCELLED\"").count());
    let pending = json
        .matches("\"conclusion\":\"\"")
        .count()
        .saturating_add(json.matches("\"conclusion\":null").count())
        .saturating_add(json.matches("\"status\":\"IN_PROGRESS\"").count())
        .saturating_add(json.matches("\"status\":\"QUEUED\"").count())
        .saturating_add(json.matches("\"status\":\"PENDING\"").count());

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
