use crossterm::event::KeyEvent;

use cp_base::panels::{CacheRequest, CacheUpdate, ContextItem, Panel};
use cp_base::state::actions::Action;
use cp_base::state::context::Entry;
use cp_base::state::context::{Kind, estimate_tokens};
use cp_base::state::runtime::State;
use cp_base::ui::{TextCell, render_table_text};

use crate::types::{MemoryImportance, MemoryState};
use cp_base::panels::scroll_key_action;
use std::fmt::Write as _;

/// Panel that renders memory items and provides LLM context.
pub(crate) struct MemoryPanel;

impl MemoryPanel {
    /// Format memories for LLM context.
    /// Closed memories: table with ID, `tl_dr`, importance, labels.
    /// Open memories: YAML-formatted complete information.
    fn format_memories_for_context(state: &State) -> String {
        let ms = MemoryState::get(state);
        if ms.memories.is_empty() {
            return "No memories".to_string();
        }

        // Sort by importance (critical first)
        let mut sorted: Vec<_> = ms.memories.iter().collect();
        sorted.sort_by_key(|m| match m.importance {
            MemoryImportance::Critical => 0,
            MemoryImportance::High => 1,
            MemoryImportance::Medium => 2,
            MemoryImportance::Low => 3,
        });

        let closed: Vec<_> = sorted.iter().filter(|m| !ms.open_memory_ids.contains(&m.id)).collect();
        let open: Vec<_> = sorted.iter().filter(|m| ms.open_memory_ids.contains(&m.id)).collect();

        let mut output = String::new();

        // Closed memories as table using shared renderer
        if !closed.is_empty() {
            let headers = ["ID", "Summary", "Importance", "Labels"];
            let rows: Vec<Vec<TextCell>> = closed
                .iter()
                .map(|m| {
                    let labels = if m.labels.is_empty() { String::new() } else { m.labels.join(", ") };
                    vec![
                        TextCell::left(&m.id),
                        TextCell::left(&m.tl_dr),
                        TextCell::left(m.importance.as_str()),
                        TextCell::left(labels),
                    ]
                })
                .collect();

            output.push_str(&render_table_text(&headers, &rows));
        }

        // Open memories as YAML
        if !open.is_empty() {
            if !closed.is_empty() {
                output.push('\n');
            }
            for (i, memory) in open.iter().enumerate() {
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
        }

        output.trim_end().to_string()
    }
}

impl Panel for MemoryPanel {
    fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {
        scroll_key_action(key)
    }

    fn blocks(&self, state: &State) -> Vec<cp_render::Block> {
        use cp_render::{Align, Block, Cell as IrCell, Semantic, Span as S};

        let ms = MemoryState::get(state);

        if ms.memories.is_empty() {
            return vec![Block::Line(vec![S::muted("  No memories".into()).italic()])];
        }

        // Sort by importance (critical first)
        let mut sorted: Vec<_> = ms.memories.iter().collect();
        sorted.sort_by_key(|m| match m.importance {
            MemoryImportance::Critical => 0,
            MemoryImportance::High => 1,
            MemoryImportance::Medium => 2,
            MemoryImportance::Low => 3,
        });

        let closed: Vec<_> = sorted.iter().filter(|m| !ms.open_memory_ids.contains(&m.id)).copied().collect();
        let open: Vec<_> = sorted.iter().filter(|m| ms.open_memory_ids.contains(&m.id)).copied().collect();

        let mut blocks = Vec::new();

        // Closed memories as IR table
        if !closed.is_empty() {
            let mut rows = Vec::new();
            for memory in &closed {
                let imp_sem = match memory.importance {
                    MemoryImportance::Critical => Semantic::Warning,
                    MemoryImportance::High => Semantic::Accent,
                    MemoryImportance::Medium => Semantic::Code,
                    MemoryImportance::Low => Semantic::Muted,
                };
                let labels = if memory.labels.is_empty() { String::new() } else { memory.labels.join(", ") };
                rows.push(vec![
                    IrCell::styled(memory.id.clone(), Semantic::AccentDim),
                    IrCell::text(memory.tl_dr.clone()),
                    IrCell::styled(memory.importance.as_str().into(), imp_sem),
                    IrCell::styled(labels, Semantic::Muted),
                ]);
            }
            blocks.push(Block::table(
                vec![
                    ("ID", Align::Left),
                    ("Summary", Align::Left),
                    ("Importance", Align::Left),
                    ("Labels", Align::Left),
                ],
                rows,
            ));
        }

        // Open memories as key-value blocks
        if !open.is_empty() {
            if !closed.is_empty() {
                blocks.push(Block::Empty);
            }
            for (i, memory) in open.iter().enumerate() {
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
                    (
                        vec![S::muted("   importance: ".into())],
                        vec![S::styled(memory.importance.as_str().into(), imp_sem)],
                    ),
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
                        blocks.push(Block::Line(vec![
                            S::new("     ".into()),
                            S::styled(line.to_string(), Semantic::Code),
                        ]));
                    }
                }
            }
        }

        blocks
    }
    fn title(&self, _state: &State) -> String {
        "Memory".to_string()
    }

    fn refresh(&self, state: &mut State) {
        let memory_content = Self::format_memories_for_context(state);
        let token_count = estimate_tokens(&memory_content);

        for ctx in &mut state.context {
            if ctx.context_type.as_str() == Kind::MEMORY {
                ctx.token_count = token_count;
                let _ = cp_base::panels::update_if_changed(ctx, &memory_content);
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
