//! Per-operation snapshot math: lifetime mean/variance/max from the raw
//! counters, plus the recent-ring std (computed only for displayed rows).

use super::{OpRaw, OpSnapshot};
use cp_base::cast::Safe as _;
use cp_base::cast::float_math;

/// Compute one operation's display snapshot from its accumulated lifetime
/// counters (`total_us`, `count`, `sum_sq_us`, `max_us`). Lifetime stats
/// (mean/variance/max, in µs) are exact over ALL samples since reset.
/// Population variance: `E[x²] − E[x]²`, clamped at 0 against float error.
/// `std_ms` starts at 0: [`ring_std_ms`] fills it for the rows the F12 table
/// shows, so the other ops never copy their sample ring.
pub(super) fn compute_op_snapshot(raw: &OpRaw) -> OpSnapshot {
    let OpRaw { name, total_us, count, sum_sq_us, max_us } = *raw;

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
        std_ms: 0.0f64,
        count,
        mean_us,
        variance_us2,
        max_us: max_us.to_f64(),
    }
}

/// Sample std (ms) over the recent-sample ring — the legacy windowed `Std`
/// column of the F12 op table. 0 with fewer than two samples.
pub(super) fn ring_std_ms(recent: &[u64]) -> f64 {
    let recent_n = recent.len();
    if recent_n < 2 {
        return 0.0f64;
    }
    let mean_us = float_math::div(recent.iter().sum::<u64>().to_f64(), recent_n.to_f64());
    let variance = float_math::div(
        float_math::sum_iter(recent.iter().map(|&x| {
            let diff = float_math::sub(x.to_f64(), mean_us);
            float_math::mul(diff, diff)
        })),
        recent_n.saturating_sub(1).to_f64(),
    );
    float_math::div(variance.sqrt(), 1000.0f64)
}
