//! `POST /api/agent/{id}/conversations/search` — hybrid conversation search
//! (T671).
//!
//! The read-path twin of [`command`](super::super::command): where a command
//! travels to the agent to *mutate* state and comes back as a durable `Ack`,
//! a query travels the same UDS socket to *read* state and comes back with
//! data. The orchestrator deliberately does **not** talk to Meilisearch itself
//! — the agent owns its own index (its port, master key and project hash are
//! process-local), so the search is answered where that knowledge lives and
//! only the hits cross the wire.
//!
//! A lookup that fails inside the agent (search module down, backend
//! unreachable) comes back as an
//! [`Outcome::Error`](cp_wire::types::payload::query::Outcome::Error) and is
//! surfaced here as `502` with the agent's own reason, so the UI can say *why*
//! search is unavailable rather than showing an empty result set.

use std::sync::Mutex;

use cp_wire::types::payload::query::{ConvHit, Kind, Outcome, Query};
use serde::{Deserialize, Serialize};

use crate::channel::AgentHandle;
use crate::transport::rest::{Backend, HttpReply, resolve_entry};

/// Default hit count when the caller does not specify one.
const DEFAULT_LIMIT: u32 = 20;

/// Upper bound on hits — a palette renders a short list, and the cap keeps one
/// oversized request from making the agent embed-and-rank an unbounded set.
const MAX_LIMIT: u32 = 100;

/// Request body for a conversation search.
#[derive(Debug, Deserialize)]
struct SearchRequest {
    /// The user's raw search text. Meilisearch embeds it server-side, so no
    /// separate semantic query is needed.
    query: String,
    /// Maximum hits to return; clamped to [`MAX_LIMIT`], defaults to
    /// [`DEFAULT_LIMIT`].
    #[serde(default)]
    limit: Option<u32>,
    /// Optional single-thread scope. `None` searches every thread.
    #[serde(default)]
    thread_id: Option<String>,
}

/// Response body: the hits the agent returned, best-scored first.
#[derive(Debug, Serialize)]
struct SearchResponse {
    /// Matching conversation messages.
    hits: Vec<ConvHit>,
}

/// Handle `POST /api/agent/{id}/conversations/search`.
///
/// Returns `400` for a malformed body, `404` for an unknown agent, and `502`
/// when the agent is unreachable or reports the lookup failed.
pub(crate) fn search_conversations(state: &Mutex<Backend>, id: &str, body_bytes: &[u8]) -> HttpReply {
    let Ok(request) = serde_json::from_slice::<SearchRequest>(body_bytes) else {
        return HttpReply::error(400, "expected {\"query\":\"...\"}");
    };

    let entry = match resolve_entry(state, id) {
        Ok(entry) => entry,
        Err(reply) => return reply,
    };

    let limit = request.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let query = Query::new(
        format!("conv-search-{id}"),
        Kind::SearchConversations { query: request.query, limit, thread_id: request.thread_id },
    );

    // Blocking agent I/O — performed with no backend lock held (same discipline
    // as the command and body-hydrate handlers).
    match AgentHandle::from_entry(&entry).query(query) {
        Ok(response) => match response.result {
            Outcome::Hits { hits } => HttpReply::ok(&SearchResponse { hits }),
            Outcome::Error { reason } => {
                crate::oerr!("conversation search failed on agent {id}: {reason}");
                HttpReply::error(502, "conversation search unavailable")
            }
        },
        Err(e) => {
            crate::oerr!("conversation search transport error for agent {id}: {e:?}");
            HttpReply::error(502, "agent unreachable")
        }
    }
}
