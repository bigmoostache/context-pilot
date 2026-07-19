use crossterm::event::KeyEvent;

use cp_base::panels::{CacheRequest, CacheUpdate, ContextItem, Panel};
use cp_base::state::actions::Action;
use cp_base::state::context::Entry;
use cp_base::state::context::{Kind, estimate_tokens};
use cp_base::state::runtime::State;

use crate::types::{MemoryImportance, MemoryState};
use cp_base::panels::scroll_key_action;
use std::fmt::Write as _;

/// Panel that renders memory items and provides LLM context.
pub(crate) struct MemoryPanel;

impl MemoryPanel {
    /// Format memories for LLM context.
    /// All memories are rendered as YAML with full contents.
    fn format_memories_for_context(state: &State) -> String {
        let ms = MemoryState::get(state);
        if ms.memories.is_empty() {
            return "No memories".to_owned();
        }

        // Sort by importance (critical first)
        let mut sorted: Vec<_> = ms.memories.iter().collect();
        sorted.sort_by_key(|m| match m.importance {
            MemoryImportance::Critical => 0i32,
            MemoryImportance::High => 1i32,
            MemoryImportance::Medium => 2i32,
            MemoryImportance::Low => 3i32,
        });

        let mut output = String::new();

        for (i, memory) in sorted.iter().enumerate() {
            if i > 0 {
                output.push('\n');
            }
            let _r1 = writeln!(output, "{}:", memory.id);
            let _r2 = writeln!(output, "  tl_dr: {}", memory.tl_dr);
            let _r3 = writeln!(output, "  importance: {}", memory.importance.as_str());
            if !memory.labels.is_empty() {
                let _r4 = writeln!(output, "  labels: [{}]", memory.labels.join(", "));
            }
            if !memory.contents.is_empty() {
                output.push_str("  contents: |\n");
                for line in memory.contents.lines() {
                    let _r5 = writeln!(output, "    {line}");
                }
            }
        }

        output.trim_end().to_owned()
    }
}

impl Panel for MemoryPanel {
    fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {
        scroll_key_action(key)
    }

    fn blocks(&self, state: &State) -> Vec<cp_render::Block> {
        use cp_render::{Block, Semantic, Span as S};

        let ms = MemoryState::get(state);

        if ms.memories.is_empty() {
            return vec![Block::Line(vec![S::muted("  No memories".into()).italic()])];
        }

        // Sort by importance (critical first)
        let mut sorted: Vec<_> = ms.memories.iter().collect();
        sorted.sort_by_key(|m| match m.importance {
            MemoryImportance::Critical => 0i32,
            MemoryImportance::High => 1i32,
            MemoryImportance::Medium => 2i32,
            MemoryImportance::Low => 3i32,
        });

        let mut blocks = Vec::new();

        // All memories rendered as key-value blocks with full contents
        for (i, memory) in sorted.iter().enumerate() {
            if i > 0 {
                blocks.push(Block::Empty);
            }
            let imp_sem = match memory.importance {
                MemoryImportance::Critical => Semantic::Warning,
                MemoryImportance::High => Semantic::Accent,
                MemoryImportance::Medium => Semantic::Code,
                MemoryImportance::Low => Semantic::Muted,
            };
            blocks.push(Block::Line(vec![S::new(" ".into()), S::accent(format!("{}:", memory.id)).bold()]));
            blocks.push(Block::KeyValue(vec![
                (vec![S::muted("   tl_dr: ".into())], vec![S::new(memory.tl_dr.clone())]),
                (vec![S::muted("   importance: ".into())], vec![S::styled(memory.importance.as_str().into(), imp_sem)]),
            ]));
            if !memory.labels.is_empty() {
                blocks.push(Block::KeyValue(vec![(
                    vec![S::muted("   labels: ".into())],
                    vec![S::styled(format!("[{}]", memory.labels.join(", ")), Semantic::Code)],
                )]));
            }
            if !memory.contents.is_empty() {
                blocks.push(Block::Line(vec![S::muted("   contents: |".into())]));
                for line in memory.contents.lines() {
                    blocks.push(Block::Line(vec![S::new("     ".into()), S::styled(line.to_owned(), Semantic::Code)]));
                }
            }
        }

        blocks
    }
    fn title(&self, _state: &State) -> String {
        "Memory".to_owned()
    }

    fn refresh(&self, state: &mut State) {
        let memory_content = Self::format_memories_for_context(state);
        let token_count = estimate_tokens(&memory_content);

        for ctx in &mut state.context {
            if ctx.context_type.as_str() == Kind::MEMORY {
                ctx.token_count = token_count;
                let _changed = cp_base::panels::update_if_changed(ctx, &memory_content);
                break;
            }
        }
    }

    fn max_freezes(&self) -> u8 {
        0
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        let content = Self::format_memories_for_context(state);
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
