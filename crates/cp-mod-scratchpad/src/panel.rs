use crossterm::event::KeyEvent;

use cp_base::panels::{ContextItem, Panel, scroll_key_action};
use cp_base::state::actions::Action;
use cp_base::state::context::{Kind, estimate_tokens};
use cp_base::state::runtime::State;

use crate::types::ScratchpadState;
use std::fmt::Write as _;

/// Panel that renders scratchpad cells and provides their content as LLM context.
pub(crate) struct ScratchpadPanel;

impl ScratchpadPanel {
    /// Format scratchpad cells for LLM context
    fn format_cells_for_context(state: &State) -> String {
        let ss = ScratchpadState::get(state);
        if ss.scratchpad_cells.is_empty() {
            return "No scratchpad cells".to_string();
        }

        let mut output = String::new();
        for cell in &ss.scratchpad_cells {
            let _r = writeln!(output, "=== [{}] {} ===", cell.id, cell.title);
            output.push_str(&cell.content);
            output.push_str("\n\n");
        }

        output.trim_end().to_string()
    }
}

impl Panel for ScratchpadPanel {
    fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {
        scroll_key_action(key)
    }

    fn blocks(&self, state: &State) -> Vec<cp_render::Block> {
        use cp_render::{Block, Span as S};

        let ss = ScratchpadState::get(state);

        if ss.scratchpad_cells.is_empty() {
            return vec![
                Block::Line(vec![S::muted("  No scratchpad cells".into()).italic()]),
                Block::Line(vec![S::muted("  Use scratchpad_create_cell to add notes".into())]),
            ];
        }

        let mut blocks = Vec::new();
        for cell in &ss.scratchpad_cells {
            blocks.push(Block::Line(vec![
                S::new("  ".into()),
                S::accent(cell.id.clone()).bold(),
                S::new(" ".into()),
                S::new(cell.title.clone()).bold(),
            ]));

            let lines: Vec<&str> = cell.content.lines().take(5).collect();
            for line in &lines {
                blocks.push(Block::Line(vec![S::new("   ".into()), S::muted(line.to_string())]));
            }

            let total_lines = cell.content.lines().count();
            if total_lines > 5 {
                blocks.push(Block::Line(vec![
                    S::new("   ".into()),
                    S::muted(format!("... ({} more lines)", total_lines.saturating_sub(5))).italic(),
                ]));
            }

            blocks.push(Block::Empty);
        }

        blocks
    }
    fn title(&self, _state: &State) -> String {
        "Scratchpad".to_string()
    }

    fn refresh(&self, state: &mut State) {
        let content = Self::format_cells_for_context(state);
        let token_count = estimate_tokens(&content);

        for ctx in &mut state.context {
            if ctx.context_type.as_str() == Kind::SCRATCHPAD {
                ctx.token_count = token_count;
                let _ = cp_base::panels::update_if_changed(ctx, &content);
                break;
            }
        }
    }

    fn max_freezes(&self) -> u8 {
        0
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        let content = Self::format_cells_for_context(state);
        // Find the Scratchpad context element to get its ID and timestamp
        let (id, last_refresh_ms) = state
            .context
            .iter()
            .find(|c| c.context_type.as_str() == Kind::SCRATCHPAD)
            .map_or(("P7", 0), |c| (c.id.as_str(), c.last_refresh_ms));
        vec![ContextItem::new(id, "Scratchpad", content, last_refresh_ms)]
    }

    fn needs_cache(&self) -> bool {
        false
    }
    fn refresh_cache(&self, _request: cp_base::panels::CacheRequest) -> Option<cp_base::panels::CacheUpdate> {
        None
    }
    fn build_cache_request(
        &self,
        _ctx: &cp_base::state::context::Entry,
        _state: &State,
    ) -> Option<cp_base::panels::CacheRequest> {
        None
    }
    fn apply_cache_update(
        &self,
        _update: cp_base::panels::CacheUpdate,
        _ctx: &mut cp_base::state::context::Entry,
        _state: &mut State,
    ) -> bool {
        false
    }
    fn cache_refresh_interval_ms(&self) -> Option<u64> {
        None
    }
    fn suicide(&self, _ctx: &cp_base::state::context::Entry, _state: &State) -> bool {
        false
    }
}
