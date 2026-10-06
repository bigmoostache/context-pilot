//! Plain-text (ASCII) dump of the perf snapshot, copied to the clipboard by
//! Ctrl+R while the F12 overlay is open so its content can be pasted into an
//! LLM. Lists EVERY recorded op (the overlay only shows the top 10).

use super::PerfSnapshot;
use std::fmt::Write as _;

/// Render `snapshot` as a header plus one fixed-width row per op.
pub(crate) fn overlay_text(snapshot: &PerfSnapshot) -> String {
    let mut out = String::with_capacity(4096);
    let _head = writeln!(
        out,
        "Perf snapshot: frame {:.1}ms avg / {:.1}ms max, CPU {:.1}%, RAM {:.1} MB, FDs {}/{}, loops {}",
        snapshot.frame_avg_ms,
        snapshot.frame_max_ms,
        snapshot.cpu_usage,
        snapshot.memory_mb,
        snapshot.open_fds,
        snapshot.fd_limit_soft,
        snapshot.loop_count,
    );
    let _cols = writeln!(
        out,
        "{:<28} {:>9} {:>11} {:>14} {:>10} {:>10} {:>11}",
        "op", "samples", "mean_us", "variance_us2", "std_us", "max_us", "total_ms"
    );
    for op in &snapshot.ops {
        let _row = writeln!(
            out,
            "{:<28} {:>9} {:>11.1} {:>14.1} {:>10.1} {:>10.0} {:>11.1}",
            op.name,
            op.count,
            op.mean_us,
            op.variance_us2,
            op.variance_us2.sqrt(),
            op.max_us,
            op.total_ms,
        );
    }
    out
}
