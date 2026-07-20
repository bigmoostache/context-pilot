//! Per-panel freeze pass — the normal (non-full-freeze) path that hashes each
//! panel, consults freeze policy, snapshots emitted content, tracks per-panel
//! cache cost, and records culprit-decomposed tick telemetry.
//!
//! Extracted from [`super`] to keep `mod.rs` under the 500-line structure limit.

use crate::app::panels::ContextItem;
use crate::state::State;
use crate::state::cache::hash_content;
use cp_base::state::data::model_helpers::ModelPricing as _;
use cp_base::state::data::{CacheBreakKind, TickTelemetry};

use super::freeze::{self, FreezeConditions, FreezeDecision};

/// Shared freeze-pass parameters: the tick's freeze conditions plus the
/// system+tools token prefix (for telemetry token decomposition).
#[derive(Clone, Copy)]
pub(super) struct FreezeMeta {
    /// Freeze conditions computed for this tick (queue / tempo flags).
    pub cond: FreezeConditions,
    /// System-prompt + tool-definition token count preceding all panels.
    pub prompt_prefix_tokens: usize,
}

/// Replay the exact previous prompt from a frozen snapshot. Replaces all panel
/// items with the snapshot (preserving the fresh "chat" item at the tail) and
/// records simplified all-hit telemetry. `previous_panel_*` state stays untouched.
pub(super) fn apply_full_freeze(
    state: &mut State,
    context_items: &mut Vec<ContextItem>,
    snapshot: &[ContextItem],
    meta: FreezeMeta,
) {
    let chat_item = context_items.iter().find(|i| i.id == "chat").cloned();
    context_items.clear();
    context_items.extend_from_slice(snapshot);
    if let Some(chat) = chat_item {
        context_items.push(chat);
    }

    let total_panel_tokens: usize =
        context_items.iter().filter(|i| i.id != "chat").map(|i| crate::state::estimate_tokens(&i.content)).sum();
    let conversation_tokens: usize =
        context_items.iter().find(|i| i.id == "chat").map_or(0, |i| crate::state::estimate_tokens(&i.content));

    state.tick_telemetry = Some(
        TickTelemetry::start(
            crate::app::panels::now_ms(),
            recent_tool_names(state),
            meta.cond.queue_active,
            meta.cond.tempo,
        )
        .token_layout(meta.prompt_prefix_tokens.saturating_add(total_panel_tokens), 0, conversation_tokens),
    );
}

/// The last 3 real tool names (skipping synthetic `Tool_execution` stubs),
/// joined by comma — for tick-telemetry culprit context.
fn recent_tool_names(state: &State) -> String {
    state
        .messages
        .iter()
        .rev()
        .filter(|m| m.msg_type == crate::state::MsgKind::ToolCall)
        .flat_map(|m| m.tool_uses.iter().map(|t| t.name.as_str()))
        .filter(|name| *name != "Tool_execution")
        .take(3)
        .collect::<Vec<_>>()
        .join(",")
}

/// Culprit tracking accumulated across the per-panel freeze pass.
#[derive(Default)]
struct FreezeCulprit {
    /// Index of the first panel that broke the cache (if any).
    panel_idx: Option<usize>,
    /// Context type of that panel, for telemetry.
    kind: Option<String>,
    /// The culprit panel's configured max-freeze budget.
    max_freezes: u8,
    /// Whether the culprit is a newly-appeared panel (no prior hash).
    is_new: bool,
}

/// Per-panel freeze outcome fed back to the pass loop.
struct PanelEmit {
    /// Hash of the content actually emitted this tick (fresh or frozen).
    emitted_hash: String,
    /// When `Some`, this panel is the first cache break (culprit) — carries its
    /// telemetry fields (type, `max_freezes`, `is_new`).
    culprit: Option<(String, u8, bool)>,
}

/// Decide one panel's fate: freeze (restore snapshot) or emit fresh. Mutates the
/// matching `state.context` entry's freeze bookkeeping. `broken` is the widened
/// cache-broken flag (real break OR past the BP anchor).
fn freeze_one_panel(state: &mut State, item: &mut ContextItem, cond: FreezeConditions, broken: bool) -> PanelEmit {
    let fresh_hash = hash_content(&item.content);
    let Some(entry) = state.context.iter_mut().find(|c| c.id == item.id) else {
        // Orphaned item (no Entry) — emit as-is, breaks cache, never emitted before.
        return PanelEmit { emitted_hash: fresh_hash, culprit: Some((item.id.clone(), 0, true)) };
    };

    let last_hash = entry.emitted.hash.as_deref();
    let content_changed = last_hash.is_none_or(|lh| lh != fresh_hash);
    if !content_changed {
        // Cache preserved naturally — no snapshot mutation.
        entry.emitted.context = Some(item.clone());
        return PanelEmit { emitted_hash: fresh_hash, culprit: None };
    }

    let panel = crate::app::panels::get_panel(&entry.context_type);
    let decision = cond.freeze_panel(broken, entry.freeze_count, panel.max_freezes());
    if decision == FreezeDecision::Freeze
        && let Some(frozen) = entry.emitted.context.as_ref()
    {
        *item = frozen.clone();
        entry.freeze_count = entry.freeze_count.saturating_add(1);
        entry.total_freezes = entry.total_freezes.saturating_add(1);
        let emitted_hash = entry.emitted.hash.clone().unwrap_or(fresh_hash);
        entry.emitted.context = Some(item.clone());
        return PanelEmit { emitted_hash, culprit: None };
    }

    // FRESH emission.
    let culprit = (entry.context_type.to_string(), panel.max_freezes(), last_hash.is_none());
    entry.freeze_count = 0;
    entry.emitted.hash = Some(fresh_hash.clone());
    entry.total_cache_misses = entry.total_cache_misses.saturating_add(1);
    entry.emitted.context = Some(item.clone());
    PanelEmit { emitted_hash: fresh_hash, culprit: Some(culprit) }
}

/// Mutable accumulators threaded through the per-panel freeze pass.
#[derive(Default)]
struct PassAcc {
    /// Whether any panel has broken the cache so far this pass.
    cache_broken: bool,
    /// `id:hash` entries for every emitted panel, in order.
    new_hash_list: Vec<String>,
    /// The first cache-break culprit seen this pass.
    culprit: FreezeCulprit,
    /// Per-panel emitted token counts, in order.
    panel_token_counts: Vec<usize>,
    /// Running index of the current panel in the pass.
    panel_idx: usize,
}

/// Per-pass constants passed to every `fold_one_panel` call.
#[derive(Clone, Copy)]
struct FoldCtx {
    /// Freeze conditions computed for this tick (queue / tempo flags).
    cond: FreezeConditions,
    /// Panel index at/after which the cache is force-broken (BP anchor).
    force_break_at: usize,
}

/// Fold one non-chat panel into the pass: decide freeze/fresh, record the first
/// cache break as culprit, and append its emitted hash + token count.
fn fold_one_panel(state: &mut State, item: &mut ContextItem, fold: FoldCtx, acc: &mut PassAcc) {
    let broken_for_decision = acc.cache_broken || acc.panel_idx >= fold.force_break_at;
    let emit = freeze_one_panel(state, item, fold.cond, broken_for_decision);
    if let Some((kind, max_freezes, is_new)) = emit.culprit {
        if !acc.cache_broken {
            acc.culprit = FreezeCulprit { panel_idx: Some(acc.panel_idx), kind: Some(kind), max_freezes, is_new };
        }
        acc.cache_broken = true;
    }
    acc.new_hash_list.push(format!("{}:{}", item.id, emit.emitted_hash));
    acc.panel_token_counts.push(crate::state::estimate_tokens(&item.content));
    acc.panel_idx = acc.panel_idx.saturating_add(1);
}

/// Per-panel freeze pass (normal, non-full-freeze path). Iterates panels, applies
/// freeze/fresh decisions, tracks per-panel cache cost via prefix-match, records
/// culprit-decomposed tick telemetry, and detects disappeared panels.
pub(super) fn run_panel_freeze_pass(state: &mut State, context_items: &mut [ContextItem], meta: FreezeMeta) {
    let cond = meta.cond;
    let force_break_at = freeze::compute_force_break_at(context_items, state, cond);
    let hit_price = state.cache_hit_price_per_mtok();
    let miss_price = state.cache_miss_price_per_mtok();

    let mut acc = PassAcc::default();
    for item in context_items.iter_mut() {
        if item.id == "chat" {
            continue;
        }
        fold_one_panel(state, item, FoldCtx { cond, force_break_at }, &mut acc);
    }
    let PassAcc { cache_broken, new_hash_list, mut culprit, panel_token_counts, .. } = acc;

    apply_panel_cache_costs(state, &new_hash_list, hit_price, miss_price);
    state.previous_panel_hash_list = new_hash_list;

    let break_kind = classify_break_kind(state, context_items, cache_broken, &mut culprit);
    save_panel_id_types(state, context_items);
    record_freeze_telemetry(
        state,
        context_items,
        meta,
        &TelemetryParts { panel_token_counts: &panel_token_counts, culprit: &culprit, break_kind },
    );
}

/// Prefix-match `new_hash_list` against the previous tick's list; mark each panel
/// hit/miss and accrue its dollar cost onto `panel_total_cost`.
fn apply_panel_cache_costs(state: &mut State, new_hash_list: &[String], hit_price: f32, miss_price: f32) {
    let prefix_len =
        new_hash_list.iter().zip(state.previous_panel_hash_list.iter()).take_while(|entry| entry.0 == entry.1).count();
    for (i, entry_str) in new_hash_list.iter().enumerate() {
        let panel_id = entry_str.split(':').next().unwrap_or("");
        let is_hit = i < prefix_len;
        let price = if is_hit { hit_price } else { miss_price };
        if let Some(ctx) = state.context.iter_mut().find(|c| c.id == panel_id) {
            let cost = cp_base::cast::float_math::cost_usd(ctx.token_count, price);
            ctx.panel_cache_hit = is_hit;
            ctx.panel_total_cost = cp_base::cast::float_math::add(ctx.panel_total_cost, cost);
        }
    }
}

/// Classify the cache-break reason. When nothing broke in the loop, checks for a
/// panel that disappeared since last tick (which still breaks the prompt) and
/// backfills the culprit type.
fn classify_break_kind(
    state: &State,
    context_items: &[ContextItem],
    cache_broken: bool,
    culprit: &mut FreezeCulprit,
) -> CacheBreakKind {
    if cache_broken {
        return if culprit.is_new { CacheBreakKind::PanelAppeared } else { CacheBreakKind::ContentChanged };
    }
    let current_ids: std::collections::HashSet<&str> =
        context_items.iter().filter(|item| item.id != "chat").map(|item| item.id.as_str()).collect();
    if let Some(entry) = state.previous_panel_id_types.iter().find(|entry| !current_ids.contains(entry.0.as_str())) {
        culprit.kind = Some(entry.1.clone());
        CacheBreakKind::PanelDisappeared
    } else {
        CacheBreakKind::NoBreak
    }
}

/// Persist `(panel_id, context_type)` pairs for next tick's disappearance check.
fn save_panel_id_types(state: &mut State, context_items: &[ContextItem]) {
    state.previous_panel_id_types = context_items
        .iter()
        .filter(|item| item.id != "chat")
        .map(|item| {
            let ctx_type = state
                .context
                .iter()
                .find(|c| c.id == item.id)
                .map_or_else(|| item.id.clone(), |c| c.context_type.to_string());
            (item.id.clone(), ctx_type)
        })
        .collect();
}

/// The pass-derived telemetry inputs (token decomposition source + culprit).
struct TelemetryParts<'ctx> {
    /// Per-panel emitted token counts, in order.
    panel_token_counts: &'ctx [usize],
    /// The first cache-break culprit seen this pass.
    culprit: &'ctx FreezeCulprit,
    /// The classified cache-break reason for this tick.
    break_kind: CacheBreakKind,
}

/// Build and store `tick_telemetry` from the pass results: culprit token
/// decomposition (before / culprit / after+conversation) and freeze flags.
fn record_freeze_telemetry(
    state: &mut State,
    context_items: &[ContextItem],
    meta: FreezeMeta,
    parts: &TelemetryParts<'_>,
) {
    let TelemetryParts { panel_token_counts, culprit, break_kind } = *parts;
    let conversation_tokens: usize =
        context_items.iter().find(|i| i.id == "chat").map_or(0, |i| crate::state::estimate_tokens(&i.content));
    let (tokens_before, tok_culprit, tokens_after) = culprit.panel_idx.map_or_else(
        || (panel_token_counts.iter().sum(), 0, 0),
        |ci| {
            let before: usize = panel_token_counts.iter().take(ci).sum();
            let c = panel_token_counts.get(ci).copied().unwrap_or(0);
            let after: usize = panel_token_counts.iter().skip(ci.saturating_add(1)).sum();
            (before, c, after)
        },
    );
    state.tick_telemetry = Some(
        TickTelemetry::start(
            crate::app::panels::now_ms(),
            recent_tool_names(state),
            meta.cond.queue_active,
            meta.cond.tempo,
        )
        .token_layout(
            meta.prompt_prefix_tokens.saturating_add(tokens_before),
            tok_culprit,
            tokens_after.saturating_add(conversation_tokens),
        )
        .culprit(culprit.kind.clone().unwrap_or_else(|| "none".to_owned()), break_kind, culprit.max_freezes),
    );
}
