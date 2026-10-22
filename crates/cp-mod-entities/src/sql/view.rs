//! Server-side view (sort, search, filters) over a `SqlTable` base query.
//!
//! The base `SELECT` is wrapped as a subquery and this module appends a
//! `WHERE … ORDER BY …` built from the view. Column names are checked against
//! the base query's own result columns and emitted as quoted identifiers;
//! every user value is a bound parameter, so neither can alter the statement.

use rusqlite::types::Value;

/// Most sort keys a view may carry.
const MAX_SORT: usize = 8;

/// Most filters a view may carry.
const MAX_FILTERS: usize = 16;

/// Comparison operator of a [`Filter`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[expect(
    clippy::exhaustive_enums,
    reason = "closed operator set mirrored 1:1 from the wire FilterOp by the agent binary"
)]
pub enum Op {
    /// Case-insensitive substring.
    Contains,
    /// Negated substring.
    NotContains,
    /// Case-insensitive prefix.
    StartsWith,
    /// Case-insensitive suffix.
    EndsWith,
    /// Equal.
    Eq,
    /// Not equal.
    Neq,
    /// Greater than.
    Gt,
    /// Greater than or equal.
    Gte,
    /// Less than.
    Lt,
    /// Less than or equal.
    Lte,
    /// Cell is `NULL`.
    IsNull,
    /// Cell is not `NULL`.
    NotNull,
}

/// One column filter.
#[derive(Clone, Debug, PartialEq, Eq)]
#[expect(
    clippy::exhaustive_structs,
    reason = "plain request tuple built field-by-field by the agent binary from the wire SqlFilter"
)]
pub struct Filter {
    /// Result column the filter applies to.
    pub column: String,
    /// Comparison operator.
    pub op: Op,
    /// Operand; ignored by [`Op::IsNull`] / [`Op::NotNull`].
    pub value: Option<String>,
}

/// One page request: the base `SELECT`, its view, and the row window.
#[derive(Clone, Copy, Debug)]
#[expect(
    clippy::exhaustive_structs,
    reason = "plain borrowed request bundle built by struct literal in the agent binary; keeps fetch under the argument budget"
)]
pub struct Request<'req> {
    /// The base `SELECT` to page through.
    pub sql: &'req str,
    /// Sort, search and filters over it.
    pub view: &'req View,
    /// Rows to skip.
    pub offset: u64,
    /// Rows in the page.
    pub limit: u32,
}

/// Sort keys, global search and filters applied on top of the base query.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[expect(
    clippy::exhaustive_structs,
    reason = "plain request tuple built field-by-field by the agent binary from the wire SqlView"
)]
pub struct View {
    /// `(column, descending)`, most significant first.
    pub sort: Vec<(String, bool)>,
    /// Case-insensitive substring matched against every column.
    pub search: Option<String>,
    /// Filters, all of which must hold.
    pub filters: Vec<Filter>,
}

/// A ` WHERE …` suffix, an ` ORDER BY …` suffix (each possibly empty) and the
/// values bound to the `WHERE` placeholders, in order.
pub(super) type Clauses = (String, String, Vec<Value>);

/// Build the clauses for `view` over a query whose result columns are `columns`.
///
/// # Errors
/// An unknown column, a missing operand or too many sort keys / filters.
pub(super) fn build(view: &View, columns: &[String]) -> Result<Clauses, String> {
    if view.sort.len() > MAX_SORT || view.filters.len() > MAX_FILTERS {
        return Err(format!("at most {MAX_SORT} sort keys and {MAX_FILTERS} filters"));
    }
    let mut params = Vec::new();
    let mut conds = Vec::new();
    if let Some(term) = view.search.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        let any: Vec<String> =
            columns.iter().map(|c| format!("CAST({} AS TEXT) LIKE ? ESCAPE '\\'", ident(c))).collect();
        for _ in columns {
            params.push(Value::Text(like_pattern(term, true, true)));
        }
        if !any.is_empty() {
            conds.push(format!("({})", any.join(" OR ")));
        }
    }
    for filter in &view.filters {
        conds.push(condition(filter, columns, &mut params)?);
    }
    let filter = if conds.is_empty() { String::new() } else { format!(" WHERE {}", conds.join(" AND ")) };
    let keys: Vec<String> = view
        .sort
        .iter()
        .map(|key| known(&key.0, columns).map(|c| format!("{c} {}", if key.1 { "DESC" } else { "ASC" })))
        .collect::<Result<_, _>>()?;
    let order = if keys.is_empty() { String::new() } else { format!(" ORDER BY {}", keys.join(", ")) };
    Ok((filter, order, params))
}

/// The SQL condition for one filter, pushing its operand onto `params`.
fn condition(filter: &Filter, columns: &[String], params: &mut Vec<Value>) -> Result<String, String> {
    let col = known(&filter.column, columns)?;
    let text = || format!("{col} IS NOT NULL AND CAST({col} AS TEXT)");
    match filter.op {
        Op::IsNull => return Ok(format!("{col} IS NULL")),
        Op::NotNull => return Ok(format!("{col} IS NOT NULL")),
        Op::Contains
        | Op::NotContains
        | Op::StartsWith
        | Op::EndsWith
        | Op::Eq
        | Op::Neq
        | Op::Gt
        | Op::Gte
        | Op::Lt
        | Op::Lte => {}
    }
    let Some(value) = filter.value.as_deref() else {
        return Err(format!("filter on `{}` needs a value", filter.column));
    };
    let (sql, param) = match filter.op {
        Op::Contains => (format!("({} LIKE ? ESCAPE '\\')", text()), Value::Text(like_pattern(value, true, true))),
        Op::NotContains => {
            (format!("NOT ({} LIKE ? ESCAPE '\\')", text()), Value::Text(like_pattern(value, true, true)))
        }
        Op::StartsWith => (format!("({} LIKE ? ESCAPE '\\')", text()), Value::Text(like_pattern(value, false, true))),
        Op::EndsWith => (format!("({} LIKE ? ESCAPE '\\')", text()), Value::Text(like_pattern(value, true, false))),
        Op::Eq => (format!("{col} = ?"), operand(value)),
        Op::Neq => (format!("{col} IS NOT ?"), operand(value)),
        Op::Gt => (format!("{col} > ?"), operand(value)),
        Op::Gte => (format!("{col} >= ?"), operand(value)),
        Op::Lt => (format!("{col} < ?"), operand(value)),
        Op::Lte => (format!("{col} <= ?"), operand(value)),
        Op::IsNull | Op::NotNull => return Ok(String::new()),
    };
    params.push(param);
    Ok(sql)
}

/// `column` as a quoted identifier, if it is one of the result `columns`.
fn known(column: &str, columns: &[String]) -> Result<String, String> {
    if columns.iter().any(|c| c == column) { Ok(ident(column)) } else { Err(format!("unknown column `{column}`")) }
}

/// Double-quoted `SQLite` identifier (embedded quotes doubled).
fn ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// `LIKE` pattern matching `term` literally, with `%` wildcards on the chosen sides.
fn like_pattern(term: &str, lead: bool, trail: bool) -> String {
    let escaped = term.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
    format!("{}{escaped}{}", if lead { "%" } else { "" }, if trail { "%" } else { "" })
}

/// Bind a comparison operand as a number when it parses as one, else as text,
/// so `n > 10` compares numerically against INTEGER/REAL columns.
fn operand(value: &str) -> Value {
    let trimmed = value.trim();
    trimmed
        .parse::<i64>()
        .map(Value::Integer)
        .or_else(|_| trimmed.parse::<f64>().map(Value::Real))
        .unwrap_or_else(|_| Value::Text(value.to_owned()))
}
