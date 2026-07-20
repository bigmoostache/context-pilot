//! Float arithmetic chokepoint: sole audited site for `f32`/`f64` math.
//!
//! `clippy::float_arithmetic` is `deny` workspace-wide, so every `+ - * /` on a
//! float funnels through this module (mirrors the `time_arith` integer-division
//! chokepoint in `panels.rs`). Helpers are domain-named — token/char/page
//! conversion, USD cost, bar-fill percentages, easing curves, and telemetry
//! statistics — so the arithmetic is centralized and testable rather than
//! sprayed across the render/telemetry layer.
//!
//! The primitives (`add`, `sub`, `mul`, `div`, `ratio`, `percent`, `fma`, …)
//! are deliberately **unguarded** — they reproduce the exact expression the
//! call site used to write inline, so routing through them changes no runtime
//! behavior. Domain guards (`if den > 0`, `if !empty`) stay at the call site.
//! Only the statistical/definitional helpers (`mean`, `variance`, `exp_decay`)
//! carry their own empty/degenerate-input guard, since that guard *is* their
//! definition.
#![expect(
    clippy::float_arithmetic,
    reason = "sole chokepoint for irreducible f32/f64 arithmetic — token/char/page conversion, USD cost, bar fills, easing, and telemetry statistics"
)]

use super::Safe as _;

// ── Token / char / page conversion ──────────────────────────────────

/// Ceil of `numer / divisor`, saturating back to `usize`.
///
/// Token estimation: `ceil(char_len / CHARS_PER_TOKEN)`.
#[inline]
#[must_use]
pub fn ceil_ratio(numer: usize, divisor: f32) -> usize {
    (numer.to_f32() / divisor).ceil().to_usize()
}

/// Widen `value` to `f32` and multiply by `factor` (per-page char span).
#[inline]
#[must_use]
pub fn scale(value: usize, factor: f32) -> f32 {
    value.to_f32() * factor
}

/// Multiply `value` by `factor`, saturating the product back to `usize`.
#[inline]
#[must_use]
pub fn scale_to_usize(value: usize, factor: f32) -> usize {
    (value.to_f32() * factor).to_usize()
}

/// Multiply two `f32` values (cache-price multipliers).
#[inline]
#[must_use]
pub const fn mul_f32(a: f32, b: f32) -> f32 {
    a * b
}

/// Add two `f32` values (threshold-slider nudges).
#[inline]
#[must_use]
pub const fn add_f32(a: f32, b: f32) -> f32 {
    a + b
}

/// Subtract `b` from `a` in `f32` (threshold-slider nudges).
#[inline]
#[must_use]
pub const fn sub_f32(a: f32, b: f32) -> f32 {
    a - b
}

// ── USD cost ────────────────────────────────────────────────────────

/// USD cost for `tokens` at `price_per_mtok` (price per million tokens, `f32`).
#[inline]
#[must_use]
pub fn cost_usd(tokens: usize, price_per_mtok: f32) -> f64 {
    tokens.to_f64() * price_per_mtok.to_f64() / 1_000_000.0
}

/// USD cost from an already-widened `f64` price (streaming path).
#[inline]
#[must_use]
pub fn cost_usd_f64(tokens: usize, price_per_mtok: f64) -> f64 {
    tokens.to_f64() * price_per_mtok / 1_000_000.0
}

// ── Unguarded `f64` primitives (exact inline-expression replacements) ─

/// `a + b`. Cost/DP accumulation.
#[inline]
#[must_use]
pub const fn add(a: f64, b: f64) -> f64 {
    a + b
}

/// `a - b`. Distance / delta.
#[inline]
#[must_use]
pub const fn sub(a: f64, b: f64) -> f64 {
    a - b
}

/// `a * b`.
#[inline]
#[must_use]
pub const fn mul(a: f64, b: f64) -> f64 {
    a * b
}

/// `num / den` (unguarded — caller guards `den == 0`).
#[inline]
#[must_use]
pub const fn div(num: f64, den: f64) -> f64 {
    num / den
}

/// `num / den` ratio (alias of [`div`], reads as a proportion at call sites).
#[inline]
#[must_use]
pub const fn ratio(num: f64, den: f64) -> f64 {
    num / den
}

/// `num / den × 100` percentage (unguarded — caller guards `den == 0`).
#[inline]
#[must_use]
pub const fn percent(num: f64, den: f64) -> f64 {
    num / den * 100.0
}

/// Fused multiply-add `factor × mul + add` (`factor.mul_add(mul, add)`).
#[inline]
#[must_use]
pub const fn fma(factor: f64, multiplicand: f64, addend: f64) -> f64 {
    factor.mul_add(multiplicand, addend)
}

/// `|a - b|` — epsilon comparisons.
#[inline]
#[must_use]
pub fn abs_diff(a: f64, b: f64) -> f64 {
    (a - b).abs()
}

/// Sum of three `f64` values (hit + miss + output cost total).
#[inline]
#[must_use]
pub const fn sum3(first: f64, second: f64, third: f64) -> f64 {
    first + second + third
}

/// Sum an iterator of `f64` (frame samples, weight totals).
#[inline]
#[must_use]
pub fn sum_iter<I>(iter: I) -> f64
where
    I: IntoIterator<Item = f64>,
{
    iter.into_iter().fold(0.0f64, |acc, x| acc + x)
}

// ── Bars, ratios over integer widths ────────────────────────────────

/// Bar fill cells from a `pct` (0–100) over `width` cells.
#[inline]
#[must_use]
pub fn bar_fill(pct: f64, width: usize) -> usize {
    (pct / 100.0 * width.to_f64()).to_usize()
}

/// Bar fill cells from a `ratio` (0–1) over `width` cells.
#[inline]
#[must_use]
pub fn fill_from_ratio(ratio: f64, width: usize) -> usize {
    (ratio * width.to_f64()).to_usize()
}

/// Uniform weight `1 / len` (unguarded — caller guards `len == 0`).
#[inline]
#[must_use]
pub fn uniform(len: usize) -> f64 {
    1.0 / len.to_f64()
}

/// Scale a `u64` count to `f64` then divide by a constant `divisor`
/// (`ms→s`, `ticks→s`; unguarded — caller guards `divisor == 0`).
#[inline]
#[must_use]
pub fn div_u64(value: u64, divisor: f64) -> f64 {
    value.to_f64() / divisor
}

// ── Animation / easing ──────────────────────────────────────────────

/// Linear interpolation `from + (to - from) × t`.
#[inline]
#[must_use]
pub fn lerp(from: f64, to: f64, t: f64) -> f64 {
    (to - from).mul_add(t, from)
}

/// Cubic ease-out `1 - (1 - t)³`.
#[inline]
#[must_use]
pub fn ease_out_cubic(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(3)
}

/// Fractional phase of `elapsed / period` (wraps to `0.0..1.0`).
#[inline]
#[must_use]
pub fn fract_phase(elapsed: f64, period: f64) -> f64 {
    (elapsed / period).fract()
}

/// Pulse `1 + amplitude × sin(phase × τ)` for animated highlights.
#[inline]
#[must_use]
pub fn pulse(amplitude: f64, phase: f64) -> f64 {
    amplitude.mul_add((phase * core::f64::consts::TAU).sin(), 1.0)
}

// ── Statistics / decay (self-guarding — the guard is the definition) ─

/// Exponential decay with floor: `0.5 + 0.5 × exp(-ln2 × age / half_life)`.
///
/// Decays from `1.0` (age 0) to a floor of `0.5`. `1.0` when `half_life <= 0`.
#[inline]
#[must_use]
pub fn exp_decay(age: f64, half_life: f64) -> f64 {
    if half_life <= 0.0f64 {
        return 1.0f64;
    }
    0.5f64.mul_add((-f64::ln(2.0f64) * age / half_life).exp(), 0.5f64)
}

/// Arithmetic mean of a slice, `0.0` when empty.
#[inline]
#[must_use]
pub fn mean(samples: &[f64]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    sum_iter(samples.iter().copied()) / samples.len().to_f64()
}

/// Bessel-corrected variance about `mean_val` over `count` samples (divides by
/// `count - 1`). `0.0` when `count <= 1`.
#[inline]
#[must_use]
pub fn variance(samples: &[f64], mean_val: f64, count: usize) -> f64 {
    if count <= 1 {
        return 0.0;
    }
    let ss = sum_iter(samples.iter().map(|&x| {
        let diff = x - mean_val;
        diff * diff
    }));
    ss / count.saturating_sub(1).to_f64()
}
