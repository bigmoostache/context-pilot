//! SQL error enrichment: fuzzy suggestions and schema context.

use crate::types::SchemaCache;

/// Enrich a `SQLite` error message with schema context and fuzzy suggestions.
pub(crate) fn enrich_error(err: &str, schema: &SchemaCache) -> String {
    let mut parts = vec![format!("SQL error: {err}")];

    // Detect "no such table" and suggest closest match
    if let Some(unknown) = extract_after(err, "no such table: ") {
        let names: Vec<&str> = schema.tables.iter().map(|t| t.name.as_str()).collect();
        if let Some(suggestion) = closest_match(&unknown, &names, 2) {
            parts.push(format!("Did you mean table '{suggestion}'?"));
        }
    }

    // Detect "no such column" and suggest closest match
    if let Some(unknown) = extract_after(err, "no such column: ") {
        let all_cols: Vec<String> =
            schema.tables.iter().flat_map(|t| t.columns.iter().map(|c| c.name.clone())).collect();
        let col_refs: Vec<&str> = all_cols.iter().map(String::as_str).collect();
        if let Some(suggestion) = closest_match(&unknown, &col_refs, 2) {
            parts.push(format!("Did you mean column '{suggestion}'?"));
        }
    }

    // Append schema summary
    if !schema.tables.is_empty() {
        parts.push(String::from("\nCurrent schema:"));
        for table in &schema.tables {
            let cols: Vec<String> = table
                .columns
                .iter()
                .map(|c| {
                    let pk = if c.is_pk { " PK" } else { "" };
                    format!("{} {}{pk}", c.name, c.col_type)
                })
                .collect();
            parts.push(format!("  {} ({}): {}", table.name, table.row_count, cols.join(", ")));
        }
    }

    parts.join("\n")
}

/// Extract text after a pattern in an error message.
fn extract_after(err: &str, pattern: &str) -> Option<String> {
    let pos = err.find(pattern)?;
    let start = pos.saturating_add(pattern.len());
    let rest = err.get(start..)?;
    let word: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    if word.is_empty() { None } else { Some(word) }
}

/// Find the closest match within a Levenshtein distance threshold.
fn closest_match<'candidate>(target: &str, candidates: &[&'candidate str], max_dist: usize) -> Option<&'candidate str> {
    let target_lower = target.to_lowercase();
    let mut best: Option<(&str, usize)> = None;

    for candidate in candidates {
        let dist = levenshtein(&target_lower, &candidate.to_lowercase());
        if dist <= max_dist && (best.is_none() || dist < best.map_or(usize::MAX, |(_, d)| d)) {
            best = Some((candidate, dist));
        }
    }

    best.map(|(name, _)| name)
}

/// Levenshtein distance between two strings.
fn levenshtein(source: &str, target: &str) -> usize {
    let source_chars: Vec<char> = source.chars().collect();
    let target_chars: Vec<char> = target.chars().collect();
    let source_len = source_chars.len();
    let target_len = target_chars.len();

    if source_len == 0 {
        return target_len;
    }
    if target_len == 0 {
        return source_len;
    }

    // Use a single row (previous + current)
    let row_len = target_len.saturating_add(1);
    let mut prev: Vec<usize> = (0..row_len).collect();
    let mut curr = vec![0usize; row_len];

    for i in 1..=source_len {
        if let Some(cell) = curr.get_mut(0) {
            *cell = i;
        }
        for j in 1..=target_len {
            let cost = usize::from(source_chars.get(i.saturating_sub(1)) != target_chars.get(j.saturating_sub(1)));

            let del = prev.get(j).copied().unwrap_or(usize::MAX).saturating_add(1);
            let ins = curr.get(j.saturating_sub(1)).copied().unwrap_or(usize::MAX).saturating_add(1);
            let sub = prev.get(j.saturating_sub(1)).copied().unwrap_or(usize::MAX).saturating_add(cost);

            if let Some(cell) = curr.get_mut(j) {
                *cell = del.min(ins).min(sub);
            }
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev.get(target_len).copied().unwrap_or_default()
}
