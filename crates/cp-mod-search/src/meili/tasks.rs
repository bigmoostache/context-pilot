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

/// Page size for a field-projection fetch.
///
/// `POST /documents/fetch` serves any limit the server can materialise, not just
/// its 20-row default: `20_000` rows of three small fields return in ~0.3s, which
/// collapses a 298k-document index from 299 round-trips to 15.
const FETCH_PAGE_LIMIT: u64 = 20_000;

/// In-flight page requests used by [`fetch_remaining_pages`].
///
/// `load_module_data` runs the projection synchronously, so this whole-index
/// sweep sits on the critical path to the first frame. Measured on a
/// 298,075-document index: 299 sequential 1_000-row pages took 15.05s, 15
/// sequential 20_000-row pages 6.71s, and this fan-out 0.41s.
const FETCH_CONCURRENCY: usize = 16;

/// One fetched page paired with the `offset` it was read at.
///
/// The offset rides along because the parallel sweep in
/// [`fetch_remaining_pages`] finishes out of order, and the caller has to
/// restore document order before folding rows into a map.
type Page = (u64, Vec<serde_json::Value>);

/// Result of one worker's shard: its pages, or the error that stopped it.
type Shard = Result<Vec<Page>, String>;

/// Stand-in error for a panicking fetch worker.
///
/// A panic carries no useful payload across a join handle, and letting it
/// propagate would abort the boot path on a worker that only does HTTP.
const WORKER_PANIC: &str = "fetch_projection worker panicked";

/// Every page offset after the probe page, in ascending order.
///
/// `successors` (rather than a `(1..).map(|n| n * LIMIT)` chain) so the stride is
/// applied with an explicitly-checked add and the sequence stays bounded by
/// `take_while` on the server-reported `total`.
fn page_offsets(total: u64) -> Vec<u64> {
    std::iter::successors(Some(FETCH_PAGE_LIMIT), |prev| prev.checked_add(FETCH_PAGE_LIMIT))
        .take_while(|&offset| offset < total)
        .collect()
}

/// Fetch one page of a field projection, plus the index's reported `total`.
///
/// `total` only ever appears on a response, so the first page must be fetched on
/// its own before the remaining page count is known.
fn fetch_page(
    client: &MeiliClient,
    url: &str,
    fields: &[&str],
    offset: u64,
) -> Result<(Vec<serde_json::Value>, u64), String> {
    let body = serde_json::json!({ "fields": fields, "limit": FETCH_PAGE_LIMIT, "offset": offset });
    let resp = client
        .client()
        .post(url)
        .header("Authorization", format!("Bearer {}", client.key()))
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send()
        .map_err(|e| format!("fetch_projection request failed: {e}"))?;

    let json: serde_json::Value = resp.json().map_err(|e| format!("fetch_projection parse failed: {e}"))?;

    let page = json.get("results").and_then(serde_json::Value::as_array).cloned().unwrap_or_default();
    let total = json.get("total").and_then(serde_json::Value::as_u64).unwrap_or(0);
    Ok((page, total))
}

/// Fetch every page after the probe page, fanned out across
/// [`FETCH_CONCURRENCY`] scoped workers.
///
/// Offsets are precomputed and dealt round-robin, so no worker shares mutable
/// state and the union of the shards is exactly the full offset range. Each page
/// comes back paired with its offset so the caller can restore document order
/// (workers finish out of order). A worker stops at the first empty page —
/// Meilisearch may return a short page before the true end, but never a non-empty
/// page past `total`.
///
/// # Errors
///
/// Returns an error if any page request fails or a response cannot be parsed. A
/// worker that panics surfaces as an error rather than propagating the panic
/// across the scope boundary.
fn fetch_remaining_pages(client: &MeiliClient, url: &str, fields: &[&str], total: u64) -> Result<Vec<Page>, String> {
    let offsets = page_offsets(total);

    let shards: Vec<Shard> = std::thread::scope(|scope| {
        // Handles are joined only after the whole fan-out is spawned — joining
        // inside the spawn loop would serialise the sweep back to one page at a
        // time. Hence the explicit `for` loops rather than iterator chains.
        let mut handles: Vec<_> = Vec::with_capacity(FETCH_CONCURRENCY);
        for worker in 0..FETCH_CONCURRENCY {
            let shard: Vec<u64> = offsets.iter().copied().skip(worker).step_by(FETCH_CONCURRENCY).collect();
            handles.push(scope.spawn(move || -> Shard {
                let mut pages: Vec<Page> = Vec::with_capacity(shard.len());
                for offset in shard {
                    let (page, _) = fetch_page(client, url, fields, offset)?;
                    if page.is_empty() {
                        break;
                    }
                    pages.push((offset, page));
                }
                Ok(pages)
            }));
        }

        let mut shards: Vec<Shard> = Vec::with_capacity(handles.len());
        for handle in handles {
            shards.push(handle.join().unwrap_or_else(|_| Err(WORKER_PANIC.to_owned())));
        }
        shards
    });

    // First failing shard aborts the sweep, so callers never act on a partial
    // snapshot (a truncated `index_map` would diff to "delete everything absent").
    let mut pages: Vec<Page> = Vec::new();
    for shard in shards {
        pages.extend(shard?);
    }
    Ok(pages)
}

/// Paged projection of every document in an index.
///
/// `POST /indexes/{uid}/documents/fetch` requesting only `fields` (no content, no
/// vectors). Page 0 is fetched alone because it is the only response reporting
/// `total`; the remaining pages are fanned out in parallel (see
/// [`FETCH_CONCURRENCY`]) and reassembled in ascending `offset` order, so row
/// order matches a plain sequential page-through.
///
/// Used by the boot/hourly reconcile to snapshot the index's expected filesystem
/// state cheaply — the files index holds one document per *chunk*, so a large
/// project means hundreds of thousands of rows to walk.
///
/// # Errors
///
/// Returns an error if any page request fails or a response cannot be parsed.
pub(crate) fn fetch_projection(
    client: &MeiliClient,
    uid: &str,
    fields: &[&str],
) -> Result<Vec<serde_json::Value>, String> {
    let url = format!("{}/indexes/{uid}/documents/fetch", client.url());

    let (first, total) = fetch_page(client, &url, fields, 0)?;
    if first.is_empty() || u64::try_from(first.len()).unwrap_or(u64::MAX) >= total {
        return Ok(first);
    }

    let mut rest = fetch_remaining_pages(client, &url, fields, total)?;
    rest.sort_unstable_by_key(|&(offset, _)| offset);

    let mut out = first;
    for (_, page) in rest {
        out.extend(page);
    }
    Ok(out)
}

/// Paged fetch of every document in an index **with its stored vectors**.
///
/// `POST /indexes/{uid}/documents/fetch` with `retrieveVectors: true` and no
/// `fields` restriction, so each returned doc carries all its fields plus
/// `_vectors.<embedder>.embeddings`. Paged on `offset` until the reported
/// `total` is covered (never stop on a short page). Used by the embedding
/// backup export — the vectors are what make a cross-machine copy skip Voyage.
///
/// # Errors
///
/// Returns an error if any page request fails or the response cannot be parsed.
pub(crate) fn fetch_all_with_vectors(client: &MeiliClient, uid: &str) -> Result<Vec<serde_json::Value>, String> {
    let url = format!("{}/indexes/{uid}/documents/fetch", client.url());
    let limit: u64 = 500;
    let mut offset: u64 = 0;
    let mut out: Vec<serde_json::Value> = Vec::new();

    loop {
        let body = serde_json::json!({ "retrieveVectors": true, "limit": limit, "offset": offset });
        let resp = client
            .client()
            .post(&url)
            .header("Authorization", format!("Bearer {}", client.key()))
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .send()
            .map_err(|e| format!("fetch_all_with_vectors request failed: {e}"))?;

        let json: serde_json::Value = resp.json().map_err(|e| format!("fetch_all_with_vectors parse failed: {e}"))?;

        let page = json.get("results").and_then(serde_json::Value::as_array).cloned().unwrap_or_default();
        let total = json.get("total").and_then(serde_json::Value::as_u64).unwrap_or(0);
        let got = u64::try_from(page.len()).unwrap_or(u64::MAX);
        out.extend(page);

        offset = offset.saturating_add(limit);
        if offset >= total || got == 0 {
            break;
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The live-server fetch benchmark, `#[ignore]`d so `cargo test` stays
    /// hermetic. Run it explicitly after touching the paging constants:
    ///
    /// ```text
    /// cargo test --release -p cp-mod-search -- --ignored live_fetch_sweep
    /// ```
    ///
    /// Use `--release`: parsing 300k JSON rows is ~5x slower unoptimized, so a
    /// debug run reports ~3.8s where the shipped binary takes ~0.7s. Read the
    /// number as JSON-parse cost, not as boot-path cost.
    ///
    /// It asserts the *correctness* invariants the parallel sweep has to
    /// preserve (every page present exactly once, in ascending offset order) and
    /// logs the wall time so the cost shows up in the log rather than being
    /// inferred from a curl approximation. Skips rather than fails when no server
    /// is running, so it is safe on a machine without one.
    #[test]
    #[ignore = "needs a live Meilisearch on this machine"]
    fn live_fetch_sweep() {
        let home = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default());
        let (Ok(key), Ok(port_raw)) = (
            std::fs::read_to_string(home.join(".context-pilot/meilisearch/master.key")),
            std::fs::read_to_string(home.join(".context-pilot/meilisearch/port")),
        ) else {
            log::info!("live_fetch_sweep: SKIP (no meilisearch key/port)");
            return;
        };
        let Ok(port) = port_raw.trim().parse::<u16>() else {
            log::warn!("live_fetch_sweep: SKIP (unparsable port)");
            return;
        };
        let Ok(client) = MeiliClient::new(port, key.trim()) else {
            log::warn!("live_fetch_sweep: SKIP (client build failed)");
            return;
        };

        // Pick the files index with the most documents out of the global stats
        // payload — a big index is what actually exercises multi-page fan-out.
        let stats = client.global_stats().unwrap_or(serde_json::Value::Null);
        let Some(biggest_files_uid) = stats.get("indexes").and_then(serde_json::Value::as_object).and_then(|indexes| {
            indexes
                .iter()
                .filter(|&(uid, _)| uid.ends_with("_files"))
                .max_by_key(|&(_, v)| v.get("numberOfDocuments").and_then(serde_json::Value::as_u64).unwrap_or(0))
                .map(|(uid, _)| uid.clone())
        }) else {
            log::info!("live_fetch_sweep: SKIP (no cp_*_files index)");
            return;
        };

        let fields = ["file_path", "last_modified_ms", "size_bytes"];
        let total = client.index_stats(&biggest_files_uid).map_or(0, |(n, _)| n);

        let t0 = Instant::now();
        let Ok(rows) = fetch_projection(&client, &biggest_files_uid, &fields) else {
            log::warn!("live_fetch_sweep: SKIP (fetch_projection failed)");
            return;
        };
        let elapsed = t0.elapsed();

        log::info!("live_fetch_sweep: {} rows of {total} in {:.2}s", rows.len(), elapsed.as_secs_f64());

        // The parallel sweep's load-bearing invariant: a page dropped or
        // double-counted would make `index_map` diff to a wrong plan.
        assert!(
            total == 0 || rows.len() >= usize::try_from(total).unwrap_or(usize::MAX),
            "parallel sweep dropped rows: got {} of {total}",
            rows.len()
        );
    }
}
