//! Context Radar YAML rendering — split from `radar.rs` for the line budget.
//!
//! Formats the scored + ranked log results and the task-signal anchors into the
//! YAML string the panel displays.

use std::fmt::Write as _;

use crate::radar::ScoredResult;

/// Append YAML lines for a single radar entry.
fn write_entry(yaml: &mut String, entry: &ScoredResult) {
    let _w0 = writeln!(yaml, "  - content: \"{}\"", entry.content.replace('"', "\\\""));
    let _w1 = writeln!(yaml, "    datetime: \"{}\"", entry.datetime);
    let _w2 = writeln!(yaml, "    importance: {}", entry.importance);
    let _w4 = writeln!(yaml, "    score: {:.3}", entry.score);
}

/// Format a millisecond timestamp as ISO 8601, or `"unknown"` if zero/out-of-range.
fn format_timestamp_ms(ms: u64) -> String {
    if ms == 0 {
        return "unknown".to_owned();
    }
    i64::try_from(ms).ok().and_then(cp_mod_utilities::time::epoch_ms_to_rfc3339).unwrap_or_else(|| "unknown".to_owned())
}

/// Build the radar YAML: header + anchors (signals) + ranked results.
pub(crate) fn build_radar_yaml(ranked: &[ScoredResult], signals: &[crate::types::TaskSignal]) -> String {
    let mut yaml = String::with_capacity(4096);
    let _h0 = writeln!(yaml, "# Context Radar — {} results from {} signals", ranked.len(), signals.len());
    let _h1 = writeln!(yaml, "# Half-life: {:.0} logs", crate::radar::HALF_LIFE_LOGS);

    // Show all task signals as anchors with timestamps (most recent first)
    if !signals.is_empty() {
        let _h2 = writeln!(yaml, "anchors:");
        for sig in signals.iter().rev() {
            let datetime = format_timestamp_ms(sig.timestamp_ms);
            let _h3 = writeln!(yaml, "  - time: \"{datetime}\"");
            let _h4 = writeln!(yaml, "    signal: \"{}\"", sig.content.replace('"', "\\\""));
        }
    }

    if ranked.is_empty() {
        let _h4 = writeln!(yaml, "# No matching logs found");
    } else {
        let _h5 = writeln!(yaml, "results:");
        for entry in ranked {
            write_entry(&mut yaml, entry);
        }
    }
    yaml
}
