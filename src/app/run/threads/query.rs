//! Read-path query answering for the bridge command socket (T671).
//!
//! The bridge crate owns the wire framing for an inbound
//! [`Query`](cp_wire::types::payload::query::Query) but deliberately knows
//! nothing about Meilisearch — it is durability infrastructure, and the search
//! module sits above it. So the bridge takes the lookup as an injected closure
//! and this module supplies it: the agent binary is the one layer that depends
//! on **both** the bridge and the search module.
//!
//! Extracted from the sibling `bridge` module so each file stays under the
//! 500-line limit.

use cp_mod_search::meili::conversation_search::{SearchConvParams, search_conversations};
use cp_wire::types::payload::query::{Kind, Outcome, Query};

/// The resolved Meilisearch coordinates a query needs: `(port, master_key,
/// project_hash)`.
///
/// Resolved **before** the bridge borrows its state (see the borrow note in
/// [`accept_commands`](super::bridge)) so the responder closure is
/// self-contained and never touches `State` mid-connection.
pub(super) type SearchCreds = (u16, String, String);

/// Answer one read-path [`Query`] against this agent's own Meilisearch indexes.
///
/// `creds` is `None` when the search module never came up; that is reported as
/// a graceful [`Outcome::Error`] rather than a dropped connection, so the
/// caller renders "search unavailable" instead of seeing the agent go dark.
///
/// A [`Kind::Unknown`] query comes from a newer protocol version this agent
/// does not implement — also an `Error` outcome, never a hard failure (the N-1
/// forward-compat contract).
pub(super) fn answer(creds: Option<&SearchCreds>, query: &Query) -> Outcome {
    let Some(resolved) = creds else {
        return Outcome::Error { reason: "search unavailable on this agent".to_owned() };
    };

    // Destructure an OWNED clone of the kind rather than borrowing it: binding
    // the payload out of `&query.kind` would need `ref` bindings, which the
    // forbidden `clippy::ref_patterns` rejects (and match ergonomics on the
    // reference trips `pattern_type_mismatch`). The clone is one short query
    // string per request — a rounding error next to the HTTP round-trip below.
    let Kind::SearchConversations { query: text, limit, thread_id } = query.kind.clone() else {
        return Outcome::Error { reason: "unsupported query kind".to_owned() };
    };

    let params = SearchConvParams {
        port: resolved.0,
        master_key: &resolved.1,
        project_hash: &resolved.2,
        query: &text,
        limit,
        thread_id: thread_id.as_deref(),
    };
    match search_conversations(&params) {
        Ok(hits) => Outcome::Hits { hits },
        Err(reason) => Outcome::Error { reason },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn search_query() -> Query {
        Query::new("q-1".to_owned(), Kind::SearchConversations { query: "auth".to_owned(), limit: 5, thread_id: None })
    }

    /// The failure reason of an [`Outcome`], or `None` when it carried hits.
    ///
    /// A helper (rather than a `let-else` + `unreachable!` at each site) because
    /// the bin crate forbids `clippy::unreachable`.
    fn error_reason(outcome: Outcome) -> Option<String> {
        match outcome {
            Outcome::Error { reason } => Some(reason),
            Outcome::Hits { .. } => None,
        }
    }

    #[test]
    fn missing_creds_yield_a_graceful_error() {
        assert_eq!(error_reason(answer(None, &search_query())).as_deref(), Some("search unavailable on this agent"));
    }

    #[test]
    fn unknown_kind_yields_a_graceful_error() {
        // An N-1 agent folds a future query kind to Unknown; it must answer with
        // an error rather than failing the connection.
        let creds: SearchCreds = (7700, "k".to_owned(), "abcd1234".to_owned());
        let unknown = Query::new("q-2".to_owned(), Kind::Unknown);
        assert_eq!(error_reason(answer(Some(&creds), &unknown)).as_deref(), Some("unsupported query kind"));
    }
}
