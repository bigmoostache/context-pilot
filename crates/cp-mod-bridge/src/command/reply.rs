//! Read-path query answering for the command socket (T671).
//!
//! The agent's single UDS carries two inbound frame shapes (see
//! [`Inbound`](cp_wire::types::payload::query::Inbound)): a **command**, which
//! is authenticated, journaled, dedup'd and applied, and a **query**, which is
//! read-only. This module owns the query half.
//!
//! A query is deliberately *not* journaled and *not* dedup'd: it mutates
//! nothing, so replaying it is meaningless and a duplicate delivery is harmless.
//! It shares only the bearer check with the command path — the same `cap_token`
//! gate, applied before the payload is trusted.
//!
//! The actual lookup is **injected** by the caller as a closure rather than
//! implemented here. The bridge is durability infrastructure and must not know
//! about Meilisearch; the agent binary (which depends on both the bridge and
//! the search module) supplies a closure that already holds the resolved search
//! credentials, so answering a query needs no `State` borrow — load-bearing,
//! because the command-path `Intake` is `&mut`-borrowed out of that same
//! `State` while the connection is being served.

use cp_wire::framing;
use cp_wire::types::payload::query::{Frame as QueryFrame, Outcome, Query, Response};

/// The injected lookup that answers a [`Query`].
///
/// Supplied per-connection by the agent binary. Takes the decoded query and
/// returns its [`Outcome`] — hits on success, a reason string on failure (a
/// failed lookup is a graceful `Error` outcome, never a dropped connection).
pub type Responder<'src> = &'src dyn Fn(&Query) -> Outcome;

/// Authenticate a query frame and answer it, or reject it.
///
/// Mirrors the command path's bearer check ([`Intake::process`]): an empty or
/// mismatched `auth` is rejected *before* the payload is trusted. A rejection
/// is returned as a normal [`Response`] carrying [`Outcome::Error`], not as a
/// torn-down connection, so a mis-credentialed caller learns why.
///
/// [`Intake::process`]: super::Intake
pub(super) fn answer(frame: &QueryFrame, cap_token: &str, responder: Responder<'_>) -> Response {
    if frame.auth.is_empty() || frame.auth != cap_token {
        return Response::new(frame.query.id.clone(), Outcome::Error { reason: "bad bearer token".to_owned() });
    }
    Response::new(frame.query.id.clone(), responder(&frame.query))
}

/// Serialise a [`Response`] and wrap it in the shared length+CRC framing.
///
/// Mirrors the command path's `encode_ack`: a serialisation or framing failure
/// yields an empty buffer, which the peer reads as a closed/again — never a
/// panic on the agent's main loop.
pub(super) fn encode(response: &Response) -> Vec<u8> {
    serde_json::to_vec(response).ok().and_then(|p| framing::encode_raw(&p).ok()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cp_wire::types::payload::query::{ConvHit, HitParts, Kind};

    const TOKEN: &str = "cap-token-256bit";

    fn query_frame(auth: &str) -> QueryFrame {
        QueryFrame::new(
            auth.to_owned(),
            Query::new(
                "q-1".to_owned(),
                Kind::SearchConversations { query: "auth bug".to_owned(), limit: 5, thread_id: None },
            ),
        )
    }

    fn one_hit(_q: &Query) -> Outcome {
        Outcome::Hits {
            hits: vec![ConvHit::from_parts(&HitParts {
                thread_id: "T3",
                thread_name: "Auth",
                author: "user",
                text: "the token check",
                ts_ms: 7,
                score: 0.9,
            })],
        }
    }

    #[test]
    fn good_bearer_is_answered_by_the_responder() {
        let resp = answer(&query_frame(TOKEN), TOKEN, &one_hit);
        assert_eq!(resp.query_id, "q-1", "the response echoes the query id");
        let Outcome::Hits { hits } = resp.result else {
            panic!("responder returned hits");
        };
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn bad_bearer_is_rejected_without_calling_the_responder() {
        let never = |_q: &Query| -> Outcome {
            panic!("responder must not run for a bad bearer");
        };
        let resp = answer(&query_frame("wrong"), TOKEN, &never);
        let Outcome::Error { reason } = resp.result else {
            panic!("bad bearer must yield an error outcome");
        };
        assert_eq!(reason, "bad bearer token");
    }

    #[test]
    fn empty_bearer_is_rejected() {
        let never = |_q: &Query| -> Outcome {
            panic!("responder must not run for an empty bearer");
        };
        assert!(matches!(answer(&query_frame(""), TOKEN, &never).result, Outcome::Error { .. }));
    }

    #[test]
    fn encoded_response_round_trips_through_framing() {
        let bytes = encode(&answer(&query_frame(TOKEN), TOKEN, &one_hit));
        let (payload, _consumed) = framing::decode_raw(&bytes).expect("decode frame");
        let back: Response = serde_json::from_slice(payload).expect("decode response");
        assert_eq!(back.query_id, "q-1");
    }
}
