//! Cache optimization engine for intelligent Anthropic prompt cache breakpoint placement.
//!
//! Tracks accumulated content hashes across requests and places up to 4 `cache_control`
//! breakpoints to minimize expected recompute cost under divergence density weighting.
//!
//! # Algorithm (v3 — DP optimizer)
//!
//! 1. Compute accumulated hashes for every content block (chain: `hash(block + prev_hash)`)
//! 2. Find the cache frontier (deepest stored breakpoint that matches current prompt)
//! 3. Place a beacon BP at `frontier + LOOKBACK_WINDOW` to extend the cached prefix
//! 4. Collect alive BPs (stored entries matching current hash chain) into Ω
//! 5. Run the exact two-level DP optimizer ([`super::cache_optimizer::optimize_gamma`])
//!    with budget K=3 to place 3 additional BPs minimizing expected recompute cost
//! 6. Tag beacon + Γ positions with `cache_control` (up to 4 total)

use cp_base::cast::Safe as _;
use serde::{Deserialize, Serialize};

use super::ApiMessage;

/// Anthropic's prompt cache lookback window: the API checks up to this many blocks
/// before the tagged position when searching for a cache match.
const LOOKBACK_WINDOW: usize = 20;

/// Cache TTL in milliseconds (5 minutes). Entries older than this are pruned
/// before each use, matching Anthropic's server-side cache eviction.
const CACHE_TTL_MS: u64 = 5 * 60 * 1000;

/// Optimizer budget: always 3 new breakpoints. The 4th slot is the beacon.
const OPTIMIZER_BUDGET: usize = 3;

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
    token_count: usize,
    /// Cumulative token count from block 0 through this block.
    cumulative_tokens: usize,
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
                self.breakpoints.push(BreakpointEntry { acc_hash: hash.clone(), timestamp_ms: now_ms });
            }
        }
    }

    /// Compute optimal breakpoint positions for the given prompt.
    ///
    /// Uses the v3 two-level DP optimizer with quadratic divergence density.
    /// Places a beacon at `frontier + LOOKBACK_WINDOW`, then runs the optimizer
    /// with alive BPs + beacon as fixed boundaries (Ω) and budget K=3.
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

        let num_blocks = block_infos.len();
        let last_idx = num_blocks.saturating_sub(1);
        let total_tokens = block_infos.last().map_or(0, |bi| bi.cumulative_tokens);

        // ── Alive BPs: stored entries matching current hash chain ────────
        let alive_block_positions: Vec<usize> = self
            .breakpoints
            .iter()
            .filter_map(|bp| block_infos.iter().position(|bi| bi.acc_hash == bp.acc_hash))
            .collect();

        let alive_count = alive_block_positions.len();
        let mut alive_positions_permille: Vec<u16> = if total_tokens > 0 {
            alive_block_positions
                .iter()
                .map(|&pos| {
                    let permille = block_infos
                        .get(pos)
                        .map_or(0, |bi| bi.cumulative_tokens)
                        .saturating_mul(1000)
                        .checked_div(total_tokens)
                        .unwrap_or(0);
                    permille.to_u16()
                })
                .collect()
        } else {
            vec![]
        };
        alive_positions_permille.sort_unstable();

        // ── Beacon: extend cached prefix past the frontier ──────────────
        let frontier = self.find_cache_frontier(&block_infos);
        let beacon_idx = frontier.map_or(last_idx, |f| f.saturating_add(LOOKBACK_WINDOW).min(last_idx));

        // ── Build Ω (1-indexed): alive BP positions + beacon ────────────
        let mut omega: Vec<usize> = alive_block_positions
            .iter()
            .map(|&pos| pos.saturating_add(1)) // 0-indexed → 1-indexed
            .filter(|&pos| pos >= 1 && pos < num_blocks) // valid range for optimizer
            .collect();
        let beacon_one_indexed = beacon_idx.saturating_add(1);
        if beacon_one_indexed >= 1 && beacon_one_indexed < num_blocks {
            omega.push(beacon_one_indexed);
        }
        omega.sort_unstable();
        omega.dedup();

        // ── Build optimizer inputs ──────────────────────────────────────
        let tok_counts: Vec<u32> = block_infos.iter().map(|bi| bi.token_count.to_u32()).collect();
        let density = super::density::ConversationTailDensity::from_api_messages(api_messages);
        let density_weights = super::density::DivergenceDensity::weights(&density, num_blocks);

        // ── Run the DP optimizer ────────────────────────────────────────
        let result = super::cache_optimizer::optimize_gamma(&tok_counts, &density_weights, &omega, OPTIMIZER_BUDGET);

        // ── Convert Γ (1-indexed) → 0-indexed ──────────────────────────
        let gamma_0idx: Vec<usize> = result.gamma.iter().map(|&pos| pos.saturating_sub(1)).collect();

        // ── Collect tag positions: beacon ∪ Γ (up to 4 total) ───────────
        let mut tag_positions: Vec<usize> = Vec::with_capacity(4);
        tag_positions.push(beacon_idx);
        tag_positions.extend_from_slice(&gamma_0idx);
        tag_positions.sort_unstable();
        tag_positions.dedup();

        // ── Build output ────────────────────────────────────────────────
        let mut positions: Vec<(usize, usize)> = Vec::with_capacity(tag_positions.len());
        let mut bp_hashes: Vec<String> = Vec::with_capacity(tag_positions.len());

        for &pos in &tag_positions {
            if let Some(bi) = block_infos.get(pos) {
                positions.push((bi.msg_idx, bi.blk_idx));
                bp_hashes.push(bi.acc_hash.clone());
            }
        }

        BreakpointPlan { positions, bp_hashes, alive_count, alive_positions_permille }
    }

    /// Find the deepest block whose accumulated hash matches a stored breakpoint.
    fn find_cache_frontier(&self, block_infos: &[BlockInfo]) -> Option<usize> {
        let mut deepest: Option<usize> = None;
        for (idx, bi) in block_infos.iter().enumerate() {
            if self.breakpoints.iter().any(|bp| bp.acc_hash == bi.acc_hash) {
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
        for (blk_idx, block) in msg.content.iter().enumerate() {
            let hash_repr = match block {
                super::ContentBlock::Text { text } => text.clone(),
                super::ContentBlock::ToolUse { id, name, input } => {
                    format!("tool_use:{id}:{name}:{}", serde_json::to_string(input).unwrap_or_default())
                }
                super::ContentBlock::ToolResult { tool_use_id, content } => {
                    format!("tool_result:{tool_use_id}:{content}")
                }
            };

            let token_count = cp_base::state::context::estimate_tokens(&hash_repr);
            cumulative_tokens = cumulative_tokens.saturating_add(token_count);

            let combined = format!("{hash_repr}{prev_hash}");
            let acc_hash = crate::state::cache::hash_content(&combined);

            infos.push(BlockInfo { msg_idx, blk_idx, acc_hash: acc_hash.clone(), token_count, cumulative_tokens });

            prev_hash = acc_hash;
        }
    }

    infos
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "cache_engine_tests.rs"]
mod tests;
