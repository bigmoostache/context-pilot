use cp_base::panels::{ContextItem, Panel};
use cp_base::state::context::{Kind, estimate_tokens};
use cp_base::state::runtime::State;

use crate::types::{LogEntry, LogsState};
use std::fmt::Write as _;

/// Fixed panel for timestamped log entries with tree-structured summaries.
/// Un-deletable, always present when the logs module is active.
pub(crate) struct LogsPanel;

impl LogsPanel {
    /// Build the text representation used for both LLM context and UI content.
    /// Shows tree structure: top-level logs, summaries with collapse/expand,
    /// and indented children when expanded.
    pub(crate) fn format_logs_tree(state: &State) -> String {
        let ls = LogsState::get(state);
        if ls.logs.is_empty() {
            return "No logs".to_string();
        }

        let mut output = String::new();
        // Only show top-level logs (no parent_id)
        let top_level: Vec<&LogEntry> = ls.logs.iter().filter(|l| l.is_top_level()).collect();

        for log in &top_level {
            format_log_entry(&mut output, log, &LogTreeContext { all_logs: &ls.logs, open_ids: &ls.open_log_ids }, 0);
        }
        output.trim_end().to_string()
    }
}

/// Shared context for recursive log entry formatting/rendering.
struct LogTreeContext<'src> {
    /// All log entries for child lookup during recursive rendering.
    all_logs: &'src [LogEntry],
    /// IDs of expanded summary logs (children visible).
    open_ids: &'src [String],
}

/// Recursively format a log entry with indentation for tree display.
fn format_log_entry(output: &mut String, entry: &LogEntry, ctx: &LogTreeContext<'_>, depth: usize) {
    let indent = "  ".repeat(depth);
    let time_str = format_timestamp(entry.timestamp_ms);

    if entry.is_summary() {
        let is_open = ctx.open_ids.contains(&entry.id);
        let icon = if is_open { "▼" } else { "▶" };
        let child_count = entry.children_ids.len();
        if is_open {
            let _r = writeln!(output, "{}{} [{}] {} {}", indent, icon, entry.id, time_str, entry.content);
            // Show children indented
            for child_id in &entry.children_ids {
                if let Some(child) = ctx.all_logs.iter().find(|l| l.id == *child_id) {
                    format_log_entry(output, child, ctx, depth.saturating_add(1));
                }
            }
        } else {
            let _r = writeln!(
                output,
                "{}{} [{}] {} {} ({} children)",
                indent, icon, entry.id, time_str, entry.content, child_count
            );
        }
    } else {
        let _r = writeln!(output, "{}[{}] {} {}", indent, entry.id, time_str, entry.content);
    }
}

impl Panel for LogsPanel {
    fn needs_cache(&self) -> bool {
        false
    }

    fn handle_key(&self, _key: &crossterm::event::KeyEvent, _state: &State) -> Option<cp_base::state::actions::Action> {
        None
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

    fn blocks(&self, state: &State) -> Vec<cp_render::Block> {
        struct IrCtx<'src> {
            all_logs: &'src [LogEntry],
            open_ids: &'src [String],
        }

        fn render_log_ir(blocks: &mut Vec<Block>, entry: &LogEntry, ctx: &IrCtx<'_>, depth: usize) {
            let indent = "  ".repeat(depth);
            let time_str = format_timestamp(entry.timestamp_ms);

            if entry.is_summary() {
                let is_open = ctx.open_ids.contains(&entry.id);
                let icon = if is_open { "▼" } else { "▶" };

                let mut spans = vec![
                    S::new(indent),
                    S::accent(format!("{icon} ")),
                    S::styled(format!("{} ", entry.id), Semantic::AccentDim),
                    S::muted(format!("{time_str} ")),
                    S::new(entry.content.clone()),
                ];

                if !is_open {
                    spans.push(S::muted(format!(" ({} children)", entry.children_ids.len())));
                }

                blocks.push(Block::Line(spans));

                if is_open {
                    for child_id in &entry.children_ids {
                        if let Some(child) = ctx.all_logs.iter().find(|l| l.id == *child_id) {
                            render_log_ir(blocks, child, ctx, depth.saturating_add(1));
                        }
                    }
                }
            } else {
                blocks.push(Block::Line(vec![
                    S::new(indent),
                    S::styled(format!("{} ", entry.id), Semantic::AccentDim),
                    S::muted(format!("{time_str} ")),
                    S::new(entry.content.clone()),
                ]));
            }
        }

        use cp_render::{Block, Semantic, Span as S};

        let ls = LogsState::get(state);
        if ls.logs.is_empty() {
            return vec![Block::Line(vec![S::muted("No logs yet".into()).italic()])];
        }

        let ctx = IrCtx { all_logs: &ls.logs, open_ids: &ls.open_log_ids };
        let mut blocks = Vec::new();
        let top_level: Vec<&LogEntry> = ls.logs.iter().filter(|l| l.is_top_level()).collect();

        for log in &top_level {
            render_log_ir(&mut blocks, log, &ctx, 0);
        }
        blocks
    }
    fn title(&self, _state: &State) -> String {
        "Logs".to_string()
    }

    fn refresh(&self, state: &mut State) {
        let content = Self::format_logs_tree(state);
        let token_count = estimate_tokens(&content);

        for ctx in &mut state.context {
            if ctx.context_type.as_str() == Kind::LOGS {
                ctx.token_count = token_count;
                let _ = cp_base::panels::update_if_changed(ctx, &content);
                break;
            }
        }
    }

    fn max_freezes(&self) -> u8 {
        3
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        let content = Self::format_logs_tree(state);
        let (id, last_refresh_ms) = state
            .context
            .iter()
            .find(|c| c.context_type.as_str() == Kind::LOGS)
            .map_or(("P10", 0), |c| (c.id.as_str(), c.last_refresh_ms));
        vec![ContextItem::new(id, "Logs", content, last_refresh_ms)]
    }
}

/// Format a millisecond timestamp as a local `YYYY-MM-DD HH:MM:SS` string.
fn format_timestamp(ms: u64) -> String {
    use chrono::{Local, TimeZone as _};
    i64::try_from(ms)
        .ok()
        .and_then(|ms| Local.timestamp_millis_opt(ms).single())
        .map_or_else(|| format!("{ms}ms"), |dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
}
