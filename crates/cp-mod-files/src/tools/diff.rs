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

/// Compute diff operations using a simple LCS-based algorithm
fn compute_diff<'src>(old_lines: &[&'src str], new_lines: &[&'src str]) -> Vec<DiffOp<'src>> {
    let lcs = lcs(old_lines, new_lines);
    let mut result = Vec::new();
    let mut old_idx: usize = 0;
    let mut new_idx: usize = 0;
    let mut lcs_idx: usize = 0;

    while old_idx < old_lines.len() || new_idx < new_lines.len() {
        if let Some(&(lcs_old, lcs_new)) = lcs.get(lcs_idx) {
            while old_idx < lcs_old {
                if let Some(line) = old_lines.get(old_idx) {
                    result.push(DiffOp::Delete(line));
                }
                old_idx = old_idx.saturating_add(1);
            }

            while new_idx < lcs_new {
                if let Some(line) = new_lines.get(new_idx) {
                    result.push(DiffOp::Insert(line));
                }
                new_idx = new_idx.saturating_add(1);
            }

            if let Some(line) = old_lines.get(old_idx) {
                result.push(DiffOp::Equal(line));
            }
            old_idx = old_idx.saturating_add(1);
            new_idx = new_idx.saturating_add(1);
            lcs_idx = lcs_idx.saturating_add(1);
        } else {
            while old_idx < old_lines.len() {
                if let Some(line) = old_lines.get(old_idx) {
                    result.push(DiffOp::Delete(line));
                }
                old_idx = old_idx.saturating_add(1);
            }
            while new_idx < new_lines.len() {
                if let Some(line) = new_lines.get(new_idx) {
                    result.push(DiffOp::Insert(line));
                }
                new_idx = new_idx.saturating_add(1);
            }
        }
    }

    result
}

/// Find the Longest Common Subsequence (LCS) between two sequences.
/// Returns pairs of (`old_index`, `new_index`) for matching lines in ascending order.
///
/// Note: O(m*n) space. Acceptable for typical file edits.
#[expect(
    clippy::indexing_slicing,
    reason = "indices i in 0..=old_len and j in 0..=new_len are always within bounds of the (old_len+1)x(new_len+1) lengths table and the old/new slices"
)]
fn lcs<'src>(old: &[&'src str], new: &[&'src str]) -> Vec<(usize, usize)> {
    let old_len = old.len();
    let new_len = new.len();

    let mut lengths = vec![vec![0usize; new_len.saturating_add(1)]; old_len.saturating_add(1)];

    for i in 1..=old_len {
        for j in 1..=new_len {
            let i_prev = i.saturating_sub(1);
            let j_prev = j.saturating_sub(1);
            if old[i_prev] == new[j_prev] {
                lengths[i][j] = lengths[i_prev][j_prev].saturating_add(1);
            } else {
                lengths[i][j] = lengths[i_prev][j].max(lengths[i][j_prev]);
            }
        }
    }

    let mut result = Vec::new();
    let mut i = old_len;
    let mut j = new_len;

    while i > 0 && j > 0 {
        let i_prev = i.saturating_sub(1);
        let j_prev = j.saturating_sub(1);
        if old[i_prev] == new[j_prev] {
            result.push((i_prev, j_prev));
            i = i_prev;
            j = j_prev;
        } else if lengths[i_prev][j] > lengths[i][j_prev] {
            i = i_prev;
        } else {
            j = j_prev;
        }
    }

    result.reverse();
    result
}
