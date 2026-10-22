//! Read-path query answering for the bridge command socket (T671).
//!
//! The bridge crate owns the wire framing for an inbound
//! [`Query`](cp_wire::types::payload::query::Query) but deliberately knows
//! nothing about Meilisearch or the entity database — it is durability
//! infrastructure, and those modules sit above it. So the bridge takes the
//! lookup as an injected closure and this module supplies it: the agent binary
//! is the one layer that depends on the bridge AND the search/entities modules.
//!
//! Queries are answered on the bridge intake thread, never the main loop, so a
//! slow page (bounded by a deadline in `cp_mod_entities::sql::page::fetch`) cannot
//! stall the agent.
//!
//! Extracted from the sibling `bridge` module so each file stays under the
//! 500-line limit.

use std::path::PathBuf;

use cp_mod_entities::sql::view::{Filter, Op, Request, View};
use cp_mod_search::meili::conversation_search::{SearchConvParams, search_conversations};
use cp_wire::types::payload::query::{FilterOp, Kind, Outcome, Query, SqlPage, SqlView};

/// The resolved Meilisearch coordinates a query needs: `(port, master_key,
/// project_hash)`.
pub(super) type SearchCreds = (u16, String, String);

/// Everything the intake thread needs to answer queries without `State`.
///
/// Copied out of `State` into the responder closure (see
/// `sync_query_responder` in [`super::bridge`]); each half is `None` when its
/// module never came up, which answers as a graceful [`Outcome::Error`].
#[derive(Clone, Debug, Default)]
pub(super) struct QueryCtx {
    /// Meilisearch coordinates for conversation search.
    search: Option<SearchCreds>,
    /// The agent's entity database, for `SqlTable` pages.
    entities_db: Option<PathBuf>,
}

impl QueryCtx {
    /// Bundle the per-kind lookup context the responder closure owns.
    pub(super) const fn new(search: Option<SearchCreds>, entities_db: Option<PathBuf>) -> Self {
        Self { search, entities_db }
    }
}

/// Answer one read-path [`Query`].
///
/// A [`Kind::Unknown`] query comes from a newer protocol version this agent
/// does not implement — an `Error` outcome, never a hard failure (the N-1
/// forward-compat contract).
pub(super) fn answer(ctx: &QueryCtx, query: &Query) -> Outcome {
    // Destructure an OWNED clone of the kind: binding out of `&query.kind`
    // would need `ref` bindings, which the forbidden `clippy::ref_patterns`
    // rejects. One short string per request — negligible.
    match query.kind.clone() {
        Kind::SearchConversations { query: text, limit, agent_id } => {
            let Some(resolved) = ctx.search.as_ref() else {
                return Outcome::Error { reason: "search unavailable on this agent".to_owned() };
            };
            let params = SearchConvParams {
                port: resolved.0,
                master_key: &resolved.1,
                project_hash: &resolved.2,
                query: &text,
                limit,
                agent_id: agent_id.as_deref(),
            };
            match search_conversations(&params) {
                Ok(hits) => Outcome::Hits { hits },
                Err(reason) => Outcome::Error { reason },
            }
        }
        Kind::SqlPage { sql, offset, limit, view: wire_view } => {
            let Some(db) = ctx.entities_db.as_ref() else {
                return Outcome::Error { reason: "entity database unavailable on this agent".to_owned() };
            };
            let view = match to_view(wire_view) {
                Ok(mapped) => mapped,
                Err(reason) => return Outcome::Error { reason },
            };
            match cp_mod_entities::sql::page::fetch(db, &Request { sql: &sql, view: &view, offset, limit }) {
                Ok(page) => Outcome::Rows { page: SqlPage::new(page.columns, page.rows, page.total, offset) },
                Err(reason) => Outcome::Error { reason },
            }
        }
        Kind::Unknown => Outcome::Error { reason: "unsupported query kind".to_owned() },
    }
}

/// Map the wire view onto the entities crate's own types.
///
/// # Errors
/// A filter operator from a newer protocol version.
fn to_view(wire: SqlView) -> Result<View, String> {
    let filters = wire
        .filters
        .into_iter()
        .map(|f| {
            let op = match f.op {
                FilterOp::Contains => Op::Contains,
                FilterOp::NotContains => Op::NotContains,
                FilterOp::StartsWith => Op::StartsWith,
                FilterOp::EndsWith => Op::EndsWith,
                FilterOp::Eq => Op::Eq,
                FilterOp::Neq => Op::Neq,
                FilterOp::Gt => Op::Gt,
                FilterOp::Gte => Op::Gte,
                FilterOp::Lt => Op::Lt,
                FilterOp::Lte => Op::Lte,
                FilterOp::IsNull => Op::IsNull,
                FilterOp::NotNull => Op::NotNull,
                FilterOp::Unknown => return Err("unsupported filter operator".to_owned()),
            };
            Ok(Filter { column: f.column, op, value: f.value })
        })
        .collect::<Result<_, _>>()?;
    Ok(View { sort: wire.sort.into_iter().map(|k| (k.column, k.desc)).collect(), search: wire.search, filters })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn search_query() -> Query {
        Query::new("q-1".to_owned(), Kind::SearchConversations { query: "auth".to_owned(), limit: 5, agent_id: None })
    }

    /// The failure reason of an [`Outcome`], or `None` when it carried data.
    ///
    /// A helper (rather than a `let-else` + `unreachable!` at each site) because
    /// the bin crate forbids `clippy::unreachable`.
    fn error_reason(outcome: Outcome) -> Option<String> {
        match outcome {
            Outcome::Error { reason } => Some(reason),
            Outcome::Hits { .. } | Outcome::Rows { .. } => None,
        }
    }

    #[test]
    fn missing_creds_yield_a_graceful_error() {
        let ctx = QueryCtx::default();
        assert_eq!(error_reason(answer(&ctx, &search_query())).as_deref(), Some("search unavailable on this agent"));
    }

    #[test]
    fn missing_entities_db_yields_a_graceful_error() {
        let q = Query::new(
            "q-3".to_owned(),
            Kind::SqlPage { sql: "SELECT 1".to_owned(), offset: 0, limit: 10, view: SqlView::default() },
        );
        assert_eq!(
            error_reason(answer(&QueryCtx::default(), &q)).as_deref(),
            Some("entity database unavailable on this agent")
        );
    }

    #[test]
    fn unknown_kind_yields_a_graceful_error() {
        // An N-1 agent folds a future query kind to Unknown; it must answer with
        // an error rather than failing the connection.
        let ctx = QueryCtx { search: Some((7700, "k".to_owned(), "abcd1234".to_owned())), entities_db: None };
        let unknown = Query::new("q-2".to_owned(), Kind::Unknown);
        assert_eq!(error_reason(answer(&ctx, &unknown)).as_deref(), Some("unsupported query kind"));
    }
}
