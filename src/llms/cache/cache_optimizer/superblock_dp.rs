//! Per-super-block forward DP: split a super-block `(left, right]` into at most
//! `max_cuts` internal cuts minimizing expected recompute cost.
//!
//! Extracted from [`super`] to keep `cache_optimizer.rs` under the 500-line
//! structure limit. Operates on the parent's [`PrefixSums`] for O(1) block-cost
//! queries.

use cp_base::cast::Safe as _;

use super::PrefixSums;

/// Normalize density weights to a probability distribution.
/// Falls back to uniform if weights sum to zero or are degenerate.
pub(super) fn normalize_density(weights: &[f64]) -> Vec<f64> {
    let total: f64 = weights.iter().copied().sum();
    if total <= 0.0f64 || !total.is_finite() {
        let len = weights.len();
        if len == 0 {
            return vec![];
        }
        let uniform = 1.0f64 / len.to_f64();
        return vec![uniform; len];
    }
    weights.iter().map(|&w| w.max(0.0) / total).collect()
}

/// Forward DP tables over candidate cut positions within a super-block.
/// `h_table[j][c]` = min cost covering `(left_bound, candidates[c]]` with
/// exactly `j` cuts, rightmost at `candidates[c]`; `parent_table[j][c]` = the
/// predecessor candidate index for backtracking.
struct HTable {
    /// `h[j][c]` = min cost covering up to candidate `c` with exactly `j` cuts.
    h: Vec<Vec<f64>>,
    /// `parent[j][c]` = predecessor candidate index for backtracking.
    parent: Vec<Vec<Option<usize>>>,
}

/// Seed the `j = 1` row: a single cut at each candidate covers `(left_bound, pos]`.
fn fill_h_base(h: &mut [Vec<f64>], candidates: &[usize], left_bound: usize, ps: &PrefixSums) {
    let Some(h_row) = h.get_mut(1) else { return };
    for (c_idx, &pos) in candidates.iter().enumerate() {
        if let Some(cell) = h_row.get_mut(c_idx) {
            *cell = ps.block_cost(left_bound, pos);
        }
    }
}

/// Target cell + predecessor for a forward-DP relaxation step.
struct HCell {
    /// Number of cuts (row index) being relaxed.
    cut_count: usize,
    /// Candidate index (column) being relaxed.
    c_idx: usize,
    /// Predecessor candidate index for backtracking.
    prev_idx: usize,
}

/// Relax `h[cell.cut_count][cell.c_idx]` to `cost` (with predecessor
/// `cell.prev_idx`) if better.
fn relax_h_cell(table: &mut HTable, cell: &HCell, cost: f64) {
    let HCell { cut_count, c_idx, prev_idx } = *cell;
    let current = table.h.get(cut_count).and_then(|row| row.get(c_idx)).copied().unwrap_or(f64::INFINITY);
    if cost >= current {
        return;
    }
    if let Some(slot) = table.h.get_mut(cut_count).and_then(|row| row.get_mut(c_idx)) {
        *slot = cost;
    }
    if let Some(slot) = table.parent.get_mut(cut_count).and_then(|row| row.get_mut(c_idx)) {
        *slot = Some(prev_idx);
    }
}

/// Fill the forward DP `HTable` for `candidates` (positions strictly inside the
/// super-block), for cut counts `1..=max_cuts`.
fn fill_h_table(candidates: &[usize], left_bound: usize, max_cuts: usize, ps: &PrefixSums) -> HTable {
    let nc = candidates.len();
    let h = vec![vec![f64::INFINITY; nc]; max_cuts.saturating_add(1)];
    let parent: Vec<Vec<Option<usize>>> = vec![vec![None; nc]; max_cuts.saturating_add(1)];
    let mut table = HTable { h, parent };

    fill_h_base(&mut table.h, candidates, left_bound, ps);

    for cut_count in 2..=max_cuts {
        let min_c = cut_count.saturating_sub(1);
        for c_idx in min_c..nc {
            let pos = candidates.get(c_idx).copied().unwrap_or(0);
            let min_prev = cut_count.saturating_sub(2);
            for prev_idx in min_prev..c_idx {
                let prev_pos = candidates.get(prev_idx).copied().unwrap_or(0);
                let prev_h = table
                    .h
                    .get(cut_count.saturating_sub(1))
                    .and_then(|row| row.get(prev_idx))
                    .copied()
                    .unwrap_or(f64::INFINITY);
                relax_h_cell(&mut table, &HCell { cut_count, c_idx, prev_idx }, prev_h + ps.block_cost(prev_pos, pos));
            }
        }
    }
    table
}

/// Backtrack the `HTable` from rightmost candidate `c_idx` to recover the
/// `cut_count` cut positions (ascending).
fn backtrack_cuts(table: &HTable, candidates: &[usize], cut_count: usize, c_idx: usize) -> Vec<usize> {
    let mut positions = Vec::with_capacity(cut_count);
    let mut trace = c_idx;
    for jj in (1..=cut_count).rev() {
        if let Some(&cand) = candidates.get(trace) {
            positions.push(cand);
        }
        if let Some(prev) = table.parent.get(jj).and_then(|row| row.get(trace)).copied().flatten() {
            trace = prev;
        }
    }
    positions.reverse();
    positions
}

/// Per-super-block DP outputs, indexed by cut count `j`.
struct SuperblockOut {
    /// `costs[j]` = minimum cost with exactly `j` cuts.
    costs: Vec<f64>,
    /// `best_cuts[j]` = the cut positions achieving that minimum.
    best_cuts: Vec<Vec<usize>>,
}

/// Read-only inputs for the best-cut scan over a super-block.
struct CutScan<'ctx> {
    /// Filled forward-DP table for this super-block.
    table: &'ctx HTable,
    /// Candidate cut positions (strictly inside the super-block).
    candidates: &'ctx [usize],
    /// Right boundary of the super-block.
    right_bound: usize,
    /// Prefix sums for O(1) block-cost queries.
    ps: &'ctx PrefixSums,
}

/// For each cut count, pick the rightmost candidate minimizing prefix + tail
/// cost, writing the winning cost + positions into `out`.
fn collect_best_cuts(scan: &CutScan<'_>, out: &mut SuperblockOut) {
    let CutScan { table, candidates, right_bound, ps } = *scan;
    let nc = candidates.len();
    let max_cuts = out.costs.len().saturating_sub(1);
    for cut_count in 1..=max_cuts {
        let min_c = cut_count.saturating_sub(1);
        for c_idx in min_c..nc {
            let pos = candidates.get(c_idx).copied().unwrap_or(0);
            let prefix_cost = table.h.get(cut_count).and_then(|row| row.get(c_idx)).copied().unwrap_or(f64::INFINITY);
            let total = prefix_cost + ps.block_cost(pos, right_bound);
            let current_best = out.costs.get(cut_count).copied().unwrap_or(f64::INFINITY);
            if total < current_best {
                if let Some(slot) = out.costs.get_mut(cut_count) {
                    *slot = total;
                }
                if let Some(slot) = out.best_cuts.get_mut(cut_count) {
                    *slot = backtrack_cuts(table, candidates, cut_count, c_idx);
                }
            }
        }
    }
}

/// Solve the per-super-block DP for `(left_bound, right_bound]` with up to
/// `max_cuts` internal cuts.
///
/// Returns `(costs, cuts)` where:
/// - `costs[j]` = minimum cost with exactly `j` cuts
/// - `cuts[j]` = the cut positions achieving that minimum
pub(super) fn solve_superblock(
    left_bound: usize,
    right_bound: usize,
    max_cuts: usize,
    ps: &PrefixSums,
) -> (Vec<f64>, Vec<Vec<usize>>) {
    let mut out = SuperblockOut {
        costs: vec![f64::INFINITY; max_cuts.saturating_add(1)],
        best_cuts: vec![vec![]; max_cuts.saturating_add(1)],
    };

    // Base case: 0 cuts = cost of the entire sub-block
    if let Some(slot) = out.costs.get_mut(0) {
        *slot = ps.block_cost(left_bound, right_bound);
    }

    let width = right_bound.saturating_sub(left_bound);
    if width <= 1 || max_cuts == 0 {
        return (out.costs, out.best_cuts);
    }

    // Candidate cut positions: left_bound+1 .. right_bound-1
    let candidates: Vec<usize> = (left_bound.saturating_add(1)..right_bound).collect();
    if candidates.is_empty() {
        return (out.costs, out.best_cuts);
    }

    let table = fill_h_table(&candidates, left_bound, max_cuts, ps);
    collect_best_cuts(&CutScan { table: &table, candidates: &candidates, right_bound, ps }, &mut out);

    (out.costs, out.best_cuts)
}
