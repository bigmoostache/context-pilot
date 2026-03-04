/// Generate a unified diff showing changes between old and new strings
pub(crate) fn generate_unified_diff(old: &str, new: &str) -> String {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    let diff_ops = compute_diff(&old_lines, &new_lines);

    let mut result = String::new();
    for op in diff_ops {
        match op {
            DiffOp::Equal(line) => {
                result.push_str(&format!("  {line}\n"));
            }
            DiffOp::Delete(line) => {
                result.push_str(&format!("- {line}\n"));
            }
            DiffOp::Insert(line) => {
                result.push_str(&format!("+ {line}\n"));
            }
        }
    }

    result
}

#[derive(Debug, Clone, PartialEq)]
enum DiffOp<'a> {
    Equal(&'a str),
    Delete(&'a str),
    Insert(&'a str),
}

/// Compute diff operations using a simple LCS-based algorithm
fn compute_diff<'a>(old_lines: &[&'a str], new_lines: &[&'a str]) -> Vec<DiffOp<'a>> {
    let lcs = lcs(old_lines, new_lines);
    let mut result = Vec::new();
    let mut old_idx = 0;
    let mut new_idx = 0;
    let mut lcs_idx = 0;

    while old_idx < old_lines.len() || new_idx < new_lines.len() {
        if lcs_idx < lcs.len() {
            let (lcs_old, lcs_new) = lcs[lcs_idx];

            while old_idx < lcs_old {
                result.push(DiffOp::Delete(old_lines[old_idx]));
                old_idx += 1;
            }

            while new_idx < lcs_new {
                result.push(DiffOp::Insert(new_lines[new_idx]));
                new_idx += 1;
            }

            result.push(DiffOp::Equal(old_lines[old_idx]));
            old_idx += 1;
            new_idx += 1;
            lcs_idx += 1;
        } else {
            while old_idx < old_lines.len() {
                result.push(DiffOp::Delete(old_lines[old_idx]));
                old_idx += 1;
            }
            while new_idx < new_lines.len() {
                result.push(DiffOp::Insert(new_lines[new_idx]));
                new_idx += 1;
            }
        }
    }

    result
}

/// Find the Longest Common Subsequence (LCS) between two sequences.
/// Returns pairs of (`old_index`, `new_index`) for matching lines in ascending order.
///
/// Note: O(m*n) space. Acceptable for typical file edits.
fn lcs<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<(usize, usize)> {
    let m = old.len();
    let n = new.len();

    let mut lengths = vec![vec![0; n + 1]; m + 1];

    for i in 1..=m {
        for j in 1..=n {
            if old[i - 1] == new[j - 1] {
                lengths[i][j] = lengths[i - 1][j - 1] + 1;
            } else {
                lengths[i][j] = lengths[i - 1][j].max(lengths[i][j - 1]);
            }
        }
    }

    let mut result = Vec::new();
    let mut i = m;
    let mut j = n;

    while i > 0 && j > 0 {
        if old[i - 1] == new[j - 1] {
            result.push((i - 1, j - 1));
            i -= 1;
            j -= 1;
        } else if lengths[i - 1][j] > lengths[i][j - 1] {
            i -= 1;
        } else {
            j -= 1;
        }
    }

    result.reverse();
    result
}
