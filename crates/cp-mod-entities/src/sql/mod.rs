//! SQL preprocessing and error enrichment.
//!
//! Groups the statement parsing helpers (classification + splitting) with the
//! error-enrichment layer that decorates `SQLite` errors with schema context.

/// SQL error enrichment: fuzzy suggestions and schema context.
pub(crate) mod errors;
/// Paged read-only queries for the web `SqlTable` (bridge intake thread).
pub mod page;
/// SQL statement parsing: classification + UTF-8-safe statement splitting.
pub(crate) mod parse;
/// Sort/search/filter clauses layered over a `SqlTable` base query.
pub mod view;
