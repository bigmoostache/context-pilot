//! Shared output helpers: token redaction, stdout/stderr merge, CI rollup.

use serde_json::Value;

/// Replace a GitHub token in output with `[REDACTED]` for safe display.
/// Tokens shorter than 8 bytes are ignored (they would redact ordinary text).
#[must_use]
pub fn redact_token(output: &str, token: &str) -> String {
    if token.len() >= 8 && output.contains(token) { output.replace(token, "[REDACTED]") } else { output.to_owned() }
}

/// Trimmed stdout and stderr joined by a newline (either may be empty), token redacted.
#[must_use]
pub fn combine_output(output: &std::process::Output, token: &str) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let raw = match (stdout.trim(), stderr.trim()) {
        (out, "") => out.to_owned(),
        ("", err) => err.to_owned(),
        (out, err) => format!("{out}\n{err}"),
    };
    redact_token(&raw, token)
}

/// Overall CI verdict of a PR's `statusCheckRollup`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checks {
    /// Every check finished successfully (or neutral/skipped).
    Passing,
    /// Nothing failed yet, but something is still queued or running.
    Pending,
    /// At least one check failed, errored, timed out or was cancelled.
    Failing,
}

/// Verdict of one rollup entry. A `CheckRun` reports `status` + `conclusion`;
/// a `StatusContext` (commit status API: Vercel, Netlify, …) reports `state`.
fn entry_verdict(entry: &Value) -> Option<Checks> {
    let field = |k: &str| entry.get(k).and_then(Value::as_str).unwrap_or("");
    let verdict = match (field("state"), field("status"), field("conclusion")) {
        ("SUCCESS", ..) => Checks::Passing,
        ("PENDING" | "EXPECTED", ..) => Checks::Pending,
        ("FAILURE" | "ERROR", ..) => Checks::Failing,
        (_, status, _) if !status.is_empty() && status != "COMPLETED" => Checks::Pending,
        (.., "SUCCESS" | "NEUTRAL" | "SKIPPED") => Checks::Passing,
        (.., "FAILURE" | "TIMED_OUT" | "CANCELLED" | "ACTION_REQUIRED" | "STARTUP_FAILURE" | "STALE") => {
            Checks::Failing
        }
        (_, "COMPLETED", "") => Checks::Pending,
        _ => return None,
    };
    Some(verdict)
}

/// CI verdict of a `gh pr … --json statusCheckRollup` object; `None` when it
/// has no checks. Failing beats pending beats passing.
#[must_use]
pub fn checks_status(pr: &Value) -> Option<Checks> {
    let verdicts: Vec<Checks> = pr.get("statusCheckRollup")?.as_array()?.iter().filter_map(entry_verdict).collect();
    [Checks::Failing, Checks::Pending, Checks::Passing].into_iter().find(|v| verdicts.contains(v))
}

#[cfg(test)]
mod tests {
    use super::{Checks, checks_status, redact_token};
    use serde_json::json;

    #[test]
    fn status_context_pending_is_not_green() {
        let pr = json!({"statusCheckRollup": [
            {"__typename": "CheckRun", "status": "COMPLETED", "conclusion": "SUCCESS"},
            {"__typename": "StatusContext", "context": "vercel", "state": "PENDING"}
        ]});
        assert_eq!(checks_status(&pr), Some(Checks::Pending));
    }

    #[test]
    fn status_context_failure_wins() {
        let pr = json!({"statusCheckRollup": [
            {"status": "IN_PROGRESS", "conclusion": ""},
            {"state": "ERROR"}
        ]});
        assert_eq!(checks_status(&pr), Some(Checks::Failing));
    }

    #[test]
    fn passing_and_empty() {
        let ok = json!({"statusCheckRollup": [{"status": "COMPLETED", "conclusion": "SKIPPED"}, {"state": "SUCCESS"}]});
        assert_eq!(checks_status(&ok), Some(Checks::Passing));
        assert_eq!(checks_status(&json!({"statusCheckRollup": []})), None);
    }

    #[test]
    fn redacts_only_real_tokens() {
        assert_eq!(redact_token("x ghp_12345678 y", "ghp_12345678"), "x [REDACTED] y");
        assert_eq!(redact_token("abc", "a"), "abc");
    }
}
