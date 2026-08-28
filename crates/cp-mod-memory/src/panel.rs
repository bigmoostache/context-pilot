use crossterm::event::KeyEvent;

use cp_base::panels::{CacheRequest, CacheUpdate, ContextItem, Panel};
use cp_base::state::actions::Action;
use cp_base::state::context::Entry;
use cp_base::state::context::{Kind, estimate_tokens};
use cp_base::state::runtime::State;

use crate::types::{MemorySlot, MemoryState, Tier};
use cp_base::panels::scroll_key_action;
use std::fmt::Write as _;

/// The 1-based slot index parsed from an id (`M-{tier}-{n}` → `n`); `0` if the
/// trailing segment isn't a number (never expected — ids are backend-built).
fn slot_index(id: &str) -> usize {
    id.rsplit('-').next().and_then(|n| n.parse().ok()).unwrap_or(0)
}

/// Occupied slots of `tier`, sorted by slot **id index** ascending (T690) — a
/// pure positional order (`M-tiny-1`, `M-tiny-2`, …). Importance is still
/// surfaced per-row (tag + colour), just no longer the sort key.
fn occupied_of(state: &MemoryState, tier: Tier) -> Vec<&MemorySlot> {
    let mut v: Vec<&MemorySlot> = state.slots.iter().filter(|s| s.occupied && s.tier == tier).collect();
    v.sort_by_key(|s| slot_index(&s.id));
    v
}

/// The 1-based indices of `tier`'s FREE (unoccupied) slots, ascending — listed
/// explicitly under each tier so the exact reusable ids are always in view (T690).
fn free_indices_of(state: &MemoryState, tier: Tier) -> Vec<usize> {
    let mut v: Vec<usize> =
        state.slots.iter().filter(|s| !s.occupied && s.tier == tier).map(|s| slot_index(&s.id)).collect();
    v.sort_unstable();
    v
}

/// Render a tier's free slots as `"{n} free: {csv}"`, or `None` when the tier is
/// full — the shared copy for both the panel and the LLM context projection.
fn free_line(indices: &[usize]) -> Option<String> {
    if indices.is_empty() {
        return None;
    }
    let csv = indices.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ");
    Some(format!("{} free: {csv}", indices.len()))
}

/// Panel that renders the fixed memory budget and provides LLM context.
pub(crate) struct MemoryPanel;

impl MemoryPanel {
    /// Format the fixed slot budget for LLM context, grouped by tier.
    ///
    /// Each tier shows a `used/total` header, its occupied slots (id order), and
    /// a single line listing the free slot indices — so both the ceiling and the
    /// exact reusable ids are always in view without one row per empty slot.
    fn format_for_context(state: &State) -> String {
        let ms = MemoryState::get(state);
        let mut out = String::new();

        for tier in Tier::ALL {
            let occ = occupied_of(ms, tier);
            let total = tier.slot_count();
            let _h = writeln!(out, "{} ({}/{}):", tier.slug(), occ.len(), total);
            for slot in &occ {
                let _r = writeln!(out, "  {} [{}] {}", slot.id, slot.importance.as_str(), slot.title);
                if !slot.contents.is_empty() {
                    for line in slot.contents.lines() {
                        let _l = writeln!(out, "      {line}");
                    }
                }
            }
            if let Some(line) = free_line(&free_indices_of(ms, tier)) {
                let _f = writeln!(out, "  … {line}");
            }
        }

        out.trim_end().to_owned()
    }
}

impl Panel for MemoryPanel {
    fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {
        scroll_key_action(key)
    }

    fn blocks(&self, state: &State) -> Vec<cp_render::Block> {
        use cp_render::{Block, Semantic, Span as S};

        let ms = MemoryState::get(state);
        let mut blocks = Vec::new();

        for tier in Tier::ALL {
            let occ = occupied_of(ms, tier);
            let total = tier.slot_count();
            blocks.push(Block::Line(vec![
                S::accent(format!(" {}", tier.slug())).bold(),
                S::muted(format!(" ({}/{})", occ.len(), total)),
            ]));
            for slot in &occ {
                let imp_sem = match slot.importance {
                    crate::types::MemoryImportance::Critical => Semantic::Warning,
                    crate::types::MemoryImportance::High => Semantic::Accent,
                    crate::types::MemoryImportance::Medium => Semantic::Code,
                    crate::types::MemoryImportance::Low => Semantic::Muted,
                };
                blocks.push(Block::Line(vec![
                    S::muted(format!("   {} ", slot.id)),
                    S::styled(format!("[{}] ", slot.importance.as_str()), imp_sem),
                    S::new(slot.title.clone()).bold(),
                ]));
                for line in slot.contents.lines() {
                    blocks.push(Block::Line(vec![S::new("      ".into()), S::styled(line.to_owned(), Semantic::Code)]));
                }
            }
            if let Some(line) = free_line(&free_indices_of(ms, tier)) {
                blocks.push(Block::Line(vec![S::muted(format!("   … {line}")).italic()]));
            }
        }

        blocks
    }

    fn title(&self, _state: &State) -> String {
        "Memory".to_owned()
    }

    fn refresh(&self, state: &mut State) {
        let content = Self::format_for_context(state);
        let token_count = estimate_tokens(&content);
        for ctx in &mut state.context {
            if ctx.context_type.as_str() == Kind::MEMORY {
                ctx.token_count = token_count;
                let _changed = cp_base::panels::update_if_changed(ctx, &content);
                break;
            }
        }
    }

    fn max_freezes(&self) -> u8 {
        0
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        let content = Self::format_for_context(state);
        let (id, last_refresh_ms) = state
            .context
            .iter()
            .find(|c| c.context_type.as_str() == Kind::MEMORY)
            .map_or(("P4", 0), |c| (c.id.as_str(), c.last_refresh_ms));
        vec![ContextItem::new(id, "Memories", content, last_refresh_ms)]
    }

    fn needs_cache(&self) -> bool {
        false
    }

    fn refresh_cache(&self, _request: CacheRequest) -> Option<CacheUpdate> {
        None
    }

    fn build_cache_request(&self, _ctx: &Entry, _state: &State) -> Option<CacheRequest> {
        None
    }

    fn apply_cache_update(&self, _update: CacheUpdate, _ctx: &mut Entry, _state: &mut State) -> bool {
        false
    }

    fn cache_refresh_interval_ms(&self) -> Option<u64> {
        None
    }

    fn suicide(&self, _ctx: &Entry, _state: &State) -> bool {
        false
    }
}
