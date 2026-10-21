//! `POST /api/fleet/{id}/sql` — one page of a read-only entity SQL query, for
//! the web's `openui` `SqlTable` component.
//!
//! Rides the same read-path query channel as conversation search
//! ([`conversations`](super::conversations)): the agent owns its entity
//! database, so the page is computed there — on its bridge intake thread, off
//! the main loop — and only the rows cross the wire. The agent enforces
//! read-only, single-statement SQL and a per-query deadline; an agent-side
//! rejection (bad SQL, write attempt) surfaces as `400` with its reason so the
//! table can show *why*, while transport failures are `502`.

use std::sync::Mutex;

use cp_wire::types::payload::query::{Kind, Outcome, Query};
use serde::Deserialize;

use crate::channel::FleetHandle;
use crate::transport::rest::{Backend, HttpReply, resolve_entry};

/// Rows per page when the caller does not specify one.
const DEFAULT_LIMIT: u32 = 25;

/// Upper bound on rows per page — pagination is mandatory, not optional.
const MAX_LIMIT: u32 = 200;

/// Request body for one SQL page.
#[derive(Debug, Deserialize)]
struct SqlRequest {
    /// The `SELECT` to page through.
    sql: String,
    /// Rows to skip (default 0).
    #[serde(default)]
    offset: u64,
    /// Rows per page; clamped to [`MAX_LIMIT`], defaults to [`DEFAULT_LIMIT`].
    #[serde(default)]
    limit: Option<u32>,
}

/// Handle `POST /api/fleet/{id}/sql`.
///
/// `400` for a malformed body or SQL the agent refused, `404` for an unknown
/// agent, `502` when the agent is unreachable.
pub(crate) fn sql_page(state: &Mutex<Backend>, id: &str, body_bytes: &[u8]) -> HttpReply {
    let Ok(request) = serde_json::from_slice::<SqlRequest>(body_bytes) else {
        return HttpReply::error(400, "expected {\"sql\":\"SELECT ...\"}");
    };
    let entry = match resolve_entry(state, id) {
        Ok(entry) => entry,
        Err(reply) => return reply,
    };
    let limit = request.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let query = Query::new(format!("sql-page-{id}"), Kind::SqlPage { sql: request.sql, offset: request.offset, limit });

    // Blocking agent I/O with no backend lock held.
    match FleetHandle::from_entry(&entry).query(query) {
        Ok(response) => match response.result {
            Outcome::Rows { page } => HttpReply::ok(&page),
            Outcome::Error { reason } => HttpReply::error(400, &reason),
            Outcome::Hits { .. } => HttpReply::error(502, "agent answered with the wrong reply kind"),
        },
        Err(e) => {
            crate::oerr!("sql page transport error for agent {id}: {e:?}");
            HttpReply::error(502, "agent unreachable")
        }
    }
}
