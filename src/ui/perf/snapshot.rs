//! Per-operation snapshot math: lifetime mean/variance/max plus the
//! recent-ring std, extracted from the raw counters held under lock.

use super::{OpRaw, OpSnapshot};
use cp_base::cast::Safe as _;
use cp_base::cast::float_math;

/// Compute one operation's display snapshot from its accumulated lifetime
/// counters (`total_us`, `count`, `sum_sq_us`, `max_us`) plus the recent-sample
/// ring. Lifetime stats (mean/variance/max, in µs) are exact over ALL samples
/// since reset; the ring only drives the legacy `std_ms` column. Population
/// variance: `E[x²] − E[x]²`, clamped at 0 against float error.
pub(super) fn compute_op_snapshot(raw: &OpRaw<'_>) -> OpSnapshot {
    let OpRaw { name, total_us, count, sum_sq_us, max_us, recent } = *raw;
    let recent_n = recent.len();

    // Legacy windowed std (ms) over the recent ring — kept for the op table.
    let recent_mean_us =
        if recent_n > 0 { float_math::div(recent.iter().sum::<u64>().to_f64(), recent_n.to_f64()) } else { 0.0f64 };
    let std_us = if recent_n > 1 {
        let variance = float_math::div(
            float_math::sum_iter(recent.iter().map(|&x| {
                let diff = float_math::sub(x.to_f64(), recent_mean_us);
                float_math::mul(diff, diff)
            })),
            recent_n.saturating_sub(1).to_f64(),
        );
        variance.sqrt()
    } else {
        0.0f64
    };

    // Lifetime mean + population variance over every recorded sample.
    let mean_us = if count > 0 { float_math::div(total_us.to_f64(), count.to_f64()) } else { 0.0f64 };
    let variance_us2 = if count > 0 {
        let mean_sq = float_math::div(sum_sq_us.to_f64(), count.to_f64());
        float_math::sub(mean_sq, float_math::mul(mean_us, mean_us)).max(0.0f64)
    } else {
        0.0f64
    };

    OpSnapshot {
        name,
        total_ms: float_math::div_u64(total_us, 1000.0f64),
        mean_ms: float_math::div(mean_us, 1000.0f64),
        std_ms: float_math::div(std_us, 1000.0f64),
        count,
        mean_us,
        variance_us2,
        max_us: max_us.to_f64(),
    }
}
