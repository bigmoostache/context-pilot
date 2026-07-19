use std::fmt::Write as _;
/// Generate a unified diff showing changes between old and new strings
pub(crate) fn generate_unified_diff(old: &str, new: &str) -> String {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    let diff_ops = compute_diff(&old_lines, &new_lines);

    let mut result = String::new();
    for op in diff_ops {
        match op {
            DiffOp::Equal(line) => {
                let _r = writeln!(result, "  {line}");
            }
            DiffOp::Delete(line) => {
                let _r = writeln!(result, "- {line}");
            }
            DiffOp::Insert(line) => {
                let _r = writeln!(result, "+ {line}");
            }
        }
    }

    result
}

/// A single diff operation on a line.
#[derive(Debug, Clone, PartialEq)]
enum DiffOp<'src> {
    /// Line is unchanged between old and new.
    Equal(&'src str),
    /// Line was removed from the old text.
    Delete(&'src str),
    /// Line was added in the new text.
    Insert(&'src str),
}

/// Drain remaining old lines as `Delete` ops until `old_idx` reaches `stop`.
fn drain_deletes<'src>(result: &mut Vec<DiffOp<'src>>, old_lines: &[&'src str], old_idx: &mut usize, stop: usize) {
    while *old_idx < stop {
        if let Some(line) = old_lines.get(*old_idx) {
            result.push(DiffOp::Delete(line));
        }
        *old_idx = old_idx.saturating_add(1);
    }
}

/// Drain remaining new lines as `Insert` ops until `new_idx` reaches `stop`.
fn drain_inserts<'src>(result: &mut Vec<DiffOp<'src>>, new_lines: &[&'src str], new_idx: &mut usize, stop: usize) {
    while *new_idx < stop {
        if let Some(line) = new_lines.get(*new_idx) {
            result.push(DiffOp::Insert(line));
        }
        *new_idx = new_idx.saturating_add(1);
    }
}

/// Compute diff operations using a simple LCS-based algorithm
fn compute_diff<'src>(old_lines: &[&'src str], new_lines: &[&'src str]) -> Vec<DiffOp<'src>> {
    let lcs = lcs(old_lines, new_lines);
    let mut result = Vec::new();
    let mut old_idx: usize = 0;
    let mut new_idx: usize = 0;
    let mut lcs_idx: usize = 0;

    while old_idx < old_lines.len() || new_idx < new_lines.len() {
        if let Some(&(lcs_old, lcs_new)) = lcs.get(lcs_idx) {
            drain_deletes(&mut result, old_lines, &mut old_idx, lcs_old);
            drain_inserts(&mut result, new_lines, &mut new_idx, lcs_new);

            if let Some(line) = old_lines.get(old_idx) {
                result.push(DiffOp::Equal(line));
            }
            old_idx = old_idx.saturating_add(1);
            new_idx = new_idx.saturating_add(1);
            lcs_idx = lcs_idx.saturating_add(1);
        } else {
            drain_deletes(&mut result, old_lines, &mut old_idx, old_lines.len());
            drain_inserts(&mut result, new_lines, &mut new_idx, new_lines.len());
        }
    }

    result
}

/// Build the LCS length DP table: `lengths[i][j]` = LCS length of `old[..i]`
/// and `new[..j]`.
fn lcs_lengths(old: &[&str], new: &[&str]) -> Vec<Vec<usize>> {
    let old_len = old.len();
    let new_len = new.len();
    let mut lengths = vec![vec![0usize; new_len.saturating_add(1)]; old_len.saturating_add(1)];

    for i in 1..=old_len {
        for j in 1..=new_len {
            let i_prev = i.saturating_sub(1);
            let j_prev = j.saturating_sub(1);
            let prev_diag = lengths.get(i_prev).and_then(|r| r.get(j_prev)).copied().unwrap_or(0);
            let value = if old.get(i_prev) == new.get(j_prev) {
                prev_diag.saturating_add(1)
            } else {
                let up = lengths.get(i_prev).and_then(|r| r.get(j)).copied().unwrap_or(0);
                let left = lengths.get(i).and_then(|r| r.get(j_prev)).copied().unwrap_or(0);
                up.max(left)
            };
            if let Some(cell) = lengths.get_mut(i).and_then(|r| r.get_mut(j)) {
                *cell = value;
            }
        }
    }
    lengths
}

/// Find the Longest Common Subsequence (LCS) between two sequences.
/// Returns pairs of (`old_index`, `new_index`) for matching lines in ascending order.
///
/// Note: O(m*n) space. Acceptable for typical file edits.
fn lcs<'src>(old: &[&'src str], new: &[&'src str]) -> Vec<(usize, usize)> {
    let lengths = lcs_lengths(old, new);

    let mut result = Vec::new();
    let mut i = old.len();
    let mut j = new.len();

    while i > 0 && j > 0 {
        let i_prev = i.saturating_sub(1);
        let j_prev = j.saturating_sub(1);
        if old.get(i_prev) == new.get(j_prev) {
            result.push((i_prev, j_prev));
            i = i_prev;
            j = j_prev;
        } else {
            let up = lengths.get(i_prev).and_then(|r| r.get(j)).copied().unwrap_or(0);
            let left = lengths.get(i).and_then(|r| r.get(j_prev)).copied().unwrap_or(0);
            if up > left {
                i = i_prev;
            } else {
                j = j_prev;
            }
        }
    }

    result.reverse();
    result
}
