//! Meilisearch HTTP API client.
//!
//! Thin wrapper around the Meilisearch REST API, providing typed methods
//! for index management, document operations, and search queries.
//! Methods are added incrementally as each implementation phase lands.

use std::time::{Duration, Instant};

/// HTTP client for a running Meilisearch server.
///
/// Created on-the-fly when needed (not stored in `SearchState`).
/// The inner `reqwest::blocking::Client` is cheap to construct.
#[derive(Debug)]
pub(crate) struct MeiliClient {
    /// Base URL including port, e.g. `http://127.0.0.1:7700`.
    base_url: String,
    /// Bearer token for authentication.
    api_key: String,
    /// Reusable HTTP client with connection pooling.
    http: reqwest::blocking::Client,
}

/// Query parameters for a single-index search.
///
/// Passed to [`MeiliClient::search`] to avoid excessive function arguments.
pub(crate) struct SearchParams<'qry> {
    /// Index UID to search.
    pub uid: &'qry str,
    /// Free-text query string.
    pub query: &'qry str,
    /// Optional Meilisearch filter expression.
    pub filter: Option<&'qry str>,
    /// Optional sort expression (e.g. `"last_modified_ms:desc"`).
    pub sort: Option<&'qry str>,
    /// Maximum number of results to return.
    pub limit: u32,
}

impl MeiliClient {
    /// Create a new client pointing at a local Meilisearch server.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be built.
    pub(crate) fn new(port: u16, api_key: &str) -> Result<Self, String> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| format!("Cannot create HTTP client: {e}"))?;

        Ok(Self { base_url: format!("http://127.0.0.1:{port}"), api_key: api_key.to_string(), http })
    }

    // -- Index operations ----------------------------------------------------

    /// Check whether an index exists.
    ///
    /// Uses `GET /indexes/{uid}` — returns `true` on 200, `false` on 404.
    ///
    /// # Errors
    ///
    /// Returns an error on network failures or unexpected status codes.
    pub(crate) fn index_exists(&self, uid: &str) -> Result<bool, String> {
        let url = format!("{}/indexes/{uid}", self.base_url);
        let resp = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .map_err(|e| format!("index_exists request failed: {e}"))?;

        let status = resp.status().as_u16();
        match status {
            200 => Ok(true),
            404 => Ok(false),
            _ => Err(format!("index_exists returned unexpected HTTP {status}")),
        }
    }

    /// Create a new index with the given primary key.
    ///
    /// Returns the task UID for polling via [`Self::wait_for_task`].
    /// Meilisearch index creation is asynchronous.
    ///
    /// # Errors
    ///
    /// Returns an error if the API call fails.
    pub(crate) fn create_index(&self, uid: &str, primary_key: &str) -> Result<u64, String> {
        let url = format!("{}/indexes", self.base_url);
        let body = serde_json::json!({
            "uid": uid,
            "primaryKey": primary_key,
        });

        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .send()
            .map_err(|e| format!("create_index request failed: {e}"))?;

        Self::extract_task_uid(resp, "create_index")
    }

    /// Update index settings (searchable, filterable, sortable attributes, etc.).
    ///
    /// Returns the task UID for polling via [`Self::wait_for_task`].
    ///
    /// # Errors
    ///
    /// Returns an error if the API call fails.
    pub(crate) fn update_settings(&self, uid: &str, settings: &serde_json::Value) -> Result<u64, String> {
        let url = format!("{}/indexes/{uid}/settings", self.base_url);

        let resp = self
            .http
            .patch(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .body(settings.to_string())
            .send()
            .map_err(|e| format!("update_settings request failed: {e}"))?;

        Self::extract_task_uid(resp, "update_settings")
    }

    /// Delete an index.
    ///
    /// Uses `DELETE /indexes/{uid}`. Returns the task UID.
    /// Returns `Ok` even if the index doesn't exist (idempotent).
    ///
    /// # Errors
    ///
    /// Returns an error if the API call fails.
    pub(crate) fn delete_index(&self, uid: &str) -> Result<u64, String> {
        let url = format!("{}/indexes/{uid}", self.base_url);

        let resp = self
            .http
            .delete(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .map_err(|e| format!("delete_index request failed: {e}"))?;

        let status = resp.status().as_u16();
        if status == 404 {
            // Index already gone — idempotent success
            return Ok(0);
        }

        Self::extract_task_uid(resp, "delete_index")
    }

    // -- Document operations -------------------------------------------------

    /// Add or update documents in an index (batch upsert).
    ///
    /// `documents` should be a JSON array of objects, each with a field
    /// matching the index's primary key.  Returns the task UID.
    ///
    /// # Errors
    ///
    /// Returns an error if the API call fails.
    pub(crate) fn add_documents(&self, uid: &str, documents: &serde_json::Value) -> Result<u64, String> {
        let url = format!("{}/indexes/{uid}/documents", self.base_url);

        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .body(documents.to_string())
            .send()
            .map_err(|e| format!("add_documents request failed: {e}"))?;

        Self::extract_task_uid(resp, "add_documents")
    }

    /// Delete documents matching a filter expression.
    ///
    /// Uses `POST /indexes/{uid}/documents/delete` with a filter body.
    /// Example filter: `"file_path = 'src/main.rs'"`.
    ///
    /// # Errors
    ///
    /// Returns an error if the API call fails.
    pub(crate) fn delete_documents_by_filter(&self, uid: &str, filter: &str) -> Result<u64, String> {
        let url = format!("{}/indexes/{uid}/documents/delete", self.base_url);
        let body = serde_json::json!({ "filter": filter });

        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .send()
            .map_err(|e| format!("delete_documents_by_filter request failed: {e}"))?;

        Self::extract_task_uid(resp, "delete_documents_by_filter")
    }

    // -- Stats ----------------------------------------------------------------

    /// Get index statistics (document count, indexing status).
    ///
    /// Uses `GET /indexes/{uid}/stats`.
    ///
    /// # Errors
    ///
    /// Returns an error on network failures or if the index doesn't exist.
    pub(crate) fn index_stats(&self, uid: &str) -> Result<(u64, bool), String> {
        let url = format!("{}/indexes/{uid}/stats", self.base_url);
        let resp = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .map_err(|e| format!("index_stats request failed: {e}"))?;

        let status = resp.status().as_u16();
        if status == 404 {
            return Ok((0, false));
        }
        if status != 200 {
            return Err(format!("index_stats returned unexpected HTTP {status}"));
        }

        let json: serde_json::Value = resp.json().map_err(|e| format!("index_stats response parse failed: {e}"))?;

        let doc_count = json.get("numberOfDocuments").and_then(serde_json::Value::as_u64).unwrap_or(0);
        let is_indexing = json.get("isIndexing").and_then(serde_json::Value::as_bool).unwrap_or(false);

        Ok((doc_count, is_indexing))
    }

    // -- Search ---------------------------------------------------------------

    /// Query a single index and return raw Meilisearch results.
    ///
    /// See [Meilisearch search API](https://docs.meilisearch.com/reference/api/search.html).
    ///
    /// # Errors
    ///
    /// Returns an error if the API call fails or the response cannot be parsed.
    pub(crate) fn search(&self, params: &SearchParams<'_>) -> Result<serde_json::Value, String> {
        let url = format!("{}/indexes/{}/search", self.base_url, params.uid);
        let mut body = serde_json::json!({
            "q": params.query,
            "limit": params.limit,
            "attributesToHighlight": ["content"],
            "attributesToCrop": ["content"],
            "cropLength": 60,
            "showMatchesPosition": false,
        });

        if let Some(f) = params.filter
            && let Some(obj) = body.as_object_mut()
        {
            let _prev = obj.insert("filter".to_string(), serde_json::Value::String(f.to_string()));
        }
        if let Some(s) = params.sort
            && let Some(obj) = body.as_object_mut()
        {
            let _prev = obj
                .insert("sort".to_string(), serde_json::Value::Array(vec![serde_json::Value::String(s.to_string())]));
        }

        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .send()
            .map_err(|e| format!("search request failed: {e}"))?;

        let json: serde_json::Value = resp.json().map_err(|e| format!("search response parse failed: {e}"))?;

        Ok(json)
    }

    // -- Task management -----------------------------------------------------

    /// Poll a task until it reaches a terminal state (`succeeded` or `failed`).
    ///
    /// Polls every 200ms for up to 30 seconds.
    ///
    /// # Errors
    ///
    /// Returns an error if the task fails, times out, or the API is unreachable.
    pub(crate) fn wait_for_task(&self, task_uid: u64) -> Result<(), String> {
        let timeout = Duration::from_secs(30);
        let interval = Duration::from_millis(200);
        let deadline = Instant::now().checked_add(timeout);

        loop {
            let url = format!("{}/tasks/{task_uid}", self.base_url);
            let resp = self
                .http
                .get(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
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

    // -- Helpers -------------------------------------------------------------

    /// Extract `taskUid` from an API response that returns a task.
    ///
    /// Meilisearch returns `202 Accepted` with `{ "taskUid": N, ... }` for
    /// asynchronous operations.
    fn extract_task_uid(resp: reqwest::blocking::Response, operation: &str) -> Result<u64, String> {
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
}
