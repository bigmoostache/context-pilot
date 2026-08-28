//! SSE upgrade — ticket redemption, access control, and producer hand-off.
//!
//! The `GET /api/stream` route's landing pad, split out of
//! [`transport`](super::super) so the acceptor/router in its `mod.rs` stays
//! within the workspace's 500-line budget. It lives under `stream/` rather than
//! beside the router because everything it does is stream-specific — redeem the
//! single-use ticket, authorise the subscriber against the agent's ACL, resolve
//! the agent's oplog directory, then spawn [`run_stream`](super::run_stream)
//! and hand the socket to the SSE body writer.
//!
//! Nothing here is reusable by the REST side: a caller that fails any of the
//! gates gets a JSON error through the parent's `respond_json`, and a caller
//! that passes never returns — the request is consumed by the streaming
//! response.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use tiny_http::Request;

use cp_wire::types::registry::Entry;

use super::super::{Backend, respond_json, rest};
use super::query::QueryParams;
use super::{run_stream, sse};

/// Redeem the ticket and stream an agent's deltas as SSE until disconnect.
pub(in crate::transport) fn handle_stream(request: Request, state: &Arc<Mutex<Backend>>, query: &str) {
    let params = QueryParams::parse(query);
    let Some(agent_id) = params.get("agent") else {
        respond_json(request, &rest::HttpReply { status: 400, body: "{\"error\":\"missing agent\"}".to_owned() });
        return;
    };
    let Some(token) = params.get("ticket") else {
        respond_json(request, &rest::HttpReply { status: 401, body: "{\"error\":\"missing ticket\"}".to_owned() });
        return;
    };

    // Single-use ticket redemption (Phase 7: now returns user identity).
    let ticket = state.lock().ok().and_then(|mut b| b.tickets.redeem(token));
    let Some(ticket) = ticket else {
        respond_json(request, &rest::HttpReply { status: 401, body: "{\"error\":\"invalid ticket\"}".to_owned() });
        return;
    };

    // Phase 7: per-agent ACL check on SSE connect. The ticket carries the
    // minting user's identity; when auth is enabled we verify they have access
    // to the requested agent before committing to a stream. System admins
    // bypass (FR-09). When auth is disabled (user_id is None) the check is
    // skipped entirely (NFR-09).
    if let Some(ref user_id) = ticket.user_id {
        let authorized = state.lock().ok().is_some_and(|b| {
            match b.auth.as_ref() {
                Some(auth) => match auth.get_user_by_id(user_id) {
                    Ok(Some(user)) => {
                        // Implicit access to all agents (manager+) bypasses the
                        // per-agent ACL (design §13.3); everyone else needs a row.
                        if user.can_manage_all_agents() {
                            true
                        } else {
                            auth.check_access(agent_id, user_id).is_ok_and(|role| role.is_some())
                        }
                    }
                    _ => false,
                },
                None => true, // auth not enabled — pass through
            }
        });
        if !authorized {
            respond_json(
                request,
                &rest::HttpReply { status: 403, body: "{\"error\":\"no access to this agent\"}".to_owned() },
            );
            return;
        }
    }

    // Resolve the agent's oplog directory before committing to a stream.
    let Some(entry) = load_entry(state, agent_id) else {
        respond_json(request, &rest::HttpReply { status: 404, body: "{\"error\":\"unknown agent\"}".to_owned() });
        return;
    };

    let last_rev =
        super::super::last_event_id(&request).or_else(|| params.get("last_rev").and_then(|s| s.parse().ok()));

    let (sink, body) = sse::channel();
    let producer_state = Arc::clone(state);
    let agent = agent_id.to_owned();
    let oplog_dir = PathBuf::from(&entry.oplog_path);
    let _producer = thread::spawn(move || {
        let ctx = super::StreamCtx { sink: &sink, state: &producer_state, agent_id: &agent, oplog_dir: &oplog_dir };
        run_stream(&ctx, last_rev);
    });

    sse::stream_to_client(request, body);
}

/// Load an agent's registry record from the backend's agents directory.
fn load_entry(state: &Arc<Mutex<Backend>>, id: &str) -> Option<Entry> {
    let dir = state.lock().ok()?.agents_dir.clone();
    let raw = std::fs::read(dir.join(format!("{id}.json"))).ok()?;
    serde_json::from_slice::<Entry>(&raw).ok()
}
