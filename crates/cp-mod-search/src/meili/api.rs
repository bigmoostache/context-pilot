//! Meilisearch HTTP API client.
//!
//! Thin wrapper around the Meilisearch REST API, providing typed methods
//! for index management, document operations, and search queries.
//! Methods are added incrementally as each implementation phase lands.

use std::time::Duration;

/// HTTP client for a running Meilisearch server.
/// Created on-the-fly when needed (not stored in `SearchState`).
#[derive(Debug)]
pub struct MeiliClient {
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
    /// Semantic search ratio (0.0 = keyword only, 1.0 = semantic only).
    /// When `Some`, enables hybrid search with the given ratio.
    pub semantic_ratio: Option<f64>,
}

impl MeiliClient {
    /// Create a new client pointing at a local Meilisearch server.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be built.
    pub fn new(port: u16, api_key: &str) -> Result<Self, String> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| format!("Cannot create HTTP client: {e}"))?;

        Ok(Self { base_url: format!("http://127.0.0.1:{port}"), api_key: api_key.to_string(), http })
    }

    /// Base URL for the Meilisearch server.
    pub(super) fn url(&self) -> &str {
        &self.base_url
    }

    /// Bearer token for authentication.
    pub(super) fn key(&self) -> &str {
        &self.api_key
    }

    /// Reusable HTTP client reference.
    pub(super) const fn client(&self) -> &reqwest::blocking::Client {
        &self.http
    }

    // -- Index operations ----------------------------------------------------

    /// Configure embedder settings for an index.
    ///
    /// Uses `PATCH /indexes/{uid}/settings/embedders` to set up the embedding
    /// source (e.g. `huggingFace`). Returns the task UID for polling.
    /// Meilisearch generates embeddings as a background task after this call.
    ///
    /// # Errors
    ///
    /// Returns an error if the API call fails.
    pub fn update_embedder_settings(&self, uid: &str, settings: &serde_json::Value) -> Result<u64, String> {
        let url = format!("{}/indexes/{uid}/settings/embedders", self.base_url);

        let resp = self
            .http
            .patch(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .body(settings.to_string())
            .send()
            .map_err(|e| format!("update_embedder_settings request failed: {e}"))?;

        super::tasks::extract_task_uid(resp, "update_embedder_settings")
    }

    /// Read the current embedder settings for an index.
    ///
    /// Returns the raw JSON value from `GET /indexes/{uid}/settings/embedders`.
    /// Returns an empty object if no embedders are configured or on any error.
    ///
    /// # Errors
    ///
    /// Returns an error on network failures.
    pub fn get_embedder_settings(&self, uid: &str) -> Result<serde_json::Value, String> {
        let url = format!("{}/indexes/{uid}/settings/embedders", self.base_url);
        let resp = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .map_err(|e| format!("get_embedder_settings request failed: {e}"))?;

        let status = resp.status().as_u16();
        if status == 200 {
            resp.json().map_err(|e| format!("get_embedder_settings parse failed: {e}"))
        } else {
            // Return empty object on any error (feature not enabled, index not found, etc.)
            Ok(serde_json::Value::Object(serde_json::Map::new()))
        }
    }

    /// Check whether an index exists.
    ///
    /// Uses `GET /indexes/{uid}` — returns `true` on 200, `false` on 404.
    ///
    /// # Errors
    ///
    /// Returns an error on network failures or unexpected status codes.
    pub fn index_exists(&self, uid: &str) -> Result<bool, String> {
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
    /// Returns the task UID for polling via [`super::tasks::wait_for_task`].
    /// Meilisearch index creation is asynchronous.
    ///
    /// # Errors
    ///
    /// Returns an error if the API call fails.
    pub fn create_index(&self, uid: &str, primary_key: &str) -> Result<u64, String> {
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

        super::tasks::extract_task_uid(resp, "create_index")
    }

    /// Update index settings (searchable, filterable, sortable attributes, etc.).
    ///
    /// Returns the task UID for polling via [`super::tasks::wait_for_task`].
    ///
    /// # Errors
    ///
    /// Returns an error if the API call fails.
    pub fn update_settings(&self, uid: &str, settings: &serde_json::Value) -> Result<u64, String> {
        let url = format!("{}/indexes/{uid}/settings", self.base_url);

        let resp = self
            .http
            .patch(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .body(settings.to_string())
            .send()
            .map_err(|e| format!("update_settings request failed: {e}"))?;

        super::tasks::extract_task_uid(resp, "update_settings")
    }

    /// Delete an index. Uses `DELETE /indexes/{uid}`. Returns the task UID.
    /// Returns `Ok` even if the index doesn't exist (idempotent).
    ///
    /// # Errors
    ///
    /// Returns an error if the API call fails.
    pub fn delete_index(&self, uid: &str) -> Result<u64, String> {
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

        super::tasks::extract_task_uid(resp, "delete_index")
    }

    // -- Document operations -------------------------------------------------

    /// Add or update documents in an index (batch upsert).
    /// `documents` is a JSON array with the index's primary key field. Returns the task UID.
    ///
    /// # Errors
    ///
    /// Returns an error if the API call fails.
    pub fn add_documents(&self, uid: &str, documents: &serde_json::Value) -> Result<u64, String> {
        let url = format!("{}/indexes/{uid}/documents", self.base_url);

        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .body(documents.to_string())
            .send()
            .map_err(|e| format!("add_documents request failed: {e}"))?;

        super::tasks::extract_task_uid(resp, "add_documents")
    }

    /// Delete documents matching a filter expression.
    ///
    /// Uses `POST /indexes/{uid}/documents/delete` with a filter body.
    /// Example filter: `"file_path = 'src/main.rs'"`.
    ///
    /// # Errors
    ///
    /// Returns an error if the API call fails.
    pub fn delete_documents_by_filter(&self, uid: &str, filter: &str) -> Result<u64, String> {
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

        super::tasks::extract_task_uid(resp, "delete_documents_by_filter")
    }

    // -- Stats ----------------------------------------------------------------

    /// Get global statistics across all indexes (`GET /stats`).
    ///
    /// Returns raw JSON with `databaseSize`, `usedDatabaseSize`, per-index stats.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub fn global_stats(&self) -> Result<serde_json::Value, String> {
        let url = format!("{}/stats", self.base_url);
        let resp = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .map_err(|e| format!("global_stats request failed: {e}"))?;

        resp.json().map_err(|e| format!("global_stats response parse failed: {e}"))
    }

    /// Get index statistics — document count and indexing status (`GET /indexes/{uid}/stats`).
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub fn index_stats(&self, uid: &str) -> Result<(u64, bool), String> {
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

    /// Get the Meilisearch server version string (`GET /version` → `pkgVersion`).
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub fn version(&self) -> Result<String, String> {
        let url = format!("{}/version", self.base_url);
        let resp = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .map_err(|e| format!("version request failed: {e}"))?;

        let json: serde_json::Value = resp.json().map_err(|e| format!("version parse failed: {e}"))?;
        Ok(json.get("pkgVersion").and_then(serde_json::Value::as_str).unwrap_or("unknown").to_string())
    }

    /// Get recent tasks filtered to specific index UIDs (`GET /tasks?limit=N&indexUids=...`).
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub fn recent_tasks(&self, limit: u32, index_uids: &[&str]) -> Result<serde_json::Value, String> {
        let uids_param = index_uids.join(",");
        let url = format!("{}/tasks?limit={limit}&indexUids={uids_param}", self.base_url);
        let resp = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .map_err(|e| format!("recent_tasks request failed: {e}"))?;

        let json: serde_json::Value = resp.json().map_err(|e| format!("recent_tasks parse failed: {e}"))?;
        Ok(json.get("results").cloned().unwrap_or_else(|| serde_json::Value::Array(Vec::new())))
    }

    // -- Search ---------------------------------------------------------------

    /// Build the JSON body for a single search query (shared by `search` and `multi_search`).
    fn build_search_body(params: &SearchParams<'_>) -> serde_json::Value {
        let mut body = serde_json::json!({
            "q": params.query,
            "limit": params.limit,
            "showRankingScore": true,
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
        if let Some(ratio) = params.semantic_ratio
            && let Some(obj) = body.as_object_mut()
        {
            let _prev = obj.insert(
                "hybrid".to_string(),
                serde_json::json!({
                    "semanticRatio": ratio,
                    "embedder": "default"
                }),
            );
        }

        body
    }

    /// Query a single index and return raw Meilisearch results.
    ///
    /// See [Meilisearch search API](https://docs.meilisearch.com/reference/api/search.html).
    ///
    /// # Errors
    ///
    /// Returns an error if the API call fails or the response cannot be parsed.
    pub(crate) fn search(&self, params: &SearchParams<'_>) -> Result<serde_json::Value, String> {
        let url = format!("{}/indexes/{}/search", self.base_url, params.uid);
        let body = Self::build_search_body(params);

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

    /// Run multiple search queries in a single request (`/multi-search`).
    ///
    /// Each query targets a specific index.  Returns one result set per query
    /// in the same order they were submitted.
    ///
    /// Used when `semantic_query` is provided: one pure-keyword query with the
    /// user's `query`, one pure-semantic query with the `semantic_query`.
    ///
    /// # Errors
    ///
    /// Returns an error if the API call fails or the response cannot be parsed.
    pub(crate) fn multi_search(&self, queries: &[SearchParams<'_>]) -> Result<Vec<serde_json::Value>, String> {
        let url = format!("{}/multi-search", self.base_url);

        let query_bodies: Vec<serde_json::Value> = queries
            .iter()
            .map(|params| {
                let mut body = Self::build_search_body(params);
                if let Some(obj) = body.as_object_mut() {
                    let _prev = obj.insert("indexUid".to_string(), serde_json::Value::String(params.uid.to_string()));
                }
                body
            })
            .collect();

        let envelope = serde_json::json!({ "queries": query_bodies });

        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .body(envelope.to_string())
            .send()
            .map_err(|e| format!("multi_search request failed: {e}"))?;

        let json: serde_json::Value = resp.json().map_err(|e| format!("multi_search response parse failed: {e}"))?;

        let results = json.get("results").and_then(serde_json::Value::as_array).cloned().unwrap_or_default();

        Ok(results)
    }

    /// Query facet distribution for one or more attributes.
    ///
    /// Sends an empty-query search with `facets` to get value counts.
    /// Used to populate overlay metrics (extension breakdown, chunk types)
    /// without re-indexing.
    ///
    /// Returns the raw `facetDistribution` object from the response.
    ///
    /// # Errors
    ///
    /// Returns an error if the API call fails or the response cannot be parsed.
    pub fn facet_distribution(&self, uid: &str, facets: &[&str]) -> Result<serde_json::Value, String> {
        let url = format!("{}/indexes/{uid}/search", self.base_url);
        let facet_arr: Vec<serde_json::Value> =
            facets.iter().map(|f| serde_json::Value::String((*f).to_string())).collect();

        let body = serde_json::json!({
            "q": "",
            "limit": 0,
            "facets": facet_arr,
        });

        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .send()
            .map_err(|e| format!("facet_distribution request failed: {e}"))?;

        let json: serde_json::Value =
            resp.json().map_err(|e| format!("facet_distribution response parse failed: {e}"))?;

        Ok(json.get("facetDistribution").cloned().unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new())))
    }

    // -- Helpers -------------------------------------------------------------
    // NOTE: `wait_for_task` and `extract_task_uid` live in `tasks.rs`
    // as free functions to avoid the `multiple_inherent_impl` lint.
}
