use crate::state::State;
use std::io::Write as _;

/// TSV log path for per-tick cost tracking.
const COST_TSV_PATH: &str = ".context-pilot/logs/cost-tracking.tsv";

/// TSV column header (written once when the file is first created).
const HEADER: &str = "datetime\tbefore_three_last_tools\tbefore_culprit_type\tbefore_tokens_before_culprit\tbefore_tokens_culprit\tbefore_tokens_after_culprit\tqueue_is_active\ttempo_is_active\tbreak_kind\tbefore_culprit_max_freezes\tafter_tokens_hit\tafter_cost_hit\tafter_tokens_miss\tafter_cost_miss\tafter_tokens_out\tafter_cost_out";

/// Append a row to the cost-tracking TSV, combining beginning-of-tick telemetry
/// (culprit data captured in `prepare_stream_context`) with end-of-tick costs
/// (available after `accumulate_pending_token_stats` or `apply_token_usage`).
///
/// Consumes `state.tick_telemetry` (takes it, leaving `None`). No-op if telemetry
/// was never populated (e.g. reverie ticks that skip `prepare_stream_context`).
pub(crate) fn append_cost_tsv(state: &mut State) {
    let Some(tel) = state.tick_telemetry.take() else {
        return;
    };

    // Epoch-millisecond timestamp (raw, unambiguous, sortable — consumer formats)
    let datetime = tel.tick_start_ms;

    let line = format!(
        "{datetime}\t{tools}\t{culprit}\t{before}\t{culp_tok}\t{after}\t{queue}\t{tempo}\t{break_kind}\t{max_freezes}\t{hit_tok}\t{hit_cost:.6}\t{miss_tok}\t{miss_cost:.6}\t{out_tok}\t{out_cost:.6}",
        tools = tel.three_last_tools,
        culprit = tel.culprit_type,
        before = tel.tokens_before_culprit,
        culp_tok = tel.tokens_culprit,
        after = tel.tokens_after_culprit,
        queue = tel.queue_is_active,
        tempo = tel.tempo_is_active,
        break_kind = tel.break_kind.as_tsv(),
        max_freezes = tel.culprit_max_freezes,
        hit_tok = state.tick_cache_hit_tokens,
        hit_cost = state.tick_cost_hit_usd,
        miss_tok = state.tick_cache_miss_tokens,
        miss_cost = state.tick_cost_miss_usd,
        out_tok = state.tick_output_tokens,
        out_cost = state.tick_cost_output_usd,
    );

    // Best-effort append — telemetry must never block the pipeline
    drop(append_line(&line));
}

/// Append a single line to the TSV file, creating it with headers if absent.
fn append_line(line: &str) -> std::io::Result<()> {
    let path = std::path::Path::new(COST_TSV_PATH);

    // Create parent directories if needed
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let needs_header = !path.exists();
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;

    if needs_header {
        writeln!(file, "{HEADER}")?;
    }
    writeln!(file, "{line}")
}
