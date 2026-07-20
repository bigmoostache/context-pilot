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
use cp_base::cast::float_math;

/// Per-super-block forward DP (split out for the 500-line structure limit).
mod superblock_dp;

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
        let prob = superblock_dp::normalize_density(density_weights);

        let mut big_t = vec![0.0f64; num_blocks.saturating_add(1)];
        let mut s_p = vec![0.0f64; num_blocks.saturating_add(1)];
        let mut s_pt = vec![0.0f64; num_blocks.saturating_add(1)];

        for idx in 1..=num_blocks {
            let tok = tokens.get(idx.saturating_sub(1)).copied().unwrap_or(0);
            let p_val = prob.get(idx.saturating_sub(1)).copied().unwrap_or(0.0f64);
            let prev_t = big_t.get(idx.saturating_sub(1)).copied().unwrap_or(0.0f64);
            let prev_sp = s_p.get(idx.saturating_sub(1)).copied().unwrap_or(0.0f64);
            let prev_spt = s_pt.get(idx.saturating_sub(1)).copied().unwrap_or(0.0f64);

            let cur_t = float_math::add(prev_t, tok.to_f64());
            if let Some(slot) = big_t.get_mut(idx) {
                *slot = cur_t;
            }
            if let Some(slot) = s_p.get_mut(idx) {
                *slot = float_math::add(prev_sp, p_val);
            }
            if let Some(slot) = s_pt.get_mut(idx) {
                *slot = prev_spt.mul_add(1.0, float_math::mul(p_val, cur_t));
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
        let cum_pt_right = self.s_pt.get(right).copied().unwrap_or(0.0f64);
        let cum_pt_left = self.s_pt.get(left).copied().unwrap_or(0.0f64);
        let tok_left = self.big_t.get(left).copied().unwrap_or(0.0f64);
        let cum_prob_right = self.s_p.get(right).copied().unwrap_or(0.0f64);
        let cum_prob_left = self.s_p.get(left).copied().unwrap_or(0.0f64);

        tok_left.mul_add(float_math::sub(cum_prob_left, cum_prob_right), float_math::sub(cum_pt_right, cum_pt_left))
    }
}

/// Per-super-block DP results: `costs[sb][j]` = min cost of super-block `sb`
/// with exactly `j` internal cuts; `cuts[sb][j]` = the achieving cut positions.
type SuperblockSolutions = (Vec<Vec<f64>>, Vec<Vec<Vec<usize>>>);

/// Phase 1 — solve each super-block `(boundaries[i], boundaries[i+1]]`
/// independently via split DP, capped at `budget` cuts per block.
fn solve_all_superblocks(boundaries: &[usize], budget: usize, ps: &PrefixSums) -> SuperblockSolutions {
    let num_superblocks = boundaries.len().saturating_sub(1);
    let mut sb_costs: Vec<Vec<f64>> = Vec::with_capacity(num_superblocks);
    let mut sb_cuts: Vec<Vec<Vec<usize>>> = Vec::with_capacity(num_superblocks);
    for sb_idx in 0..num_superblocks {
        let left = boundaries.get(sb_idx).copied().unwrap_or(0);
        let right = boundaries.get(sb_idx.saturating_add(1)).copied().unwrap_or(0);
        let max_cuts = budget.min(right.saturating_sub(left).saturating_sub(1));
        let (costs, cuts) = superblock_dp::solve_superblock(left, right, max_cuts, ps);
        sb_costs.push(costs);
        sb_cuts.push(cuts);
    }
    (sb_costs, sb_cuts)
}

/// Knapsack DP tables across super-blocks: `dp[blk][j]` = min total cost using
/// the first `blk` super-blocks with `j` total cuts; `choice[blk][j]` = the
/// number of cuts assigned to super-block `blk` in that optimum.
type KnapsackTables = (Vec<Vec<f64>>, Vec<Vec<usize>>);

/// Target cell + candidate for a knapsack relaxation step.
struct KnapCell {
    /// Super-block row index (1-based) in the DP table.
    blk: usize,
    /// Total cut budget column being relaxed.
    total_j: usize,
    /// Cuts assigned to this super-block in the candidate.
    jk: usize,
    /// Candidate total cost.
    candidate: f64,
}

/// Relax knapsack cell `[cell.blk][cell.total_j]` to `cell.candidate` cost with
/// `cell.jk` cuts for this super-block, when it improves on the current value.
fn relax_knapsack_cell(dp_table: &mut [Vec<f64>], choice: &mut [Vec<usize>], cell: &KnapCell) {
    let KnapCell { blk, total_j, jk, candidate } = *cell;
    let current = dp_table.get(blk).and_then(|row| row.get(total_j)).copied().unwrap_or(f64::INFINITY);
    if candidate >= current {
        return;
    }
    if let Some(slot) = dp_table.get_mut(blk).and_then(|row| row.get_mut(total_j)) {
        *slot = candidate;
    }
    if let Some(slot) = choice.get_mut(blk).and_then(|row| row.get_mut(total_j)) {
        *slot = jk;
    }
}

/// Phase 2 — knapsack across super-blocks: distribute `budget` cuts to minimize
/// total cost, tracking each block's cut allocation for reconstruction.
fn knapsack_across(sb_costs: &[Vec<f64>], budget: usize) -> KnapsackTables {
    let num_superblocks = sb_costs.len();
    let dp_rows = num_superblocks.saturating_add(1);
    let dp_cols = budget.saturating_add(1);
    let mut dp_table = vec![vec![f64::INFINITY; dp_cols]; dp_rows];
    let mut choice = vec![vec![0usize; dp_cols]; dp_rows];

    if let Some(cell) = dp_table.get_mut(0).and_then(|row| row.get_mut(0)) {
        *cell = 0.0f64;
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
                relax_knapsack_cell(
                    &mut dp_table,
                    &mut choice,
                    &KnapCell { blk, total_j, jk, candidate: float_math::add(prev_cost, sb_cost) },
                );
            }
        }
    }
    (dp_table, choice)
}

/// Minimum cost across all total-cut counts `0..=budget` in the final DP row.
#[cfg(test)]
fn best_total_cost(dp_table: &[Vec<f64>], num_superblocks: usize, budget: usize) -> f64 {
    let mut best = dp_table.get(num_superblocks).and_then(|row| row.first()).copied().unwrap_or(f64::INFINITY);
    for total_j in 1..=budget {
        let cost = dp_table.get(num_superblocks).and_then(|row| row.get(total_j)).copied().unwrap_or(f64::INFINITY);
        if cost < best {
            best = cost;
        }
    }
    best
}

/// Find the cheapest total-cut count ≤ `budget`, then backtrack the knapsack
/// `choice` table to collect the chosen breakpoint positions (sorted).
fn reconstruct_gamma(
    dp_table: &[Vec<f64>],
    choice: &[Vec<usize>],
    sb_cuts: &[Vec<Vec<usize>>],
    budget: usize,
) -> Vec<usize> {
    let num_superblocks = sb_cuts.len();
    let mut best_j = 0usize;
    let mut best_cost = dp_table.get(num_superblocks).and_then(|row| row.first()).copied().unwrap_or(f64::INFINITY);
    for total_j in 1..=budget {
        let cost = dp_table.get(num_superblocks).and_then(|row| row.get(total_j)).copied().unwrap_or(f64::INFINITY);
        if cost < best_cost {
            best_cost = cost;
            best_j = total_j;
        }
    }

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
    gamma
}

/// Debug-assert the optimizer's input invariants (lengths match, Ω in range).
#[cfg(debug_assertions)]
fn debug_check_inputs(tokens: &[u32], density_weights: &[f64], omega: &[usize], num_blocks: usize) {
    debug_assert_eq!(tokens.len(), density_weights.len(), "tokens and density_weights must have the same length");
    for &pos in omega {
        debug_assert!(pos >= 1 && pos < num_blocks, "Ω position {pos} out of range [1, {num_blocks})");
    }
}

/// Cost of the boundary partition with no new cuts (budget == 0), summed over
/// consecutive super-blocks. Only needed for the test-facing cost field.
#[cfg(test)]
fn zero_budget_cost(boundaries: &[usize], ps: &PrefixSums) -> f64 {
    boundaries
        .windows(2)
        .map(|win| {
            let left = win.first().copied().unwrap_or(0);
            let right = win.get(1).copied().unwrap_or(0);
            ps.block_cost(left, right)
        })
        .sum()
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
    if num_blocks == 0 {
        return OptimizationResult {
            gamma: vec![],
            #[cfg(test)]
            cost: 0.0,
        };
    }

    #[cfg(debug_assertions)]
    debug_check_inputs(tokens, density_weights, omega, num_blocks);

    let ps = PrefixSums::build(tokens, density_weights);

    // ── Build super-block boundaries from Ω ─────────────────────────────
    let mut boundaries = Vec::with_capacity(omega.len().saturating_add(2));
    boundaries.push(0usize); // left sentinel
    boundaries.extend_from_slice(omega);
    boundaries.push(num_blocks); // right sentinel
    boundaries.sort_unstable();
    boundaries.dedup();

    if budget == 0 {
        return OptimizationResult {
            gamma: vec![],
            #[cfg(test)]
            cost: zero_budget_cost(&boundaries, &ps),
        };
    }

    let (sb_costs, sb_cuts) = solve_all_superblocks(&boundaries, budget, &ps);
    let (dp_table, choice) = knapsack_across(&sb_costs, budget);
    let gamma = reconstruct_gamma(&dp_table, &choice, &sb_cuts, budget);

    OptimizationResult {
        gamma,
        #[cfg(test)]
        cost: best_total_cost(&dp_table, sb_cuts.len(), budget),
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "../cache_optimizer_tests.rs"]
mod tests;
