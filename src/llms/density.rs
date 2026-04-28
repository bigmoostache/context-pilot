//! Divergence density distributions for cache breakpoint optimization.
//!
//! A divergence density models *where* in the prompt the content is most likely
//! to change between consecutive LLM requests. The optimizer uses these weights
//! to concentrate breakpoints where divergence probability is highest, minimizing
//! expected recompute cost.
//!
//! # Provided implementations
//!
//! | Density | Weight formula | Use case |
//! |---------|---------------|----------|
//! | [`UniformDensity`] | `w_i = 1` | Baseline / no prior |
//! | [`QuadraticDensity`] | `w_i = i²` | Default — later panels change more often |
//! | [`PowerLawDensity`] | `w_i = i^α` | Tunable tail-heaviness |
//! | [`EmpiricalDensity`] | `w_i = counts_i + ε` | Learned from observed divergence history |

use std::fmt::Debug;

use cp_base::cast::Safe as _;

use super::ApiMessage;

// ─── Trait ──────────────────────────────────────────────────────────────────

/// A probability distribution over block positions modeling where the prompt
/// is most likely to diverge from the cached version.
///
/// Implementations return **un-normalized** weights — the optimizer normalizes
/// them internally. This avoids redundant divisions and lets densities focus
/// purely on shape.
pub(crate) trait DivergenceDensity: Send + Sync + Debug {
    /// Return un-normalized divergence weights for `num_blocks` blocks.
    ///
    /// The returned `Vec` must have length `num_blocks`. Each `w[i]` represents
    /// the relative likelihood that block `i+1` (1-indexed) is the first block
    /// to differ from the cached prompt. Values must be non-negative and finite.
    fn weights(&self, num_blocks: usize) -> Vec<f64>;
}

// ─── Uniform ────────────────────────────────────────────────────────────────

/// Every block is equally likely to diverge.
///
/// Useful as a baseline or when no prior information is available.
#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct UniformDensity;

#[cfg(test)]
impl DivergenceDensity for UniformDensity {
    fn weights(&self, num_blocks: usize) -> Vec<f64> {
        vec![1.0; num_blocks]
    }
}

// ─── Quadratic ──────────────────────────────────────────────────────────────

/// Later blocks are quadratically more likely to diverge: `w_i = i²`.
///
/// This is the default density. Rationale: panels near the end of the context
/// (recent tool results, scratchpad, todos) change far more often than early
/// panels (system prompt, library, tool definitions).
#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct QuadraticDensity;

#[cfg(test)]
impl DivergenceDensity for QuadraticDensity {
    fn weights(&self, num_blocks: usize) -> Vec<f64> {
        // w[i] = (i+1)² where i is 0-indexed → block (i+1) is 1-indexed
        (1..=num_blocks)
            .map(|i| {
                let val = i.to_f64();
                val * val
            })
            .collect()
    }
}

// ─── Conversation Tail ──────────────────────────────────────────────────────

/// Tail-heavy density that zeroes out assistant-side blocks and past conversation,
/// concentrating divergence probability on user-side panels and the last user turn.
///
/// # Weight rules
///
/// | Region | Block source | Weight |
/// |--------|-------------|--------|
/// | Panel region | `role="user"` (`ToolResult`) | `i²` (quadratic) |
/// | Panel region | `role="assistant"` (`ToolUse`) | `0` |
/// | Conversation | Last `role="user"` message | `i²` (quadratic) |
/// | Conversation | Everything else | `0` |
///
/// Rationale: Anthropic's cache always extends up to a user-side boundary.
/// Assistant messages are deterministic (our own output) and past conversation
/// is immutable history. Only user-side panels (which may refresh) and the
/// latest user turn (the actual new content) have non-zero divergence probability.
#[derive(Debug, Clone)]
pub(crate) struct ConversationTailDensity {
    /// Per-block mask: `true` = apply quadratic weight, `false` = zero weight.
    hot_blocks: Vec<bool>,
}

impl ConversationTailDensity {
    /// Build the density from the full prompt's `api_messages`.
    ///
    /// Walks the message array to identify:
    /// 1. Panel user-side blocks (role="user" with `panel_*` tool results) → hot
    /// 2. The last `role="user"` message in the conversation → hot
    /// 3. Everything else → cold (zero density)
    pub(crate) fn from_api_messages(api_messages: &[ApiMessage]) -> Self {
        let total_blocks: usize = api_messages.iter().map(|m| m.content.len()).sum();
        let mut hot_blocks = vec![false; total_blocks];

        // Find the index of the last role="user" message
        let last_user_msg_idx = api_messages.iter().rposition(|m| m.role == "user");

        let mut block_offset = 0_usize;
        for (msg_idx, msg) in api_messages.iter().enumerate() {
            let is_user = msg.role == "user";
            let is_last_user = last_user_msg_idx == Some(msg_idx);
            let is_panel_user = is_user && Self::is_panel_message(msg);

            for blk_idx in 0..msg.content.len() {
                let global_idx = block_offset.saturating_add(blk_idx);
                if let Some(slot) = hot_blocks.get_mut(global_idx) {
                    // Hot if: panel user-side block OR last user message in conversation
                    *slot = is_panel_user || is_last_user;
                }
            }
            block_offset = block_offset.saturating_add(msg.content.len());
        }

        Self { hot_blocks }
    }

    /// Check if a user message is a panel injection (contains `ToolResult` with `panel_*` id).
    fn is_panel_message(msg: &ApiMessage) -> bool {
        msg.content.iter().any(|block| {
            matches!(block, super::ContentBlock::ToolResult { tool_use_id, .. }
                if tool_use_id.starts_with("panel_"))
        })
    }
}

impl DivergenceDensity for ConversationTailDensity {
    fn weights(&self, num_blocks: usize) -> Vec<f64> {
        (1..=num_blocks)
            .map(|i| {
                let is_hot = self.hot_blocks.get(i.saturating_sub(1)).copied().unwrap_or(false);
                if is_hot {
                    let val = i.to_f64();
                    val * val
                } else {
                    0.0
                }
            })
            .collect()
    }
}

// ─── Power Law ──────────────────────────────────────────────────────────────

/// Generalized power-law density: `w_i = i^α`.
///
/// - `α = 0` → uniform
/// - `α = 2` → quadratic (same as [`QuadraticDensity`])
/// - `α > 2` → increasingly tail-heavy (concentrates cuts at the end)
/// - `α < 0` → head-heavy (concentrates cuts at the beginning)
#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct PowerLawDensity {
    /// The exponent. Typical range: `[0.5, 4.0]`.
    pub alpha: f64,
}

#[cfg(test)]
impl DivergenceDensity for PowerLawDensity {
    fn weights(&self, num_blocks: usize) -> Vec<f64> {
        (1..=num_blocks).map(|i| i.to_f64().powf(self.alpha)).collect()
    }
}

// ─── Empirical ──────────────────────────────────────────────────────────────

/// Density learned from observed divergence counts with Laplace smoothing.
///
/// Each entry in `counts` records how many times that block position was
/// observed as the first divergent block. Laplace smoothing (`ε`) ensures
/// no position has zero probability, preventing the optimizer from ignoring
/// blocks that simply haven't been observed yet.
///
/// If `counts` is shorter than `num_blocks`, missing positions get weight `ε`.
/// If longer, excess entries are ignored.
#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct EmpiricalDensity {
    /// Raw divergence counts per block position (0-indexed: `counts[i]` = count
    /// for block `i+1`).
    pub counts: Vec<u32>,
    /// Laplace smoothing parameter. Must be positive. Default: `1.0`.
    pub smoothing: f64,
}

#[cfg(test)]
impl DivergenceDensity for EmpiricalDensity {
    fn weights(&self, num_blocks: usize) -> Vec<f64> {
        let epsilon = self.smoothing.max(f64::MIN_POSITIVE);
        (0..num_blocks)
            .map(|idx| {
                let count = self.counts.get(idx).copied().unwrap_or(0);
                count.to_f64() + epsilon
            })
            .collect()
    }
}

// ─── Factory (test + future use) ────────────────────────────────────────────

/// Selectable density kind for configuration.
///
/// Currently used by tests and the optimizer integration (PR 4).
/// Gated behind `#[cfg(test)]` until the integration PR lands.
#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) enum DensityKind {
    /// Flat prior — every position equally likely.
    Uniform,
    /// Quadratic tail-heavy — default for v3.
    Quadratic,
    /// Tunable power law.
    PowerLaw {
        /// The exponent. Typical range: `[0.5, 4.0]`.
        alpha: f64,
    },
    /// Learned from observation history.
    Empirical {
        /// Raw divergence counts per block position.
        counts: Vec<u32>,
        /// Laplace smoothing parameter. Must be positive.
        smoothing: f64,
    },
}

#[cfg(test)]
impl DensityKind {
    /// Build a concrete density from the selected kind.
    pub(crate) fn build(&self) -> Box<dyn DivergenceDensity> {
        match self {
            Self::Uniform => Box::new(UniformDensity),
            Self::Quadratic => Box::new(QuadraticDensity),
            Self::PowerLaw { alpha } => Box::new(PowerLawDensity { alpha: *alpha }),
            Self::Empirical { counts, smoothing } => {
                Box::new(EmpiricalDensity { counts: counts.clone(), smoothing: *smoothing })
            }
        }
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Validate fundamental properties that ALL densities must satisfy.
    fn assert_density_properties(density: &dyn DivergenceDensity, num_blocks: usize) {
        let weights = density.weights(num_blocks);

        // Correct length
        assert_eq!(weights.len(), num_blocks, "{density:?}: expected {num_blocks} weights, got {}", weights.len());

        // All non-negative
        for (idx, &w_val) in weights.iter().enumerate() {
            assert!(w_val >= 0.0, "{density:?}: weight[{idx}] = {w_val} is negative");
        }

        // All finite
        for (idx, &w_val) in weights.iter().enumerate() {
            assert!(w_val.is_finite(), "{density:?}: weight[{idx}] = {w_val} is not finite");
        }

        // Sum is positive (normalizable)
        let total: f64 = weights.iter().sum();
        assert!(total > 0.0, "{density:?}: total weight {total} is not positive");

        // Normalized sum ≈ 1.0
        let normalized: Vec<f64> = weights.iter().map(|&w_val| w_val / total).collect();
        let norm_sum: f64 = normalized.iter().sum();
        assert!((norm_sum - 1.0).abs() < 1e-10, "{density:?}: normalized sum {norm_sum} != 1.0");
    }

    // ── Property tests for all densities ────────────────────────────────

    #[test]
    fn test_uniform_properties() {
        let density = UniformDensity;
        for num_blocks in [1, 5, 50, 200] {
            assert_density_properties(&density, num_blocks);
        }
    }

    #[test]
    fn test_quadratic_properties() {
        let density = QuadraticDensity;
        for num_blocks in [1, 5, 50, 200] {
            assert_density_properties(&density, num_blocks);
        }
    }

    #[test]
    fn test_power_law_properties() {
        for alpha in [0.0, 0.5, 1.0, 2.0, 3.0, 4.0] {
            let density = PowerLawDensity { alpha };
            for num_blocks in [1, 5, 50, 200] {
                assert_density_properties(&density, num_blocks);
            }
        }
    }

    #[test]
    fn test_empirical_properties() {
        let density = EmpiricalDensity { counts: vec![0, 3, 1, 0, 7, 2], smoothing: 1.0 };
        for num_blocks in [1, 5, 6, 10, 200] {
            assert_density_properties(&density, num_blocks);
        }
    }

    // ── Specific value tests ────────────────────────────────────────────

    #[test]
    fn test_uniform_values() {
        let weights = UniformDensity.weights(5);
        assert_eq!(weights, vec![1.0; 5]);
    }

    #[test]
    fn test_quadratic_values() {
        let weights = QuadraticDensity.weights(5);
        assert_eq!(weights, vec![1.0, 4.0, 9.0, 16.0, 25.0]);
    }

    #[test]
    fn test_power_law_alpha_zero_is_uniform() {
        let weights = PowerLawDensity { alpha: 0.0 }.weights(5);
        // i^0 = 1 for all i
        assert_eq!(weights, vec![1.0; 5]);
    }

    #[test]
    fn test_power_law_alpha_two_matches_quadratic() {
        let power = PowerLawDensity { alpha: 2.0 }.weights(10);
        let quad = QuadraticDensity.weights(10);
        for (p_val, q_val) in power.iter().zip(quad.iter()) {
            assert!((p_val - q_val).abs() < 1e-10, "PowerLaw(2) != Quadratic: {p_val} vs {q_val}");
        }
    }

    #[test]
    fn test_empirical_with_smoothing() {
        let density = EmpiricalDensity { counts: vec![0, 5, 10], smoothing: 1.0 };
        let weights = density.weights(5);
        // counts: [0, 5, 10, (missing), (missing)] + ε=1.0
        assert!((weights.first().copied().unwrap_or(0.0) - 1.0).abs() < 1e-10); // 0 + 1
        assert!((weights.get(1).copied().unwrap_or(0.0) - 6.0).abs() < 1e-10); // 5 + 1
        assert!((weights.get(2).copied().unwrap_or(0.0) - 11.0).abs() < 1e-10); // 10 + 1
        assert!((weights.get(3).copied().unwrap_or(0.0) - 1.0).abs() < 1e-10); // missing + 1
        assert!((weights.get(4).copied().unwrap_or(0.0) - 1.0).abs() < 1e-10); // missing + 1
    }

    #[test]
    fn test_empirical_zero_smoothing_floors_to_min_positive() {
        let density = EmpiricalDensity { counts: vec![0], smoothing: 0.0 };
        let weights = density.weights(1);
        // smoothing clamped to f64::MIN_POSITIVE, so weight > 0
        assert!(weights.first().copied().unwrap_or(0.0) > 0.0);
    }

    // ── Edge cases ──────────────────────────────────────────────────────

    #[test]
    fn test_zero_blocks() {
        assert!(UniformDensity.weights(0).is_empty());
        assert!(QuadraticDensity.weights(0).is_empty());
        assert!(PowerLawDensity { alpha: 2.0 }.weights(0).is_empty());
        assert!((EmpiricalDensity { counts: vec![1, 2, 3], smoothing: 1.0 }).weights(0).is_empty());
    }

    #[test]
    fn test_single_block() {
        for num_blocks in [1] {
            assert_density_properties(&UniformDensity, num_blocks);
            assert_density_properties(&QuadraticDensity, num_blocks);
            assert_density_properties(&PowerLawDensity { alpha: 3.0 }, num_blocks);
            assert_density_properties(&EmpiricalDensity { counts: vec![], smoothing: 1.0 }, num_blocks);
        }
    }

    // ── Monotonicity tests ──────────────────────────────────────────────

    #[test]
    fn test_quadratic_monotonically_increasing() {
        let weights = QuadraticDensity.weights(20);
        for pair in weights.windows(2) {
            let left = pair.first().copied().unwrap_or(0.0);
            let right = pair.get(1).copied().unwrap_or(0.0);
            assert!(right > left, "Quadratic not monotonically increasing: {left} >= {right}");
        }
    }

    #[test]
    fn test_power_law_positive_alpha_monotonically_increasing() {
        for alpha in [0.5, 1.0, 2.0, 3.0] {
            let weights = PowerLawDensity { alpha }.weights(20);
            for pair in weights.windows(2) {
                let left = pair.first().copied().unwrap_or(0.0);
                let right = pair.get(1).copied().unwrap_or(0.0);
                assert!(right > left, "PowerLaw({alpha}) not monotonically increasing: {left} >= {right}");
            }
        }
    }

    // ── Factory tests ───────────────────────────────────────────────────

    #[test]
    fn test_density_kind_builds_all_variants() {
        let kinds = [
            DensityKind::Uniform,
            DensityKind::Quadratic,
            DensityKind::PowerLaw { alpha: 2.0 },
            DensityKind::Empirical { counts: vec![1, 2, 3], smoothing: 1.0 },
        ];
        for kind in &kinds {
            let density = kind.build();
            assert_density_properties(density.as_ref(), 10);
        }
    }
}
