//! Exact two-level DP optimizer for cache breakpoint placement.
//!
//! Given N content blocks with token counts `t[1..N]`, a divergence density
//! `p[1..N]`, and a set of fixed boundaries Ω, finds the set Γ of at most K
//! new breakpoints that minimizes the expected recompute cost:
//!
//! ```text
//! L(Γ) = Σ_{x=1}^{N}  p_x · (T_x − T_{Ω'₋(x)})
//! ```
//!
//! where `Ω' = Ω ∪ Γ ∪ {0, N}` and `Ω'₋(x)` is the largest element of
//! `Ω' ∪ {0}` strictly less than `x`.
//!
//! # Indexing convention
//!
//! **1-indexed throughout this module.** Blocks are numbered `1..=N`.
//! Boundaries live in `{1..=N-1}`. Sentinels `0` (left) and `N` (right)
//! are implicit. Callers using 0-indexed block arrays must convert at the
//! boundary — see [`CacheEngine::compute_breakpoints`] for the integration
//! point.
//!
//! # Algorithm
//!
//! Ω partitions `{1..=N}` into independent **super-blocks** `(a, b]`.
//! Optimization decomposes into:
//!
//! 1. **Phase 1** — Per super-block: standard 1D split DP computes `f[j]` =
//!    min cost with exactly `j` cuts, for `j ∈ 0..=K`. Each `block_cost(u, v)`
//!    is O(1) via prefix sums.
//!
//! 2. **Phase 2** — Knapsack across super-blocks: `dp[blk][j]` = min total
//!    cost using the first `blk` super-blocks with `j` total cuts.
//!
//! Total: **O(N² · K)** time, **O(N · K)** space.

use cp_base::cast::Safe as _;

// ─── Public API ─────────────────────────────────────────────────────────────

/// Result of the optimization: chosen breakpoint positions and expected cost.
#[derive(Debug, Clone)]
pub(crate) struct OptimizationResult {
    /// Sorted breakpoint positions to place (1-indexed, within `{1..=N-1}`).
    /// Length ≤ K. Disjoint from Ω.
    pub gamma: Vec<usize>,
    /// Expected recompute cost L(Γ) in token-units.
    #[cfg(test)]
    pub cost: f64,
}

/// Prefix-sum arrays for O(1) block-cost computation.
///
/// All arrays are 1-indexed with a zero sentinel at position 0.
struct PrefixSums {
    /// `big_t[i]` = cumulative token count through block `i`.
    big_t: Vec<f64>,
    /// `s_p[i]` = cumulative normalized probability through block `i`.
    s_p: Vec<f64>,
    /// `s_pt[i]` = cumulative `p_x * T_x` through block `i`.
    s_pt: Vec<f64>,
}

impl PrefixSums {
    /// Build prefix sums from raw token counts and density weights.
    fn build(tokens: &[u32], density_weights: &[f64]) -> Self {
        let num_blocks = tokens.len();
        let prob = normalize_density(density_weights);

        let mut big_t = vec![0.0_f64; num_blocks.saturating_add(1)];
        let mut s_p = vec![0.0_f64; num_blocks.saturating_add(1)];
        let mut s_pt = vec![0.0_f64; num_blocks.saturating_add(1)];

        for idx in 1..=num_blocks {
            let tok = tokens.get(idx.saturating_sub(1)).copied().unwrap_or(0);
            let p_val = prob.get(idx.saturating_sub(1)).copied().unwrap_or(0.0);
            let prev_t = big_t.get(idx.saturating_sub(1)).copied().unwrap_or(0.0);
            let prev_sp = s_p.get(idx.saturating_sub(1)).copied().unwrap_or(0.0);
            let prev_spt = s_pt.get(idx.saturating_sub(1)).copied().unwrap_or(0.0);

            let cur_t = prev_t + tok.to_f64();
            if let Some(slot) = big_t.get_mut(idx) {
                *slot = cur_t;
            }
            if let Some(slot) = s_p.get_mut(idx) {
                *slot = prev_sp + p_val;
            }
            if let Some(slot) = s_pt.get_mut(idx) {
                *slot = prev_spt.mul_add(1.0, p_val * cur_t);
            }
        }

        Self { big_t, s_p, s_pt }
    }

    /// Cost of a sub-block `(left, right]` with no internal cuts.
    ///
    /// ```text
    /// block_cost(u, v) = (S_pT[v] − S_pT[u]) − T_u · (S_p[v] − S_p[u])
    /// ```
    #[inline]
    fn block_cost(&self, left: usize, right: usize) -> f64 {
        let cum_pt_right = self.s_pt.get(right).copied().unwrap_or(0.0);
        let cum_pt_left = self.s_pt.get(left).copied().unwrap_or(0.0);
        let tok_left = self.big_t.get(left).copied().unwrap_or(0.0);
        let cum_prob_right = self.s_p.get(right).copied().unwrap_or(0.0);
        let cum_prob_left = self.s_p.get(left).copied().unwrap_or(0.0);

        (cum_pt_right - cum_pt_left) - tok_left * (cum_prob_right - cum_prob_left)
    }
}

/// Find the set Γ of at most `budget` new breakpoints minimizing expected
/// recompute cost under the given divergence density.
///
/// # Arguments
/// * `tokens` — Token counts per block, length N. `tokens[0]` = block 1's tokens.
/// * `density_weights` — Un-normalized divergence weights, length N.
///   `density_weights[0]` = weight for block 1. Normalized internally.
/// * `omega` — Fixed boundary positions (1-indexed, within `{1..=N-1}`).
///   Typically the cache frontier + alive breakpoints.
/// * `budget` — Max number of new breakpoints to place.
///
/// # Panics
/// Debug-asserts that `tokens` and `density_weights` have the same length,
/// and that all Ω values are in `{1..=N-1}`.
pub(crate) fn optimize_gamma(
    tokens: &[u32],
    density_weights: &[f64],
    omega: &[usize],
    budget: usize,
) -> OptimizationResult {
    let num_blocks = tokens.len();

    // ── Edge cases ──────────────────────────────────────────────────────
    if num_blocks == 0 {
        return OptimizationResult {
            gamma: vec![],
            #[cfg(test)]
            cost: 0.0,
        };
    }

    debug_assert_eq!(tokens.len(), density_weights.len(), "tokens and density_weights must have the same length");
    for &pos in omega {
        debug_assert!(pos >= 1 && pos < num_blocks, "Ω position {pos} out of range [1, {num_blocks})");
    }

    // ── Prefix sums ─────────────────────────────────────────────────────
    let ps = PrefixSums::build(tokens, density_weights);

    // ── Build super-block boundaries from Ω ─────────────────────────────
    let mut boundaries = Vec::with_capacity(omega.len().saturating_add(2));
    boundaries.push(0_usize); // left sentinel
    boundaries.extend_from_slice(omega);
    boundaries.push(num_blocks); // right sentinel
    boundaries.sort_unstable();
    boundaries.dedup();

    let num_superblocks = boundaries.len().saturating_sub(1);

    if budget == 0 {
        #[cfg(test)]
        let cost: f64 = boundaries
            .windows(2)
            .map(|win| {
                let left = win.first().copied().unwrap_or(0);
                let right = win.get(1).copied().unwrap_or(0);
                ps.block_cost(left, right)
            })
            .sum();
        return OptimizationResult {
            gamma: vec![],
            #[cfg(test)]
            cost,
        };
    }

    // ── Phase 1: per-super-block DP ─────────────────────────────────────
    let mut sb_costs: Vec<Vec<f64>> = Vec::with_capacity(num_superblocks);
    let mut sb_cuts: Vec<Vec<Vec<usize>>> = Vec::with_capacity(num_superblocks);

    for sb_idx in 0..num_superblocks {
        let left = boundaries.get(sb_idx).copied().unwrap_or(0);
        let right = boundaries.get(sb_idx.saturating_add(1)).copied().unwrap_or(0);
        let max_cuts = budget.min(right.saturating_sub(left).saturating_sub(1));

        let (costs, cuts) = solve_superblock(left, right, max_cuts, &ps);
        sb_costs.push(costs);
        sb_cuts.push(cuts);
    }

    // ── Phase 2: knapsack across super-blocks ───────────────────────────
    let dp_rows = num_superblocks.saturating_add(1);
    let dp_cols = budget.saturating_add(1);
    let mut dp_table = vec![vec![f64::INFINITY; dp_cols]; dp_rows];
    let mut choice = vec![vec![0_usize; dp_cols]; dp_rows];

    if let Some(row) = dp_table.get_mut(0)
        && let Some(cell) = row.get_mut(0)
    {
        *cell = 0.0;
    }

    for blk in 1..=num_superblocks {
        let sb = blk.saturating_sub(1);
        let max_j_this = sb_costs.get(sb).map_or(0, |costs| costs.len().saturating_sub(1));

        for total_j in 0..=budget {
            for jk in 0..=total_j.min(max_j_this) {
                let prev_j = total_j.saturating_sub(jk);
                let prev_cost = dp_table
                    .get(blk.saturating_sub(1))
                    .and_then(|row| row.get(prev_j))
                    .copied()
                    .unwrap_or(f64::INFINITY);
                let sb_cost = sb_costs.get(sb).and_then(|costs| costs.get(jk)).copied().unwrap_or(f64::INFINITY);
                let candidate = prev_cost + sb_cost;

                let current = dp_table.get(blk).and_then(|row| row.get(total_j)).copied().unwrap_or(f64::INFINITY);

                if candidate < current {
                    if let Some(row) = dp_table.get_mut(blk)
                        && let Some(cell) = row.get_mut(total_j)
                    {
                        *cell = candidate;
                    }
                    if let Some(row) = choice.get_mut(blk)
                        && let Some(cell) = row.get_mut(total_j)
                    {
                        *cell = jk;
                    }
                }
            }
        }
    }

    // Find the best total number of cuts ≤ budget
    let mut best_j = 0_usize;
    let mut best_cost = dp_table.get(num_superblocks).and_then(|row| row.first()).copied().unwrap_or(f64::INFINITY);

    for total_j in 1..=budget {
        let cost = dp_table.get(num_superblocks).and_then(|row| row.get(total_j)).copied().unwrap_or(f64::INFINITY);
        if cost < best_cost {
            best_cost = cost;
            best_j = total_j;
        }
    }

    // ── Reconstruct Γ ───────────────────────────────────────────────────
    let mut gamma: Vec<usize> = Vec::with_capacity(best_j);
    let mut remaining_j = best_j;

    for blk in (1..=num_superblocks).rev() {
        let jk = choice.get(blk).and_then(|row| row.get(remaining_j)).copied().unwrap_or(0);
        if jk > 0 {
            let sb = blk.saturating_sub(1);
            if let Some(cuts) = sb_cuts.get(sb).and_then(|c| c.get(jk)) {
                gamma.extend_from_slice(cuts);
            }
        }
        remaining_j = remaining_j.saturating_sub(jk);
    }

    gamma.sort_unstable();

    OptimizationResult {
        gamma,
        #[cfg(test)]
        cost: best_cost,
    }
}

// ─── Internals ──────────────────────────────────────────────────────────────

/// Normalize density weights to a probability distribution.
/// Falls back to uniform if weights sum to zero or are degenerate.
fn normalize_density(weights: &[f64]) -> Vec<f64> {
    let total: f64 = weights.iter().copied().sum();
    if total <= 0.0 || !total.is_finite() {
        let len = weights.len();
        if len == 0 {
            return vec![];
        }
        let uniform = 1.0 / len.to_f64();
        return vec![uniform; len];
    }
    weights.iter().map(|&w| w.max(0.0) / total).collect()
}

/// Solve the per-super-block DP for `(left_bound, right_bound]` with up to
/// `max_cuts` internal cuts.
///
/// Returns `(costs, cuts)` where:
/// - `costs[j]` = minimum cost with exactly `j` cuts
/// - `cuts[j]` = the cut positions achieving that minimum
fn solve_superblock(
    left_bound: usize,
    right_bound: usize,
    max_cuts: usize,
    ps: &PrefixSums,
) -> (Vec<f64>, Vec<Vec<usize>>) {
    let mut costs = vec![f64::INFINITY; max_cuts.saturating_add(1)];
    let mut best_cuts: Vec<Vec<usize>> = vec![vec![]; max_cuts.saturating_add(1)];

    // Base case: 0 cuts = cost of the entire sub-block
    if let Some(slot) = costs.get_mut(0) {
        *slot = ps.block_cost(left_bound, right_bound);
    }

    let width = right_bound.saturating_sub(left_bound);
    if width <= 1 || max_cuts == 0 {
        return (costs, best_cuts);
    }

    // Candidate cut positions: left_bound+1 .. right_bound-1
    let candidates: Vec<usize> = (left_bound.saturating_add(1)..right_bound).collect();
    let nc = candidates.len();

    if nc == 0 {
        return (costs, best_cuts);
    }

    // h_table[j][c] = min prefix cost of covering (left_bound, candidates[c]]
    //                 with exactly j cuts, rightmost cut at candidates[c].
    // parent_table[j][c] = predecessor candidate index for backtracking.
    let mut h_table = vec![vec![f64::INFINITY; nc]; max_cuts.saturating_add(1)];
    let mut parent_table: Vec<Vec<Option<usize>>> = vec![vec![None; nc]; max_cuts.saturating_add(1)];

    // Base: j = 1 — single cut at each candidate
    if let Some(h_row) = h_table.get_mut(1) {
        for (c_idx, &pos) in candidates.iter().enumerate() {
            if let Some(cell) = h_row.get_mut(c_idx) {
                *cell = ps.block_cost(left_bound, pos);
            }
        }
    }

    // Fill: j = 2..=max_cuts
    for cut_count in 2..=max_cuts {
        let min_c = cut_count.saturating_sub(1);
        for c_idx in min_c..nc {
            let pos = candidates.get(c_idx).copied().unwrap_or(0);
            let min_prev = cut_count.saturating_sub(2);
            for prev_idx in min_prev..c_idx {
                let prev_pos = candidates.get(prev_idx).copied().unwrap_or(0);
                let prev_h = h_table
                    .get(cut_count.saturating_sub(1))
                    .and_then(|row| row.get(prev_idx))
                    .copied()
                    .unwrap_or(f64::INFINITY);
                let candidate_cost = prev_h + ps.block_cost(prev_pos, pos);

                let current = h_table.get(cut_count).and_then(|row| row.get(c_idx)).copied().unwrap_or(f64::INFINITY);

                if candidate_cost < current {
                    if let Some(row) = h_table.get_mut(cut_count)
                        && let Some(cell) = row.get_mut(c_idx)
                    {
                        *cell = candidate_cost;
                    }
                    if let Some(row) = parent_table.get_mut(cut_count)
                        && let Some(cell) = row.get_mut(c_idx)
                    {
                        *cell = Some(prev_idx);
                    }
                }
            }
        }
    }

    // Collect: for each j, find the best rightmost cut
    for cut_count in 1..=max_cuts {
        let min_c = cut_count.saturating_sub(1);
        for c_idx in min_c..nc {
            let pos = candidates.get(c_idx).copied().unwrap_or(0);
            let prefix_cost = h_table.get(cut_count).and_then(|row| row.get(c_idx)).copied().unwrap_or(f64::INFINITY);
            let total = prefix_cost + ps.block_cost(pos, right_bound);

            let current_best = costs.get(cut_count).copied().unwrap_or(f64::INFINITY);
            if total < current_best {
                if let Some(slot) = costs.get_mut(cut_count) {
                    *slot = total;
                }

                // Backtrack to recover cut positions
                let mut positions = Vec::with_capacity(cut_count);
                let mut trace = c_idx;
                for jj in (1..=cut_count).rev() {
                    if let Some(&cand) = candidates.get(trace) {
                        positions.push(cand);
                    }
                    if let Some(prev) = parent_table.get(jj).and_then(|row| row.get(trace)).copied().flatten() {
                        trace = prev;
                    }
                }
                positions.reverse();
                if let Some(slot) = best_cuts.get_mut(cut_count) {
                    *slot = positions;
                }
            }
        }
    }

    (costs, best_cuts)
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "cache_optimizer_tests.rs"]
mod tests;
