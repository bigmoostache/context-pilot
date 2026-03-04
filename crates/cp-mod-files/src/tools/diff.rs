/// Generate a unified diff showing changes between old and new strings
pub(crate) fn generate_unified_diff(old: &str, new: &str) -> String {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    let diff_ops = compute_diff(&old_lines, &new_lines);

    let mut result = String::new();
    for op in diff_ops {
        match op {
            DiffOp::Equal(line) => {
                result.push_str(&format!("  {}\n", line));
            }
            DiffOp::Delete(line) => {
                result.push_str(&format!("- {}\n", line));
            }
            DiffOp::Insert(line) => {
                result.push_str(&format!("+ {}\n", line));
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
/// Returns pairs of (old_index, new_index) for matching lines in ascending order.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unified_diff_with_common_lines() {
        let old = "line1\nline2\nline3";
        let new = "line1\nline2_modified\nline3";

        let diff = generate_unified_diff(old, new);

        assert!(diff.contains("  line1\n"));
        assert!(diff.contains("- line2\n"));
        assert!(diff.contains("+ line2_modified\n"));
        assert!(diff.contains("  line3\n"));

        let del_pos = diff.find("- line2\n").unwrap();
        let add_pos = diff.find("+ line2_modified\n").unwrap();
        assert!(del_pos < add_pos);

        let between = &diff[del_pos + "- line2\n".len()..add_pos];
        assert!(between.is_empty(), "Expected interleaved diff, but found: '{}'", between);
    }

    #[test]
    fn test_unified_diff_interleaved_changes() {
        let old = "keep1\nchange1\nkeep2\nchange2\nkeep3";
        let new = "keep1\nnew1\nkeep2\nnew2\nkeep3";

        let diff = generate_unified_diff(old, new);

        assert!(diff.contains("  keep1\n"));
        assert!(diff.contains("- change1\n"));
        assert!(diff.contains("+ new1\n"));
        assert!(diff.contains("  keep2\n"));
        assert!(diff.contains("- change2\n"));
        assert!(diff.contains("+ new2\n"));
        assert!(diff.contains("  keep3\n"));

        let change1_del_pos = diff.find("- change1\n").unwrap();
        let change1_add_pos = diff.find("+ new1\n").unwrap();
        let keep2_pos = diff.find("  keep2\n").unwrap();

        assert!(change1_del_pos < change1_add_pos);
        assert!(change1_add_pos < keep2_pos);
    }

    #[test]
    fn test_unified_diff_pure_addition() {
        let old = "line1";
        let new = "line1\nline2\nline3";

        let diff = generate_unified_diff(old, new);

        assert!(diff.contains("  line1\n"));
        assert!(diff.contains("+ line2\n"));
        assert!(diff.contains("+ line3\n"));
    }

    #[test]
    fn test_unified_diff_pure_deletion() {
        let old = "line1\nline2\nline3";
        let new = "line1";

        let diff = generate_unified_diff(old, new);

        assert!(diff.contains("  line1\n"));
        assert!(diff.contains("- line2\n"));
        assert!(diff.contains("- line3\n"));
    }
}
