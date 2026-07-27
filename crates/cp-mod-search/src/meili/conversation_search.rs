//! Read-path hybrid search over the conversations index (T671).
//!
//! The agent answers a backend `SearchConversations` query by running a
//! **hybrid** (keyword + semantic) Meilisearch query over its own
//! `cp_{hash}_conversations` index and mapping each hit into a transport-ready
//! [`ConvHit`]. This is deliberately **stateless** — it takes the resolved
//! Meilisearch credentials by value rather than reaching into `State` — so the
//! agent's command-socket accept loop can call it from inside a closure without
//! holding a `State` borrow across the (blocking) HTTP round-trip (the
//! command-path `Intake` is `&mut`-borrowed out of the same `State` at that
//! point, so a second `State` borrow for search would not type-check).
//!
//! Meilisearch embeds the query server-side for the semantic leg, so the caller
//! passes only the raw user text — no fabricated semantic query is needed.

use cp_wire::types::payload::query::ConvHit;

use crate::meili::api::{MeiliClient, SearchParams};

/// Blend ratio for the conversations hybrid query: an even keyword/semantic
/// split. Exact-term recall (names, ids the user remembers) and paraphrase
/// recall (semantic) both matter for "find where we talked about X", so neither
/// leg dominates.
const CONVERSATION_SEMANTIC_RATIO: f64 = 0.5;

/// Resolved inputs for [`search_conversations`].
///
/// A single params struct (rather than six positional arguments) keeps the
/// stateless entry point within the argument-count budget while the caller
/// still passes owned credentials by value (no `State` borrow — see the module
/// docs). Deliberately **exhaustive**: the agent's accept-loop closure builds
/// it by struct literal, and a constructor would take six positional arguments,
/// re-tripping `too_many_arguments`.
#[derive(Debug)]
#[expect(
    clippy::exhaustive_structs,
    reason = "resolved search inputs built by struct literal at the single agent accept-loop call site; a constructor would take six positional arguments and re-trip too_many_arguments, so #[non_exhaustive] is impossible"
)]
pub struct SearchConvParams<'src> {
    /// Meilisearch server port.
    pub port: u16,
    /// Meilisearch master key.
    pub master_key: &'src str,
    /// 8-char project hash — selects the `cp_{hash}_conversations` index.
    pub project_hash: &'src str,
    /// Free-text query (Meilisearch embeds it server-side for the semantic leg).
    pub query: &'src str,
    /// Maximum number of hits to return.
    pub limit: u32,
    /// Optional single-thread scope (`thread_id` filter); `None` searches all.
    pub thread_id: Option<&'src str>,
}

/// Run a hybrid keyword+semantic search over the conversations index and return
/// the hits as transport-ready [`ConvHit`]s (best-scored first).
///
/// `thread_id` scopes the search to a single thread when `Some` (a Meilisearch
/// `thread_id = '…'` filter); `None` searches every thread. The index name is
/// derived from `project_hash` exactly as the bootstrap/reconcile paths derive
/// it (`cp_{hash}_conversations`), so this queries the same index the
/// reconciler fills.
///
/// # Errors
///
/// Returns an error string if the Meilisearch client cannot be built or the
/// search request fails — the caller surfaces it as a graceful
/// [`Outcome::Error`](cp_wire::types::payload::query::Outcome::Error) rather
/// than tearing down the connection.
pub fn search_conversations(p: &SearchConvParams<'_>) -> Result<Vec<ConvHit>, String> {
    let client = MeiliClient::new(p.port, p.master_key)?;
    let uid = format!("cp_{}_conversations", p.project_hash);

    // Single-quote-escape a caller-supplied thread id before splicing it into
    // the Meilisearch filter expression, matching the file-indexer's own
    // escaping (`delete_one_file`) so a stray quote can't malform the filter.
    let filter = p.thread_id.map(|t| format!("thread_id = '{}'", t.replace('\'', "\\'")));

    let params = SearchParams {
        uid: &uid,
        query: p.query,
        filter: filter.as_deref(),
        sort: None,
        limit: p.limit,
        semantic_ratio: Some(CONVERSATION_SEMANTIC_RATIO),
    };

    let json = client.search(&params)?;
    Ok(parse_hits(&json))
}

/// Map a raw Meilisearch search response into [`ConvHit`]s.
///
/// Each element of the response `hits` array is a stored conversation document
/// plus a `_rankingScore` (search is issued with `showRankingScore`). Missing
/// fields degrade to empty/zero rather than dropping the hit — a partial hit is
/// still a navigable result.
fn parse_hits(json: &serde_json::Value) -> Vec<ConvHit> {
    let Some(hits) = json.get("hits").and_then(serde_json::Value::as_array) else {
        return Vec::new();
    };
    hits.iter().map(hit_from_doc).collect()
}

/// Build one [`ConvHit`] from a single Meilisearch hit document.
fn hit_from_doc(doc: &serde_json::Value) -> ConvHit {
    let str_field = |k: &str| doc.get(k).and_then(serde_json::Value::as_str).unwrap_or_default().to_owned();
    ConvHit::from_parts(&cp_wire::types::payload::query::HitParts {
        thread_id: &str_field("thread_id"),
        thread_name: &str_field("thread_name"),
        author: &str_field("author"),
        text: &str_field("text"),
        ts_ms: doc.get("ts_ms").and_then(serde_json::Value::as_u64).unwrap_or(0),
        score: doc.get("_rankingScore").and_then(serde_json::Value::as_f64).unwrap_or(0.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The single hit of a one-element parse, or a test failure.
    fn only_hit(json: &serde_json::Value) -> ConvHit {
        let hits = parse_hits(json);
        assert_eq!(hits.len(), 1, "expected exactly one parsed hit");
        let Some(hit) = hits.into_iter().next() else {
            unreachable!("len checked above");
        };
        hit
    }

    #[test]
    fn parse_hits_maps_fields_and_score() {
        let json = serde_json::json!({
            "hits": [ {
                "id": "T3-2",
                "thread_id": "T3",
                "thread_name": "Auth",
                "author": "user",
                "text": "the token check",
                "ts_ms": 42u64,
                "_rankingScore": 0.91f64
            } ]
        });
        let h = only_hit(&json);
        assert_eq!(h.thread_id, "T3");
        assert_eq!(h.thread_name, "Auth");
        assert_eq!(h.author, "user");
        assert_eq!(h.text, "the token check");
        assert_eq!(h.ts_ms, 42);
        assert!((h.score - 0.91).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_hits_empty_when_no_hits_key() {
        assert!(parse_hits(&serde_json::json!({})).is_empty());
    }

    #[test]
    fn parse_hits_tolerates_missing_fields() {
        let h = only_hit(&serde_json::json!({ "hits": [ { "thread_id": "T1" } ] }));
        assert_eq!(h.thread_id, "T1");
        assert_eq!(h.text, "");
        assert_eq!(h.ts_ms, 0);
    }
}
