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
    /// Panel index of the last surviving breakpoint (BP anchor). Panels
    /// *strictly after* this index are force-broken (cache reuse stops at the
    /// anchor, so their refresh is free); the anchor itself is preserved.
    force_break_at: usize,
}

/// Fold one non-chat panel into the pass: decide freeze/fresh, record the first
/// cache break as culprit, and append its emitted hash + token count.
fn fold_one_panel(state: &mut State, item: &mut ContextItem, fold: FoldCtx, acc: &mut PassAcc) {
    // Strictly `>` (not `>=`): the anchor panel at `force_break_at` IS the last
    // surviving cache breakpoint. Force-refreshing it would change its bytes and
    // invalidate the segment it anchors, regressing the prefix to the previous
    // breakpoint. Keep it frozen; free-refresh only panels strictly after it.
    let broken_for_decision = acc.cache_broken || acc.panel_idx > fold.force_break_at;
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

/// Per-panel freeze pass (normal, non-full-freeze path). Reorders the broken
/// tail by cost, then iterates panels applying freeze/fresh decisions, tracks
/// per-panel cache cost via prefix-match, records culprit-decomposed tick
/// telemetry, detects disappeared panels, and persists the final panel order.
pub(super) fn run_panel_freeze_pass(state: &mut State, context_items: &mut [ContextItem], meta: FreezeMeta) {
    let cond = meta.cond;

    // 1. Reorder the free-to-permute tail into the fixed T740 panel priority
    //    (see `emission_rank`): stable panels (conversation_history, results)
    //    sink deep to hug the cache frontier, volatile ones (console, file,
    //    tree) rise toward the conversation tip where a break is cheap. The tail
    //    is past the last surviving breakpoint (or the whole list is a miss), so
    //    permuting it is billed-fresh-anyway = zero cost this turn. `chat` stays
    //    pinned last.
    let reorder_from = freeze::compute_reorder_from(context_items, state, cond);
    reorder_broken_tail(state, context_items, reorder_from);

    // 2. Compute the content force-break anchor on the FINAL (post-reorder) order
    //    so the fold loop's freeze/fresh decisions align with the emitted layout.
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
    // Persist the final order (including chat, pinned last) as the stable base
    // for next tick's replay in `prepare_stream_context`.
    state.previous_panel_order = context_items.iter().map(|i| i.id.clone()).collect();
    record_freeze_telemetry(
        state,
        context_items,
        meta,
        &TelemetryParts { panel_token_counts: &panel_token_counts, culprit: &culprit, break_kind },
    );
}

/// Default emission rank for a context type not in [`emission_rank`]'s table.
///
/// Sits just below `console` (the nearest-tip listed type) but above `chat`, so
/// an unrecognised panel lands in the volatile, cheap-to-break zone near the
/// conversation tip — the conservative choice when we can't assume it is stable.
const DEFAULT_EMISSION_RANK: usize = 900;

/// Fixed emission rank for a panel `context_type`: **lower = emitted earlier =
/// deeper in the prompt = more likely to stay cached**; higher = nearer the
/// conversation tip = cheaper to break.
///
/// This is the user-specified panel priority (T740), written here as the
/// EMISSION order (the reverse of the "closest-to-tip first" list the user
/// gave): `conversation_history` is deepest (immutable, cache it forever) and
/// `console` sits just above `chat` (most volatile, break it for free).
///
/// Types the user did not enumerate are slotted next to their natural sibling
/// (`brave_result` by `firecrawl_result`, `entity_result` by `entities`,
/// `library`/`skill` by `tools`, `agora` by `overview`); anything unknown falls
/// to [`DEFAULT_EMISSION_RANK`].
fn emission_rank(context_type: &str) -> usize {
    match context_type {
        "conversation_history" => 0,
        "firecrawl_result" => 1,
        "brave_result" => 2,
        "entities" => 3,
        "entity_result" => 4,
        "github_result" => 5,
        "scratchpad" => 6,
        "callback" => 7,
        "queue" => 8,
        "git_result" => 9,
        "memory" => 10,
        "library" => 11,
        "skill" => 12,
        "tools" => 13,
        "search_result" => 14,
        "todo" => 15,
        "overview" => 16,
        "agora" => 17,
        "context_radar" => 18,
        "threads" => 19,
        "tree" => 20,
        "file" => 21,
        "console" => 22,
        _ => DEFAULT_EMISSION_RANK,
    }
}

/// Emission rank for a panel by its `id`: resolves the id to its context type
/// via `state.context`, then to a fixed rank. `chat` is pinned last (`MAX`);
/// an id with no matching entry falls to [`DEFAULT_EMISSION_RANK`].
fn panel_rank(state: &State, id: &str) -> usize {
    if id == "chat" {
        return usize::MAX;
    }
    state.context.iter().find(|c| c.id == id).map_or(DEFAULT_EMISSION_RANK, |c| emission_rank(c.context_type.as_str()))
}

/// Permute `context_items[reorder_from..]` in place into the fixed T740 panel
/// order (ascending [`emission_rank`]), keeping `chat` pinned last. The sort is
/// STABLE, so panels sharing a rank (same type — multiple files, several
/// history chunks) keep their existing relative order, preserving ancienneté
/// (older first). No-op when `reorder_from` is out of range (no culprit →
/// nothing broke → nothing to reorder).
fn reorder_broken_tail(state: &State, context_items: &mut [ContextItem], reorder_from: usize) {
    let Some(tail) = context_items.get_mut(reorder_from..) else {
        return; // out of range: no culprit → nothing broke → nothing to reorder
    };
    // sort_by_key is stable → equal-rank (same-type) panels keep insertion order.
    tail.sort_by_key(|a| panel_rank(state, &a.id));
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

#[cfg(test)]
mod tests {
    use super::emission_rank;

    /// The T740 panel order, from DEEPEST (emitted first, most cached) to
    /// nearest the conversation tip. This is the reverse of the user's
    /// "closest-to-tip first" list and must stay strictly increasing.
    #[test]
    fn t740_emission_order_is_strictly_increasing() {
        let order = [
            "conversation_history",
            "firecrawl_result",
            "entities",
            "github_result",
            "scratchpad",
            "callback",
            "queue",
            "git_result",
            "memory",
            "tools",
            "search_result",
            "todo",
            "overview",
            "context_radar",
            "threads",
            "tree",
            "file",
            "console",
        ];
        for pair in order.windows(2) {
            let &[deep, shallow] = pair else { continue };
            assert!(emission_rank(deep) < emission_rank(shallow), "{deep} must be deeper (lower rank) than {shallow}");
        }
    }

    /// `conversation_history` is the deepest listed panel; console the shallowest.
    #[test]
    fn t740_endpoints() {
        assert_eq!(emission_rank("conversation_history"), 0);
        assert!(emission_rank("console") < emission_rank("__unknown_type__"));
    }
}
