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

/// Max characters of a message returned in a [`ConvHit`]. A search hit is a
/// preview line, not the whole message, so the stored text is snippet-capped
/// here at the source: it keeps the reply small (a whole thread's worth of
/// multi-KB messages otherwise blows the backend's framed-read bound) and
/// matches what the palette actually renders — one truncated line. UTF-8-safe
/// (cut on a `char` boundary via `chars().take`).
const HIT_SNIPPET_CHARS: usize = 280;

/// Candidate over-fetch multiplier for per-thread dedupe. The index holds ONE
/// doc per message, so a hot thread would otherwise flood the result list with
/// its own name repeated. We fetch `limit * OVERFETCH` message-hits from Meili,
/// then collapse to the best-scored message per thread (see
/// [`dedupe_by_thread`]) so the caller gets `limit` DISTINCT conversations —
/// the "find the conversation" behaviour the palette wants. Over-fetching is
/// what makes N distinct threads reachable even when the top hits cluster in a
/// few threads.
const CANDIDATE_OVERFETCH: u32 = 8;

/// Hard cap on candidate fetch, so a huge `limit` can't ask Meili for an
/// unbounded page (Meili's own default max is 1000; we stay well under).
const MAX_CANDIDATES: u32 = 200;

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

    // Over-fetch message-hits, then collapse to one (best-scored) per thread so
    // the caller gets `limit` DISTINCT conversations, not every message of a hot
    // thread. Meili has no native group-by, so we dedupe in Rust (M141 — logic
    // in the backend, not the palette).
    let candidates = p.limit.saturating_mul(CANDIDATE_OVERFETCH).min(MAX_CANDIDATES).max(p.limit);

    let params = SearchParams {
        uid: &uid,
        query: p.query,
        filter: filter.as_deref(),
        sort: None,
        limit: candidates,
        semantic_ratio: Some(CONVERSATION_SEMANTIC_RATIO),
    };

    let json = client.search(&params)?;
    let hits = parse_hits(&json);
    // `usize::try_from` (not `as`) — clippy::as_conversions is forbid; on the
    // 16/32/64-bit targets we ship, a u32 limit always fits usize, so the
    // error arm is unreachable and degrades to "no truncation".
    let keep = usize::try_from(p.limit).unwrap_or(usize::MAX);
    Ok(dedupe_by_thread(hits, keep))
}

/// Collapse message-hits to one per thread, keeping the FIRST occurrence of each
/// `thread_id` and truncating to `limit` threads.
///
/// Meili returns hits already sorted by descending `_rankingScore`, so the first
/// hit seen for a thread is its best-scoring message — the natural representative
/// for a "find the conversation" result. Insertion order is preserved (highest
/// overall score first), so the returned list stays score-ranked across threads.
fn dedupe_by_thread(hits: Vec<ConvHit>, limit: usize) -> Vec<ConvHit> {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out: Vec<ConvHit> = Vec::new();
    for hit in hits {
        if out.len() >= limit {
            break;
        }
        if seen.insert(hit.thread_id.clone()) {
            out.push(hit);
        }
    }
    out
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
    // Snippet-cap the message text (UTF-8-safe) — a hit is a preview, not the
    // whole message; keeps the framed reply small (see HIT_SNIPPET_CHARS).
    let text: String = str_field("text").chars().take(HIT_SNIPPET_CHARS).collect();
    ConvHit::from_parts(&cp_wire::types::payload::query::HitParts {
        thread_id: &str_field("thread_id"),
        thread_name: &str_field("thread_name"),
        author: &str_field("author"),
        text: &text,
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

    /// Build a bare [`ConvHit`] carrying just a `thread_id` + `text` marker for
    /// the dedupe tests.
    fn hit(thread_id: &str, text: &str) -> ConvHit {
        ConvHit::from_parts(&cp_wire::types::payload::query::HitParts {
            thread_id,
            thread_name: "",
            author: "",
            text,
            ts_ms: 0,
            score: 0.0,
        })
    }

    #[test]
    fn dedupe_keeps_first_per_thread() {
        // T1 appears twice: only its FIRST (best-scored) message survives; order
        // across threads is preserved.
        let hits = vec![hit("T1", "best"), hit("T2", "a"), hit("T1", "worse"), hit("T3", "b")];
        let out = dedupe_by_thread(hits, 10);
        let ids: Vec<&str> = out.iter().map(|h| h.thread_id.as_str()).collect();
        assert_eq!(ids, vec!["T1", "T2", "T3"]);
        assert_eq!(out[0].text, "best", "kept the first (best-scored) T1 message");
    }

    #[test]
    fn dedupe_honors_limit() {
        let hits = vec![hit("T1", ""), hit("T2", ""), hit("T3", "")];
        assert_eq!(dedupe_by_thread(hits, 2).len(), 2);
    }
}
