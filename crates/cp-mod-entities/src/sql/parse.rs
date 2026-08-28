//! SQL statement parsing: classification + UTF-8-safe statement splitting.
//!
//! These helpers run *before* the SQL reaches `rusqlite`. The splitter must be
//! byte-offset correct: slicing the original `&str` with character indices
//! silently truncates statements that contain multi-byte UTF-8 characters
//! (issue #113), so all slicing here is driven by [`str::char_indices`].

// =============================================================================
// SQL classification
// =============================================================================

/// Broad category of a SQL statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SqlKind {
    /// `SELECT`, `EXPLAIN`, `PRAGMA` (read-only).
    Select,
    /// `INSERT`, `UPDATE`, `DELETE` (data manipulation).
    Dml,
    /// `CREATE`, `ALTER`, `DROP` (schema change).
    Ddl,
}

/// Classify a SQL statement by its first keyword.
///
/// Leading SQL comments (`--` line and `/* */` block) are stripped before
/// classification. CTEs (`WITH ... SELECT` vs `WITH ... INSERT`) are detected
/// by scanning for DML/DDL keywords after the CTE. Default is [`SqlKind::Dml`]
/// (conservative).
pub(crate) fn classify(sql: &str) -> SqlKind {
    let stripped = strip_leading_comments(sql);
    let upper = stripped.trim().to_uppercase();
    let first_word: String = upper.chars().take_while(char::is_ascii_alphabetic).collect();

    match first_word.as_str() {
        "SELECT" | "EXPLAIN" | "PRAGMA" => SqlKind::Select,
        "CREATE" | "ALTER" | "DROP" => SqlKind::Ddl,
        "WITH" => classify_cte(&upper),
        _ => SqlKind::Dml, // conservative: INSERT/UPDATE/DELETE/REPLACE and unknown
    }
}

/// Classify a CTE by scanning for DML/DDL keywords after `WITH`.
fn classify_cte(upper: &str) -> SqlKind {
    // Look for DDL keywords
    if upper.contains("CREATE ") || upper.contains("ALTER ") || upper.contains("DROP ") {
        return SqlKind::Ddl;
    }
    // Look for DML keywords
    if upper.contains("INSERT ") || upper.contains("UPDATE ") || upper.contains("DELETE ") || upper.contains("REPLACE ")
    {
        return SqlKind::Dml;
    }
    SqlKind::Select
}

/// Strip leading SQL comments from a string.
///
/// Removes `--` line comments and `/* ... */` block comments that appear
/// before the first actual SQL keyword. Handles multiple consecutive comments.
pub(crate) fn strip_leading_comments(sql: &str) -> &str {
    let mut s = sql.trim_start();
    loop {
        if s.starts_with("--") {
            // Skip to end of line
            s = s.find('\n').map_or("", |pos| s.get(pos.saturating_add(1)..).unwrap_or(""));
            s = s.trim_start();
        } else if s.starts_with("/*") {
            // Skip to closing */
            s = s.get(2..).unwrap_or("").find("*/").map_or("", |pos| s.get(pos.saturating_add(4)..).unwrap_or(""));
            s = s.trim_start();
        } else {
            break;
        }
    }
    s
}

// =============================================================================
// Statement splitting
// =============================================================================

/// Split SQL on `;`, treating a semicolon as a statement delimiter **only**
/// when it appears in ordinary code — never inside a string literal, a quoted
/// identifier, or a comment.
///
/// This is a small SQLite-aware tokenizer that skips over every region where a
/// `;` is *data*, not a delimiter:
///
/// | Region | Opener | Closer | Escape |
/// |--------|--------|--------|--------|
/// | String literal | `'` | `'` | `''` |
/// | Quoted identifier | `"` | `"` | `""` |
/// | Backtick identifier | `` ` `` | `` ` `` | ` `` ` ` ` ``` |
/// | Bracket identifier | `[` | `]` | — (no nesting/escape in SQLite) |
/// | Line comment | `--` | newline | — |
/// | Block comment | `/*` | `*/` | — (not nestable) |
///
/// A naive earlier version tracked only single-quoted strings, so a `;` inside
/// a `--`/`/* */` comment or a `"quoted;identifier"` mis-split the batch into
/// invalid fragments (same failure family as issue #113).
///
/// # UTF-8 correctness
///
/// All slicing is driven by the **byte offsets** from [`str::char_indices`],
/// never by character counts — multi-byte characters anywhere in the input are
/// preserved exactly (issue #113).
pub(crate) fn split_statements(sql: &str) -> Vec<&str> {
    let mut results = Vec::new();
    let mut start = 0; // BYTE offset where the current statement begins
    let mut chars = sql.char_indices().peekable();

    while let Some((idx, ch)) = chars.next() {
        match ch {
            // ── String literal: '...''...' ──
            '\'' => skip_quoted(&mut chars, '\''),
            // ── Double-quoted identifier: "...""..." ──
            '"' => skip_quoted(&mut chars, '"'),
            // ── Backtick identifier: `...``...` ──
            '`' => skip_quoted(&mut chars, '`'),
            // ── Bracket identifier: [...] (no escape in SQLite) ──
            '[' => skip_until(&mut chars, ']'),
            // ── Comments ──
            '-' if chars.peek().map(|&(_, c)| c) == Some('-') => {
                let _second = chars.next();
                skip_line_comment(&mut chars);
            }
            '/' if chars.peek().map(|&(_, c)| c) == Some('*') => {
                let _second = chars.next();
                skip_block_comment(&mut chars);
            }
            // ── Statement delimiter ──
            ';' => {
                let stmt = sql.get(start..idx).unwrap_or_default().trim();
                if !stmt.is_empty() && !strip_leading_comments(stmt).is_empty() {
                    results.push(stmt);
                }
                start = idx.saturating_add(ch.len_utf8());
            }
            _ => {}
        }
    }

    // Last statement (no trailing semicolon)
    let tail = sql.get(start..).unwrap_or_default().trim();
    if !tail.is_empty() && !strip_leading_comments(tail).is_empty() {
        results.push(tail);
    }

    results
}

/// Type alias for the `char_indices` iterator the skip helpers consume.
type CharCursor<'src> = std::iter::Peekable<std::str::CharIndices<'src>>;

/// Skip a quoted span opened by `quote`, honouring the SQL doubling escape
/// (`''`, `""`, ` `` `). Assumes the opening quote has already been consumed.
fn skip_quoted(chars: &mut CharCursor<'_>, quote: char) {
    while let Some((_, ch)) = chars.next() {
        if ch == quote {
            // Doubled quote → escaped literal quote, stay inside the span.
            if chars.peek().map(|&(_, c)| c) == Some(quote) {
                let _escaped = chars.next();
                continue;
            }
            return; // closing quote
        }
    }
    // Unterminated span: consume to EOF (no panic, no split).
}

/// Skip characters until `close` (inclusive). Used for bracket identifiers,
/// which have no escape mechanism in `SQLite`. Assumes the opener was consumed.
fn skip_until(chars: &mut CharCursor<'_>, close: char) {
    for (_, ch) in chars.by_ref() {
        if ch == close {
            return;
        }
    }
}

/// Skip a `--` line comment up to and including the next newline. Assumes both
/// dashes were consumed.
fn skip_line_comment(chars: &mut CharCursor<'_>) {
    for (_, ch) in chars.by_ref() {
        if ch == '\n' {
            return;
        }
    }
}

/// Skip a `/* */` block comment. `SQLite` block comments do not nest. Assumes the
/// opening `/*` was consumed.
fn skip_block_comment(chars: &mut CharCursor<'_>) {
    while let Some((_, ch)) = chars.next() {
        if ch == '*' && chars.peek().map(|&(_, c)| c) == Some('/') {
            let _slash = chars.next();
            return;
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression for issue #113: multi-byte UTF-8 in a non-final `VALUES` row
    /// must not truncate the statement. A char-indexed slice into a byte-indexed
    /// `&str` cut the tail short by the number of extra UTF-8 bytes.
    #[test]
    fn multibyte_non_final_rows_not_truncated() {
        let sql = "INSERT INTO t(a,b) VALUES\n\
                   ('s1','R\u{e9}serve l\u{e9}gale'),\n\
                   ('s2','Disponibilit\u{e9}s'),\n\
                   ('s3','Cr\u{e9}ances diverses'),\n\
                   ('s4','AT');";
        let stmts = split_statements(sql);
        assert_eq!(stmts.len(), 1, "expected one statement, got {stmts:?}");
        let stmt = stmts.first().copied().unwrap_or_default();
        // The full statement must survive — including the final row + closing paren.
        assert!(stmt.ends_with("('s4','AT')"), "statement truncated: {stmt}");
        assert!(stmt.contains("R\u{e9}serve l\u{e9}gale"));
        assert!(stmt.contains("Cr\u{e9}ances diverses"));
    }

    /// Multiple statements separated by `;`, each carrying multi-byte content,
    /// must split at the correct byte boundaries.
    #[test]
    fn multibyte_multiple_statements() {
        let sql = "INSERT INTO t VALUES ('\u{e9}');\nINSERT INTO t VALUES ('\u{e0}');";
        let stmts = split_statements(sql);
        assert_eq!(stmts.len(), 2, "got {stmts:?}");
        assert!(stmts.first().copied().unwrap_or_default().contains("'\u{e9}'"));
        assert!(stmts.get(1).copied().unwrap_or_default().contains("'\u{e0}'"));
    }

    /// Escaped quotes (`''`) inside a multi-byte string literal must not end
    /// the string early or desynchronize byte offsets.
    #[test]
    fn escaped_quotes_with_multibyte() {
        let sql = "INSERT INTO t VALUES ('Cr\u{e9}''ance'),('x');";
        let stmts = split_statements(sql);
        assert_eq!(stmts.len(), 1, "got {stmts:?}");
        assert!(stmts.first().copied().unwrap_or_default().contains("Cr\u{e9}''ance"));
    }

    /// A `;` inside a string literal (with multi-byte chars present) must NOT
    /// split the statement.
    #[test]
    fn semicolon_inside_multibyte_string() {
        let sql = "INSERT INTO t VALUES ('caf\u{e9}; th\u{e9}');";
        let stmts = split_statements(sql);
        assert_eq!(stmts.len(), 1, "got {stmts:?}");
        assert!(stmts.first().copied().unwrap_or_default().contains("caf\u{e9}; th\u{e9}"));
    }

    /// Pure-ASCII multi-statement splitting still works (no regression).
    #[test]
    fn ascii_multi_statement() {
        let sql = "CREATE TABLE t (a TEXT); INSERT INTO t VALUES ('x'); SELECT * FROM t;";
        let stmts = split_statements(sql);
        assert_eq!(stmts.len(), 3, "got {stmts:?}");
    }

    /// Classification stays correct after the rewrite.
    #[test]
    fn classify_basic() {
        assert_eq!(classify("SELECT 1"), SqlKind::Select);
        assert_eq!(classify("  -- c\nINSERT INTO t VALUES (1)"), SqlKind::Dml);
        assert_eq!(classify("CREATE TABLE t (a)"), SqlKind::Ddl);
        assert_eq!(classify("WITH x AS (SELECT 1) INSERT INTO t SELECT * FROM x"), SqlKind::Dml);
        assert_eq!(classify("WITH x AS (SELECT 1) SELECT * FROM x"), SqlKind::Select);
    }

    // ── Comment-aware splitting (stress-test regressions) ───────────────

    /// A `;` inside a trailing `--` line comment must NOT split the batch.
    /// Previously produced invalid fragments (`-- reset` / `clear...`).
    #[test]
    fn semicolon_in_line_comment_not_split() {
        let sql = "DELETE FROM t; -- reset; clear everything\nINSERT INTO t VALUES ('p');";
        let stmts = split_statements(sql);
        assert_eq!(stmts.len(), 2, "got {stmts:?}");
        assert_eq!(stmts.first().copied().unwrap_or_default(), "DELETE FROM t");
        assert!(stmts.get(1).copied().unwrap_or_default().starts_with("-- reset; clear everything"));
        assert!(stmts.get(1).copied().unwrap_or_default().contains("INSERT INTO t"));
    }

    /// A `;` inside a `/* block comment */` must NOT split the statement.
    #[test]
    fn semicolon_in_block_comment_not_split() {
        let sql = "UPDATE t SET b='z' /* note; do not remove */ WHERE a='x';\nINSERT INTO t VALUES ('m');";
        let stmts = split_statements(sql);
        assert_eq!(stmts.len(), 2, "got {stmts:?}");
        assert!(stmts.first().copied().unwrap_or_default().contains("/* note; do not remove */"));
        assert!(stmts.get(1).copied().unwrap_or_default().contains("INSERT INTO t"));
    }

    /// A line comment containing `'` or `"` must not confuse quote tracking.
    #[test]
    fn quotes_inside_line_comment_ignored() {
        let sql = "SELECT 1; -- it's a \"weird\" comment; really\nSELECT 2;";
        let stmts = split_statements(sql);
        assert_eq!(stmts.len(), 2, "got {stmts:?}");
        assert_eq!(stmts.first().copied().unwrap_or_default(), "SELECT 1");
    }

    // ── Identifier-quoting-aware splitting ──────────────────────────────

    /// A `;` inside a double-quoted identifier must NOT split.
    #[test]
    fn semicolon_in_double_quoted_identifier() {
        let sql = "SELECT 'a;b' AS \"col;name\" FROM t;";
        let stmts = split_statements(sql);
        assert_eq!(stmts.len(), 1, "got {stmts:?}");
        assert!(stmts.first().copied().unwrap_or_default().contains("\"col;name\""));
    }

    /// A `;` inside a backtick identifier must NOT split.
    #[test]
    fn semicolon_in_backtick_identifier() {
        let sql = "SELECT 1 AS `weird;col` FROM t;";
        let stmts = split_statements(sql);
        assert_eq!(stmts.len(), 1, "got {stmts:?}");
        assert!(stmts.first().copied().unwrap_or_default().contains("`weird;col`"));
    }

    /// A `;` inside a `[bracket]` identifier must NOT split.
    #[test]
    fn semicolon_in_bracket_identifier() {
        let sql = "SELECT 1 AS [weird;col] FROM t;";
        let stmts = split_statements(sql);
        assert_eq!(stmts.len(), 1, "got {stmts:?}");
        assert!(stmts.first().copied().unwrap_or_default().contains("[weird;col]"));
    }

    /// Doubled double-quotes (`""`) escape a literal quote inside an identifier.
    #[test]
    fn escaped_double_quote_in_identifier() {
        let sql = "SELECT 1 AS \"a\"\"b;c\" FROM t; SELECT 2;";
        let stmts = split_statements(sql);
        assert_eq!(stmts.len(), 2, "got {stmts:?}");
        assert!(stmts.first().copied().unwrap_or_default().contains("\"a\"\"b;c\""));
    }

    /// An unterminated string/identifier/comment must not panic — the tail is
    /// simply consumed to EOF as a single (possibly malformed) statement.
    #[test]
    fn unterminated_spans_do_not_panic() {
        for sql in ["SELECT 'abc", "SELECT \"abc", "SELECT 1 /* unterminated", "SELECT 1 -- trailing"] {
            let _stmts = split_statements(sql); // must not panic
        }
    }

    /// Combined adversarial input: comments, multiple quote styles, multi-byte,
    /// and in-data semicolons — exactly one statement should result.
    #[test]
    fn combined_adversarial_single_statement() {
        let sql = "INSERT INTO \"tbl;x\" (a,b) /* c; */ VALUES ('R\u{e9}serve; l\u{e9}gale','x') -- trailing; note";
        let stmts = split_statements(sql);
        assert_eq!(stmts.len(), 1, "got {stmts:?}");
        let stmt = stmts.first().copied().unwrap_or_default();
        assert!(stmt.contains("R\u{e9}serve; l\u{e9}gale"));
        assert!(stmt.contains("\"tbl;x\""));
    }
}
