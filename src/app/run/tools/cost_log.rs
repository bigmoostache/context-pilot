use crate::app::App;
use crate::state::State;
use cp_base::state::data::model_helpers::{ModelPricing as _, token_cost};
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

    // Rescale proxy token segments by API-reported totals.
    //
    // The proxy (estimate_tokens — chars/4) and the API tokenizer disagree, but
    // both describe the same request.  Scale the proxy segments so they sum to
    // the API-reported total input (hit + miss), preserving their proportions.
    let proxy_total =
        tel.tokens_before_culprit.saturating_add(tel.tokens_culprit).saturating_add(tel.tokens_after_culprit);
    let api_total = state.tick_cache_hit_tokens.saturating_add(state.tick_cache_miss_tokens);

    let (before, culp_tok, after) = if proxy_total > 0 && api_total > 0 {
        let before = tel.tokens_before_culprit.saturating_mul(api_total).checked_div(proxy_total).unwrap_or(0);
        let culprit = tel.tokens_culprit.saturating_mul(api_total).checked_div(proxy_total).unwrap_or(0);
        // Remainder goes to `after` to avoid rounding drift.
        let after = api_total.saturating_sub(before).saturating_sub(culprit);
        (before, culprit, after)
    } else {
        (tel.tokens_before_culprit, tel.tokens_culprit, tel.tokens_after_culprit)
    };

    let line = format!(
        "{datetime}\t{tools}\t{culprit}\t{before}\t{culp_tok}\t{after}\t{queue}\t{tempo}\t{break_kind}\t{max_freezes}\t{hit_tok}\t{hit_cost:.6}\t{miss_tok}\t{miss_cost:.6}\t{out_tok}\t{out_cost:.6}",
        tools = tel.three_last_tools,
        culprit = tel.culprit_type,
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

/// Accumulate token stats AND costs from the intermediate stream into tick/stream/total counters.
///
/// Called before `continue_streaming()` for tool-use ticks — the intermediate
/// `pending_done` would otherwise be lost (only the final tick goes through
/// `finalize_stream → handle_stream_done → apply_token_usage`).
pub(crate) fn accumulate_pending_token_stats(app: &mut App) {
    if let Some((input_tokens, output_tokens, cache_hit_tokens, cache_miss_tokens, _, _, _, _, _)) = app.pending_done {
        // Fold uncached input into cache_miss for correct cost accounting
        let effective_miss = cache_miss_tokens.saturating_add(input_tokens);

        // --- Token accumulation ---
        app.state.tick_cache_hit_tokens = cache_hit_tokens;
        app.state.tick_cache_miss_tokens = effective_miss;
        app.state.tick_output_tokens = output_tokens;
        app.state.tick_uncached_input_tokens = input_tokens;
        app.state.stream_cache_hit_tokens = app.state.stream_cache_hit_tokens.saturating_add(cache_hit_tokens);
        app.state.stream_cache_miss_tokens = app.state.stream_cache_miss_tokens.saturating_add(effective_miss);
        app.state.stream_output_tokens = app.state.stream_output_tokens.saturating_add(output_tokens);
        app.state.stream_uncached_input_tokens = app.state.stream_uncached_input_tokens.saturating_add(input_tokens);
        app.state.cache_hit_tokens = app.state.cache_hit_tokens.saturating_add(cache_hit_tokens);
        app.state.cache_miss_tokens = app.state.cache_miss_tokens.saturating_add(effective_miss);
        app.state.total_output_tokens = app.state.total_output_tokens.saturating_add(output_tokens);
        app.state.uncached_input_tokens = app.state.uncached_input_tokens.saturating_add(input_tokens);

        // --- Cost accumulation (frozen at consumption-time pricing) ---
        let cost_hit = token_cost(cache_hit_tokens, app.state.cache_hit_price_per_mtok());
        let cost_miss = cp_base::cast::float_math::add(
            token_cost(cache_miss_tokens, app.state.cache_miss_price_per_mtok()),
            token_cost(input_tokens, app.state.input_price_per_mtok()),
        );
        let cost_output = token_cost(output_tokens, app.state.output_price_per_mtok());

        app.state.tick_cost_hit_usd = cost_hit;
        app.state.tick_cost_miss_usd = cost_miss;
        app.state.tick_cost_output_usd = cost_output;
        app.state.stream_cost_hit_usd = cp_base::cast::float_math::add(app.state.stream_cost_hit_usd, cost_hit);
        app.state.stream_cost_miss_usd = cp_base::cast::float_math::add(app.state.stream_cost_miss_usd, cost_miss);
        app.state.stream_cost_output_usd =
            cp_base::cast::float_math::add(app.state.stream_cost_output_usd, cost_output);
        app.state.cost_hit_usd = cp_base::cast::float_math::add(app.state.cost_hit_usd, cost_hit);
        app.state.cost_miss_usd = cp_base::cast::float_math::add(app.state.cost_miss_usd, cost_miss);
        app.state.cost_output_usd = cp_base::cast::float_math::add(app.state.cost_output_usd, cost_output);
    }
}
