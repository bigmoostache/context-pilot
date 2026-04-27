//! Cache optimization engine for intelligent Anthropic prompt cache breakpoint placement.
//!
//! Tracks accumulated content hashes across requests and places up to 4 `cache_control`
//! breakpoints to minimize expected cost under uniform divergence.
//!
//! Algorithm overview:
//! 1. Compute accumulated hashes for every content block (chain: `hash(block + prev_hash)`)
//! 2. Find the cache frontier (deepest stored breakpoint that matches current prompt)
//! 3. Place a beacon BP at `frontier + LOOKBACK_WINDOW` to extend the cached prefix
//! 4. Place 3 remaining BPs via optimal gap-decomposition DP to minimize expected cost

use cp_base::cast::Safe as _;
use serde::{Deserialize, Serialize};

use super::ApiMessage;

/// Anthropic's prompt cache lookback window: the API checks up to this many blocks
/// before the tagged position when searching for a cache match.
const LOOKBACK_WINDOW: usize = 20;

/// Cache TTL in milliseconds (5 minutes). Entries older than this are pruned
/// before each use, matching Anthropic's server-side cache eviction.
const CACHE_TTL_MS: u64 = 5 * 60 * 1000;

// ─── Data Types ─────────────────────────────────────────────────────────────

/// A stored breakpoint: accumulated hash + timestamp of last use.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BreakpointEntry {
    /// Accumulated hash at this breakpoint position.
    pub acc_hash: String,
    /// When this breakpoint was last sent (ms since epoch).
    pub timestamp_ms: u64,
}

/// Per-block metadata computed during hash chain construction.
#[derive(Debug, Clone)]
struct BlockInfo {
    /// Message index in the `api_messages` array.
    msg_idx: usize,
    /// Content block index within that message.
    blk_idx: usize,
    /// Accumulated hash up to and including this block.
    acc_hash: String,
    /// Token count for this individual block.
    _token_count: usize,
    /// Cumulative token count from block 0 through this block.
    cumulative_tokens: usize,
    /// Whether this block belongs to a user-role message.
    is_user_msg: bool,
}

/// Result of `compute_breakpoints`: positions to tag + metadata for post-request bookkeeping.
pub(crate) struct BreakpointPlan {
    /// `(msg_idx, blk_idx)` pairs to tag with `cache_control`.
    pub positions: Vec<(usize, usize)>,
    /// Accumulated hashes at each BP position (for `record_breakpoints`).
    pub bp_hashes: Vec<String>,
    /// How many stored BPs matched the current request's hash chain.
    pub alive_count: usize,
    /// Per-mille positions (0–1000) of alive BPs within the prompt.
    pub alive_positions_permille: Vec<u16>,
}

// ─── Engine ─────────────────────────────────────────────────────────────────

/// Cache optimization engine: tracks breakpoints across requests and computes
/// optimal placement for the next request.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct CacheEngine {
    /// Known breakpoints from previous requests.
    pub breakpoints: Vec<BreakpointEntry>,
}

impl CacheEngine {
    /// Deserialize from JSON (stored on `State.cache_engine_json`).
    pub(crate) fn from_json(json: &str) -> Self {
        serde_json::from_str(json).unwrap_or_default()
    }

    /// Serialize to JSON for persistence.
    pub(crate) fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Remove breakpoints older than the TTL window.
    pub(crate) fn prune(&mut self, now_ms: u64) {
        let cutoff = now_ms.saturating_sub(CACHE_TTL_MS);
        self.breakpoints.retain(|bp| bp.timestamp_ms >= cutoff);
    }

    /// Record breakpoint hashes from the most recent request.
    /// Refreshes timestamps for existing hashes, inserts new ones.
    pub(crate) fn record_breakpoints(&mut self, hashes: &[String], now_ms: u64) {
        for hash in hashes {
            if let Some(existing) = self.breakpoints.iter_mut().find(|bp| &bp.acc_hash == hash) {
                existing.timestamp_ms = now_ms;
            } else {
                self.breakpoints.push(BreakpointEntry {
                    acc_hash: hash.clone(),
                    timestamp_ms: now_ms,
                });
            }
        }
    }

    /// Compute optimal breakpoint positions for the given prompt.
    pub(crate) fn compute_breakpoints(&self, api_messages: &[ApiMessage]) -> BreakpointPlan {
        let block_infos = compute_accumulated_hashes(api_messages);
        if block_infos.is_empty() {
            return BreakpointPlan {
                positions: vec![],
                bp_hashes: vec![],
                alive_count: 0,
                alive_positions_permille: vec![],
            };
        }

        let total_tokens = block_infos
            .last()
            .map_or(0, |bi| bi.cumulative_tokens);

        // Phase 1: Find cache frontier (deepest matching stored BP)
        let frontier = self.find_cache_frontier(&block_infos);

        // Count alive BPs and compute their per-mille positions
        let mut alive_count: usize = 0;
        let mut alive_positions_permille: Vec<u16> = Vec::new();
        for stored_bp in &self.breakpoints {
            if let Some(pos) = block_infos
                .iter()
                .position(|bi| bi.acc_hash == stored_bp.acc_hash)
            {
                alive_count = alive_count.saturating_add(1);
                if total_tokens > 0 {
                    let permille = block_infos
                        .get(pos)
                        .map_or(0, |bi| bi.cumulative_tokens)
                        .saturating_mul(1000)
                        .checked_div(total_tokens)
                        .unwrap_or(0);
                    alive_positions_permille.push(permille.to_u16());
                }
            }
        }
        alive_positions_permille.sort_unstable();

        // Phase 2: Place beacon BP (extends cached prefix)
        let beacon_idx = place_beacon(&block_infos, frontier);

        // Phase 3: Collect alive BP block positions for gap decomposition
        let alive_block_positions: Vec<usize> = self
            .breakpoints
            .iter()
            .filter_map(|bp| {
                block_infos
                    .iter()
                    .position(|bi| bi.acc_hash == bp.acc_hash)
            })
            .collect();

        // Phase 4: Place 3 remaining BPs via optimal gap decomposition
        let remaining = place_remaining_bps(&block_infos, beacon_idx, &alive_block_positions);

        // Collect all BP positions and hashes
        let mut all_positions: Vec<(usize, usize)> = Vec::with_capacity(4);
        let mut bp_hashes: Vec<String> = Vec::with_capacity(4);

        if let Some(beacon) = beacon_idx
            && let Some(bi) = block_infos.get(beacon)
        {
            all_positions.push((bi.msg_idx, bi.blk_idx));
            bp_hashes.push(bi.acc_hash.clone());
        }
        for &pos in &remaining {
            if let Some(bi) = block_infos.get(pos) {
                all_positions.push((bi.msg_idx, bi.blk_idx));
                bp_hashes.push(bi.acc_hash.clone());
            }
        }

        BreakpointPlan {
            positions: all_positions,
            bp_hashes,
            alive_count,
            alive_positions_permille,
        }
    }

    /// Find the deepest block whose accumulated hash matches a stored breakpoint.
    fn find_cache_frontier(&self, block_infos: &[BlockInfo]) -> Option<usize> {
        let mut deepest: Option<usize> = None;
        for (idx, bi) in block_infos.iter().enumerate() {
            if self
                .breakpoints
                .iter()
                .any(|bp| bp.acc_hash == bi.acc_hash)
            {
                deepest = Some(idx);
            }
        }
        deepest
    }
}

// ─── Hash Chain ─────────────────────────────────────────────────────────────

/// Compute accumulated hashes for every content block in the prompt.
/// `acc_hash[i] = sha256(block_content + acc_hash[i-1])`
fn compute_accumulated_hashes(api_messages: &[ApiMessage]) -> Vec<BlockInfo> {
    let mut infos: Vec<BlockInfo> = Vec::new();
    let mut prev_hash = String::new();
    let mut cumulative_tokens: usize = 0;

    for (msg_idx, msg) in api_messages.iter().enumerate() {
        let is_user = msg.role == "user";
        for (blk_idx, block) in msg.content.iter().enumerate() {
            let block_text = match block {
                super::ContentBlock::Text { text } => text.as_str(),
                super::ContentBlock::ToolUse { name, .. } => name.as_str(),
                super::ContentBlock::ToolResult { content, .. } => content.as_str(),
            };

            let token_count = cp_base::state::context::estimate_tokens(block_text);
            cumulative_tokens = cumulative_tokens.saturating_add(token_count);

            let combined = format!("{block_text}{prev_hash}");
            let acc_hash = crate::state::cache::hash_content(&combined);

            infos.push(BlockInfo {
                msg_idx,
                blk_idx,
                acc_hash: acc_hash.clone(),
                _token_count: token_count,
                cumulative_tokens,
                is_user_msg: is_user,
            });

            prev_hash = acc_hash;
        }
    }

    infos
}

// ─── Beacon Placement ───────────────────────────────────────────────────────

/// Place the beacon BP at `frontier + LOOKBACK_WINDOW`, preferring user-message blocks.
/// Falls back to the last user-message block if no frontier exists.
fn place_beacon(block_infos: &[BlockInfo], frontier: Option<usize>) -> Option<usize> {
    if block_infos.is_empty() {
        return None;
    }

    let last_idx = block_infos.len().saturating_sub(1);

    let target = frontier.map_or_else(
        || {
            // No frontier: place at last user-message block (tail strategy)
            block_infos
                .iter()
                .rposition(|bi| bi.is_user_msg)
                .unwrap_or(last_idx)
        },
        |f| f.saturating_add(LOOKBACK_WINDOW).min(last_idx),
    );

    // Prefer a user-message block near the target (within a few blocks)
    let search_start = target.saturating_sub(3);
    let search_end = target.saturating_add(3).min(last_idx);

    let user_near = (search_start..=search_end)
        .rev()
        .find(|&idx| block_infos.get(idx).is_some_and(|bi| bi.is_user_msg));

    Some(user_near.unwrap_or(target))
}

// ─── Optimal BP Placement (Gap Decomposition + DP) ──────────────────────────

/// Place 3 remaining breakpoints using optimal gap-decomposition.
///
/// The problem: given existing BPs (beacon + alive), place 3 new BPs to minimize
/// expected cost under uniform divergence. Each BP at position `p` contributes
/// `T[p] × gap_to_right` to the objective. Gaps between existing BPs are
/// independent sub-problems, so we:
///
/// 1. Identify gaps between existing BPs (beacon + alive Ω)
/// 2. For each gap, compute optimal 1/2/3-BP placement via DP
/// 3. Distribute 3 BPs across gaps to maximize total gain
fn place_remaining_bps(
    block_infos: &[BlockInfo],
    beacon_idx: Option<usize>,
    alive_positions: &[usize],
) -> Vec<usize> {
    let block_count = block_infos.len();
    if block_count == 0 {
        return vec![];
    }

    // Build sorted set of all existing BP positions (sentinel 0 + alive + beacon)
    let mut existing: Vec<usize> =
        Vec::with_capacity(alive_positions.len().saturating_add(2));
    existing.push(0); // left sentinel
    existing.extend_from_slice(alive_positions);
    if let Some(beacon) = beacon_idx {
        existing.push(beacon);
    }
    existing.sort_unstable();
    existing.dedup();

    // Right sentinel: one past last valid index
    let right_sentinel = block_count;

    // Build gap list: (left_bp_pos, right_boundary)
    let mut gaps: Vec<(usize, usize)> = Vec::new();
    for idx in 0..existing.len() {
        let left = existing.get(idx).copied().unwrap_or(0);
        let right = existing
            .get(idx.saturating_add(1))
            .copied()
            .unwrap_or(right_sentinel);
        if right.saturating_sub(left) > 1 {
            gaps.push((left, right));
        }
    }

    if gaps.is_empty() {
        return vec![];
    }

    // For each gap, compute optimal placement for num_bps=1,2,3
    // gap_gains[gap_idx][num_bps] = (gain, positions)
    let mut gap_gains: Vec<[GapResult; 4]> = Vec::with_capacity(gaps.len());

    for &(left, right) in &gaps {
        gap_gains.push([
            GapResult::default(),                                // 0 BPs: no gain
            optimal_bps_in_gap(block_infos, left, right, 1),
            optimal_bps_in_gap(block_infos, left, right, 2),
            optimal_bps_in_gap(block_infos, left, right, 3),
        ]);
    }

    // Master problem: distribute 3 BPs across gaps to maximize total gain.
    // Since G is small (typically ≤ 10), enumerate all distributions.
    let mut best = MasterBest {
        gain: 0,
        allocation: vec![0; gaps.len()],
        gap_idx: 0,
    };
    let mut current_alloc = vec![0usize; gaps.len()];
    enumerate_bp_distributions(
        &EnumerationCtx {
            gap_gains: &gap_gains,
            num_gaps: gaps.len(),
        },
        3,
        &mut current_alloc,
        &mut best,
    );

    // Collect the BP positions from the optimal allocation
    let mut placed: Vec<usize> = Vec::with_capacity(3);
    for (gap_idx, &alloc) in best.allocation.iter().enumerate() {
        if alloc > 0
            && let Some(result) = gap_gains
                .get(gap_idx)
                .and_then(|gap_row| gap_row.get(alloc))
        {
            placed.extend_from_slice(&result.positions);
        }
    }

    placed
}

/// Result of optimal k-BP placement within a single gap.
#[derive(Debug, Clone, Default)]
struct GapResult {
    /// Gain over baseline (no BPs in this gap).
    gain: u64,
    /// Optimal BP positions within the gap.
    positions: Vec<usize>,
}

/// Context for the recursive enumeration (bundles read-only args to stay under 4 params).
struct EnumerationCtx<'ctx> {
    /// Per-gap optimal results for 0..3 BPs.
    gap_gains: &'ctx [[GapResult; 4]],
    /// Total number of gaps.
    num_gaps: usize,
}

/// Tracks the best allocation found during master-problem enumeration.
struct MasterBest {
    /// Best total gain found so far.
    gain: u64,
    /// Per-gap BP count that achieves the best gain.
    allocation: Vec<usize>,
    /// Current recursion depth (which gap we're distributing to).
    gap_idx: usize,
}

/// Recursively enumerate all distributions of `remaining` BPs across gaps.
fn enumerate_bp_distributions(
    ctx: &EnumerationCtx<'_>,
    remaining: usize,
    current: &mut Vec<usize>,
    best: &mut MasterBest,
) {
    if best.gap_idx == ctx.num_gaps {
        if remaining == 0 {
            let total: u64 = current
                .iter()
                .enumerate()
                .map(|(gi, &num_bps)| {
                    ctx.gap_gains
                        .get(gi)
                        .and_then(|row| row.get(num_bps))
                        .map_or(0, |r| r.gain)
                })
                .sum();
            if total > best.gain {
                best.gain = total;
                best.allocation.clone_from(current);
            }
        }
        return;
    }

    // Remaining gaps after this one
    let gaps_left = ctx.num_gaps.saturating_sub(best.gap_idx).saturating_sub(1);
    let max_here = remaining.min(3); // each gap takes at most 3

    for num_bps in 0..=max_here {
        // Prune: remaining gaps must be able to absorb the rest
        if remaining.saturating_sub(num_bps) > gaps_left.saturating_mul(3) {
            continue;
        }
        if let Some(slot) = current.get_mut(best.gap_idx) {
            *slot = num_bps;
        }
        best.gap_idx = best.gap_idx.saturating_add(1);
        enumerate_bp_distributions(
            ctx,
            remaining.saturating_sub(num_bps),
            current,
            best,
        );
        best.gap_idx = best.gap_idx.saturating_sub(1);
    }
}

/// Find optimal placement of `num_bps` BPs within a single gap `(left, right)`.
///
/// Uses DP: `dp[m][j]` = best "prefix value" using m BPs from candidates, rightmost at j.
///
/// Total gap value with BPs q₁ < q₂ < ... < qₖ:
///   `T[L]×(q₁-L) + T[q₁]×(q₂-q₁) + ... + T[qₖ]×(R-qₖ)`
///
/// We maximize the GAIN over baseline (no BPs): `T[L]×(R-L)`.
fn optimal_bps_in_gap(
    block_infos: &[BlockInfo],
    left: usize,
    right: usize,
    num_bps: usize,
) -> GapResult {
    // Candidate positions within gap: (left+1, left+2, ..., right-1)
    let candidates: Vec<usize> = (left.saturating_add(1)..right).collect();
    let nc = candidates.len();

    if nc == 0 || num_bps == 0 {
        return GapResult::default();
    }
    if num_bps > nc {
        // Can't place more BPs than candidates; place at all candidates
        let gain = gap_value_with_bps(block_infos, left, right, &candidates)
            .saturating_sub(gap_baseline(block_infos, left, right));
        return GapResult {
            gain,
            positions: candidates,
        };
    }

    let cum =
        |pos: usize| -> u64 { block_infos.get(pos).map_or(0, |bi| bi.cumulative_tokens as u64) };

    // val(a, b) = T[a] × (b - a) = contribution of BP at `a` covering gap to its right `b`
    let val = |a_pos: usize, b_pos: usize| -> u64 {
        cum(a_pos).saturating_mul(b_pos.saturating_sub(a_pos) as u64)
    };

    // dp[m][j] = best "prefix value" using m BPs from candidates[0..=j], rightmost = j
    // prefix_value = T[L]×(cand[0]-L) + ... + T[cand[j-1]]×(cand[j]-cand[j-1])
    //              (everything except the final right-tail T[cand[j]]×(R-cand[j]))
    // At collection time we add the right-tail for the chosen rightmost.
    //
    // Base case (m=1): dp[1][j] = T[L] × (candidates[j] - L)
    // Transition (m>1): dp[m][j] = max_{i<j} { dp[m-1][i] + T[cand[i]] × (cand[j] - cand[i]) }
    // Final answer: max_j { dp[num_bps][j] + T[cand[j]] × (right - cand[j]) }

    let mut dp = vec![vec![0u64; nc]; num_bps.saturating_add(1)];
    let mut parent: Vec<Vec<Option<usize>>> = vec![vec![None; nc]; num_bps.saturating_add(1)];

    // Base case: m = 1
    for (col, &cand) in candidates.iter().enumerate() {
        if let Some(row) = dp.get_mut(1)
            && let Some(cell) = row.get_mut(col)
        {
            *cell = val(left, cand);
        }
    }

    // Fill DP for m = 2..num_bps
    for bp_count in 2..=num_bps {
        let min_col = bp_count.saturating_sub(1);
        for col in min_col..nc {
            let cand_col = candidates.get(col).copied().unwrap_or(0);
            let prev_min = bp_count.saturating_sub(2);
            for prev in prev_min..col {
                let cand_prev = candidates.get(prev).copied().unwrap_or(0);
                let prev_dp = dp
                    .get(bp_count.saturating_sub(1))
                    .and_then(|row| row.get(prev).copied())
                    .unwrap_or(0);
                let candidate_val = prev_dp.saturating_add(val(cand_prev, cand_col));
                let current_dp = dp
                    .get(bp_count)
                    .and_then(|row| row.get(col).copied())
                    .unwrap_or(0);
                if candidate_val > current_dp {
                    if let Some(cell) = dp
                        .get_mut(bp_count)
                        .and_then(|row| row.get_mut(col))
                    {
                        *cell = candidate_val;
                    }
                    if let Some(cell) = parent
                        .get_mut(bp_count)
                        .and_then(|row| row.get_mut(col))
                    {
                        *cell = Some(prev);
                    }
                }
            }
        }
    }

    // Find best rightmost position for num_bps BPs
    let mut best_val = 0u64;
    let mut best_col: usize = 0;
    let min_final_col = num_bps.saturating_sub(1);
    for col in min_final_col..nc {
        let cand_col = candidates.get(col).copied().unwrap_or(0);
        let dp_val = dp
            .get(num_bps)
            .and_then(|row| row.get(col).copied())
            .unwrap_or(0);
        let total = dp_val.saturating_add(val(cand_col, right));
        if total > best_val {
            best_val = total;
            best_col = col;
        }
    }

    // Trace back to recover BP positions
    let mut positions: Vec<usize> = Vec::with_capacity(num_bps);
    let mut trace_col = best_col;
    for bp_count in (1..=num_bps).rev() {
        let cand = candidates.get(trace_col).copied().unwrap_or(0);
        positions.push(cand);
        if let Some(prev) = parent
            .get(bp_count)
            .and_then(|row| row.get(trace_col).copied())
            .flatten()
        {
            trace_col = prev;
        }
    }
    positions.reverse();

    let baseline = gap_baseline(block_infos, left, right);
    let gain = best_val.saturating_sub(baseline);

    GapResult { gain, positions }
}

/// Baseline value of a gap with no BPs: `T[left] × (right - left)`.
fn gap_baseline(block_infos: &[BlockInfo], left: usize, right: usize) -> u64 {
    let cum_left = block_infos
        .get(left)
        .map_or(0, |bi| bi.cumulative_tokens as u64);
    cum_left.saturating_mul(right.saturating_sub(left) as u64)
}

/// Compute total gap value with specific BP positions placed.
/// `Value = T[L]×(q₁-L) + T[q₁]×(q₂-q₁) + ... + T[qₖ]×(R-qₖ)`
fn gap_value_with_bps(
    block_infos: &[BlockInfo],
    left: usize,
    right: usize,
    bps: &[usize],
) -> u64 {
    let cum =
        |pos: usize| -> u64 { block_infos.get(pos).map_or(0, |bi| bi.cumulative_tokens as u64) };

    if bps.is_empty() {
        return cum(left).saturating_mul(right.saturating_sub(left) as u64);
    }

    // Left boundary → first BP
    let first_bp = bps.first().copied().unwrap_or(left);
    let mut total = cum(left).saturating_mul(first_bp.saturating_sub(left) as u64);

    // Internal segments via sliding window
    for window in bps.windows(2) {
        let &[bp_a, bp_b] = window else { continue };
        total =
            total.saturating_add(cum(bp_a).saturating_mul(bp_b.saturating_sub(bp_a) as u64));
    }

    // Last BP → right boundary
    let last_bp = bps.last().copied().unwrap_or(left);
    total.saturating_add(cum(last_bp).saturating_mul(right.saturating_sub(last_bp) as u64))
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a simple text content block.
    fn make_text_block(text: &str) -> super::super::ContentBlock {
        super::super::ContentBlock::Text {
            text: text.to_string(),
        }
    }

    /// Helper: create N alternating user/assistant messages, one text block each.
    fn make_messages(block_count: usize) -> Vec<ApiMessage> {
        let mut msgs: Vec<ApiMessage> = Vec::new();
        for idx in 0..block_count {
            let role = if idx % 2 == 0 { "user" } else { "assistant" };
            msgs.push(ApiMessage {
                role: role.to_string(),
                content: vec![make_text_block(&format!("block_{idx}"))],
            });
        }
        msgs
    }

    #[test]
    fn test_accumulated_hashes_are_chained() {
        let msgs = make_messages(3);
        let infos = compute_accumulated_hashes(&msgs);
        assert_eq!(infos.len(), 3);
        // Each hash should be different (chained)
        assert_ne!(infos[0].acc_hash, infos[1].acc_hash);
        assert_ne!(infos[1].acc_hash, infos[2].acc_hash);
        // Cumulative tokens should be non-decreasing
        assert!(infos[0].cumulative_tokens <= infos[1].cumulative_tokens);
        assert!(infos[1].cumulative_tokens <= infos[2].cumulative_tokens);
    }

    #[test]
    fn test_same_content_produces_same_hash() {
        let msgs1 = make_messages(3);
        let msgs2 = make_messages(3);
        let infos1 = compute_accumulated_hashes(&msgs1);
        let infos2 = compute_accumulated_hashes(&msgs2);
        for (hash_a, hash_b) in infos1.iter().zip(infos2.iter()) {
            assert_eq!(hash_a.acc_hash, hash_b.acc_hash);
        }
    }

    #[test]
    fn test_prune_removes_old_entries() {
        let mut engine = CacheEngine::default();
        let now = 1_000_000_u64;
        engine.breakpoints.push(BreakpointEntry {
            acc_hash: "old".to_string(),
            timestamp_ms: now.saturating_sub(CACHE_TTL_MS).saturating_sub(1),
        });
        engine.breakpoints.push(BreakpointEntry {
            acc_hash: "fresh".to_string(),
            timestamp_ms: now,
        });
        engine.prune(now);
        assert_eq!(engine.breakpoints.len(), 1);
        assert_eq!(engine.breakpoints[0].acc_hash, "fresh");
    }

    #[test]
    fn test_frontier_detection() {
        let msgs = make_messages(10);
        let infos = compute_accumulated_hashes(&msgs);

        let mut engine = CacheEngine::default();
        // Store hash at position 4
        engine.breakpoints.push(BreakpointEntry {
            acc_hash: infos[4].acc_hash.clone(),
            timestamp_ms: 999_999,
        });

        let frontier = engine.find_cache_frontier(&infos);
        assert_eq!(frontier, Some(4));
    }

    #[test]
    fn test_beacon_placement_after_frontier() {
        let msgs = make_messages(40);
        let infos = compute_accumulated_hashes(&msgs);

        // Frontier at block 10 → beacon should be near block 30 (10 + 20)
        let beacon = place_beacon(&infos, Some(10));
        assert!(beacon.is_some());
        let beacon_pos = beacon.unwrap();
        assert!(
            beacon_pos >= 27 && beacon_pos <= 33,
            "beacon at {beacon_pos}, expected near 30 (±3 user-message search)"
        );
    }

    #[test]
    fn test_no_frontier_falls_back_to_tail() {
        let msgs = make_messages(10);
        let infos = compute_accumulated_hashes(&msgs);

        let beacon = place_beacon(&infos, None);
        assert!(beacon.is_some());
        let beacon_pos = beacon.unwrap();
        assert!(beacon_pos >= 7, "beacon at {beacon_pos}, expected near tail");
    }

    #[test]
    fn test_remaining_bps_cover_different_zones() {
        let msgs = make_messages(80);
        let infos = compute_accumulated_hashes(&msgs);

        let beacon = Some(70_usize);
        let alive_positions = vec![]; // no alive BPs besides beacon
        let remaining = place_remaining_bps(&infos, beacon, &alive_positions);

        // Should place up to 3 BPs
        assert!(!remaining.is_empty());
        assert!(remaining.len() <= 3);

        // BPs should be spread out (not all clustered at the tail)
        if remaining.len() >= 2 {
            let mut sorted = remaining.clone();
            sorted.sort_unstable();
            // With 80 blocks, BPs should NOT all be in the last 20% (positions > 64)
            let in_last_20_pct = sorted.iter().filter(|&&pos| pos > 64).count();
            assert!(
                in_last_20_pct < sorted.len(),
                "all BPs clustered in tail: {sorted:?}"
            );
        }
    }

    #[test]
    fn test_record_and_retrieve() {
        let mut engine = CacheEngine::default();
        let hashes = vec!["hash_a".to_string(), "hash_b".to_string()];
        engine.record_breakpoints(&hashes, 1_000_000);
        assert_eq!(engine.breakpoints.len(), 2);

        // Recording same hash again should refresh, not duplicate
        engine.record_breakpoints(&["hash_a".to_string()], 2_000_000);
        assert_eq!(engine.breakpoints.len(), 2);
        assert_eq!(
            engine
                .breakpoints
                .iter()
                .find(|bp| bp.acc_hash == "hash_a")
                .unwrap()
                .timestamp_ms,
            2_000_000
        );
    }

    #[test]
    fn test_serialization_roundtrip() {
        let mut engine = CacheEngine::default();
        engine.breakpoints.push(BreakpointEntry {
            acc_hash: "test_hash".to_string(),
            timestamp_ms: 12345,
        });

        let json = engine.to_json();
        let restored = CacheEngine::from_json(&json);
        assert_eq!(restored.breakpoints.len(), 1);
        assert_eq!(restored.breakpoints[0].acc_hash, "test_hash");
        assert_eq!(restored.breakpoints[0].timestamp_ms, 12345);
    }

    #[test]
    fn test_empty_prompt() {
        let engine = CacheEngine::default();
        let plan = engine.compute_breakpoints(&[]);
        assert!(plan.positions.is_empty());
        assert!(plan.bp_hashes.is_empty());
    }

    #[test]
    fn test_full_pipeline() {
        let msgs = make_messages(60);
        let infos = compute_accumulated_hashes(&msgs);

        // Simulate: previous request cached up to block 20
        let mut engine = CacheEngine::default();
        engine.record_breakpoints(&[infos[20].acc_hash.clone()], 999_000);

        let plan = engine.compute_breakpoints(&msgs);

        // Should have breakpoints placed
        assert!(!plan.positions.is_empty());
        assert!(plan.positions.len() <= 4);

        // Beacon should be around block 40 (20 + 20)
        let has_near_40 = plan
            .positions
            .iter()
            .any(|(msg_idx, _)| *msg_idx >= 30 && *msg_idx <= 45);
        assert!(
            has_near_40,
            "expected a BP near block 40, got {:?}",
            plan.positions
        );
    }

    #[test]
    fn test_optimal_placement_spreads_bps() {
        // Uniform token distribution: each block has ~equal tokens.
        // With a gap of (0, 100), optimal 3-BP placement should spread evenly.
        let msgs = make_messages(100);
        let infos = compute_accumulated_hashes(&msgs);

        // No beacon, no alive BPs — pure optimal placement test
        let result = place_remaining_bps(&infos, None, &[]);

        assert_eq!(result.len(), 3);
        let mut sorted = result.clone();
        sorted.sort_unstable();

        // With 100 blocks, BPs should be roughly at 25/50/75 (not all at 80+)
        // At minimum, the first BP should be before position 50
        assert!(
            sorted[0] < 50,
            "first BP should be in first half, got {sorted:?}"
        );
        // And the last shouldn't be at the very end
        assert!(
            sorted[2] < 95,
            "last BP too close to tail, got {sorted:?}"
        );
    }

    #[test]
    fn test_optimal_beats_greedy_counterexample() {
        // The counterexample from our analysis: uniform tokens, gap (0, 100).
        // Greedy places at 50, then 75, then ~87.
        // Optimal should place closer to 33, 67, ~83 (better total gain).
        let msgs = make_messages(100);
        let infos = compute_accumulated_hashes(&msgs);

        let result = place_remaining_bps(&infos, None, &[]);
        let mut sorted = result.clone();
        sorted.sort_unstable();

        // Compute the actual objective value using gap_value_with_bps
        let val_optimal = gap_value_with_bps(&infos, 0, 100, &sorted);

        // Compare with greedy-style placement (50, 75, 87)
        let greedy_bps = vec![50, 75, 87];
        let val_greedy = gap_value_with_bps(&infos, 0, 100, &greedy_bps);

        assert!(
            val_optimal >= val_greedy,
            "optimal ({val_optimal}) should be >= greedy ({val_greedy}). Placements: optimal={sorted:?}, greedy={greedy_bps:?}"
        );
    }
}
