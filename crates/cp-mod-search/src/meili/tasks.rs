//! Meilisearch task polling and UID extraction.
//!
//! Free functions (not `impl MeiliClient`) to avoid the
//! `multiple_inherent_impl` lint while keeping `client.rs` under 500 lines.

use std::time::{Duration, Instant};

use super::api::MeiliClient;

/// Poll a task until it reaches a terminal state (`succeeded` or `failed`).
///
/// Polls every 200ms for up to 30 seconds.
///
/// # Errors
///
/// Returns an error if the task fails, times out, or the API is unreachable.
pub(crate) fn wait_for_task(client: &MeiliClient, task_uid: u64) -> Result<(), String> {
    let timeout = Duration::from_secs(30);
    let interval = Duration::from_millis(200);
    let deadline = Instant::now().checked_add(timeout);

    loop {
        let url = format!("{}/tasks/{task_uid}", client.url());
        let resp = client
            .client()
            .get(&url)
            .header("Authorization", format!("Bearer {}", client.key()))
            .send()
            .map_err(|e| format!("task poll failed: {e}"))?;

        let json: serde_json::Value = resp.json().map_err(|e| format!("task poll: cannot parse response: {e}"))?;

        let status = json.get("status").and_then(serde_json::Value::as_str).unwrap_or("unknown");

        match status {
            "succeeded" => return Ok(()),
            "failed" => {
                let err = json
                    .get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("unknown error");
                return Err(format!("Meilisearch task {task_uid} failed: {err}"));
            }
            "canceled" => {
                return Err(format!("Meilisearch task {task_uid} was canceled"));
            }
            // "enqueued" | "processing" → keep polling
            _ => {}
        }

        if deadline.is_some_and(|d| Instant::now() >= d) {
            return Err(format!("Meilisearch task {task_uid} did not complete within {timeout:?}"));
        }

        std::thread::sleep(interval);
    }
}

/// Extract `taskUid` from an API response that returns a task.
///
/// Meilisearch returns `202 Accepted` with `{ "taskUid": N, ... }` for
/// asynchronous operations.
pub(super) fn extract_task_uid(resp: reqwest::blocking::Response, operation: &str) -> Result<u64, String> {
    let status = resp.status().as_u16();
    let json: serde_json::Value = resp.json().map_err(|e| format!("{operation}: cannot parse response: {e}"))?;

    if status == 202 {
        json.get("taskUid")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| format!("{operation}: response missing 'taskUid'"))
    } else {
        let msg = json.get("message").and_then(serde_json::Value::as_str).unwrap_or("unknown error");
        Err(format!("{operation} returned HTTP {status}: {msg}"))
    }
}
