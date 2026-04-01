use crossterm::event::KeyEvent;

use cp_base::panels::{ContextItem, Panel, now_ms, scroll_key_action};
use cp_base::state::actions::Action;
use cp_base::state::context::{Kind, estimate_tokens};
use cp_base::state::runtime::State;
use cp_base::state::watchers::WatcherRegistry;

use crate::types::{NotificationType, SpineState};
use std::fmt::Write as _;

/// Panel for displaying spine notifications, watchers, and config.
pub(crate) struct SpinePanel;

/// Format a millisecond timestamp as HH:MM:SS
fn format_timestamp(ms: u64) -> String {
    let secs = cp_base::panels::time_arith::ms_to_secs(ms);
    let (hours, minutes, seconds) = cp_base::panels::time_arith::secs_to_hms(secs);
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

impl SpinePanel {
    /// Format notifications for LLM context
    fn format_notifications_for_context(state: &State) -> String {
        let unprocessed: Vec<_> = SpineState::get(state).notifications.iter().filter(|n| n.is_unprocessed()).collect();
        let blocked: Vec<_> = SpineState::get(state)
            .notifications
            .iter()
            .filter(|n| n.status == crate::types::NotificationStatus::Blocked)
            .collect();
        let recent_processed: Vec<_> =
            SpineState::get(state).notifications.iter().filter(|n| n.is_processed()).rev().take(10).collect();

        let mut output = String::new();

        if unprocessed.is_empty() {
            output.push_str("No unprocessed notifications.\n");
        } else {
            for n in &unprocessed {
                let ts = format_timestamp(n.timestamp_ms);
                let _r = writeln!(output, "[{}] {} {} — {}", n.id, ts, n.kind.label(), n.content);
            }
        }

        if !blocked.is_empty() {
            output.push_str("\n=== Blocked (awaiting guard rail clearance) ===\n");
            for n in &blocked {
                let ts = format_timestamp(n.timestamp_ms);
                let _r = writeln!(output, "[{}] {} {} — {}", n.id, ts, n.kind.label(), n.content);
            }
        }

        if !recent_processed.is_empty() {
            output.push_str("\n=== Recent Processed ===\n");
            for n in &recent_processed {
                let ts = format_timestamp(n.timestamp_ms);
                let _r = writeln!(output, "[{}] {} {} — {}", n.id, ts, n.kind.label(), n.content);
            }
        }

        // Show spine config summary
        output.push_str("\n=== Spine Config ===\n");
        let _r1 =
            writeln!(output, "continue_until_todos_done: {}", SpineState::get(state).config.continue_until_todos_done);
        let _r2 =
            writeln!(output, "auto_continuation_count: {}", SpineState::get(state).config.auto_continuation_count);
        if let Some(v) = SpineState::get(state).config.max_auto_retries {
            let _r3 = writeln!(output, "max_auto_retries: {v}");
        }

        // Show active watchers
        if let Some(registry) = state.get_ext::<WatcherRegistry>() {
            let watchers = registry.active_watchers();
            if !watchers.is_empty() {
                output.push_str("\n=== Active Watchers ===\n");
                let now = now_ms();
                for w in watchers {
                    let age_s = cp_base::panels::time_arith::ms_to_secs(now.saturating_sub(w.registered_ms()));
                    let mode = if w.is_blocking() { "blocking" } else { "async" };
                    let _r4 = writeln!(output, "[{}] {} ({}, {}s ago)", w.id(), w.description(), mode, age_s);
                }
            }
        }

        output.trim_end().to_string()
    }
}

impl Panel for SpinePanel {
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

    fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {
        scroll_key_action(key)
    }

    fn blocks(&self, state: &State) -> Vec<cp_render::Block> {
        use cp_render::{Block, Semantic, Span as S};

        let mut blocks = Vec::new();

        // === Unprocessed Notifications ===
        let unprocessed: Vec<_> = SpineState::get(state).notifications.iter().filter(|n| n.is_unprocessed()).collect();

        if unprocessed.is_empty() {
            blocks.push(Block::Line(vec![S::muted("No unprocessed notifications".into()).italic()]));
        } else {
            for n in &unprocessed {
                let type_sem = notification_type_semantic(n.kind);
                let ts = format_timestamp(n.timestamp_ms);
                blocks.push(Block::Line(vec![
                    S::styled(format!("{} ", n.id), type_sem).bold(),
                    S::muted(format!("{ts} ")),
                    S::styled(n.kind.label().to_string(), type_sem),
                    S::new(format!(" — {}", n.content)),
                ]));
            }
        }

        blocks.push(Block::Empty);

        // === Blocked Notifications ===
        let blocked: Vec<_> = SpineState::get(state)
            .notifications
            .iter()
            .filter(|n| n.status == crate::types::NotificationStatus::Blocked)
            .collect();

        if !blocked.is_empty() {
            blocks.push(Block::Line(vec![S::warning(format!("Blocked ({})", blocked.len()))]));
            for n in &blocked {
                let ts = format_timestamp(n.timestamp_ms);
                blocks.push(Block::Line(vec![
                    S::styled(format!("{} ", n.id), Semantic::Warning),
                    S::muted(format!("{ts} ")),
                    S::warning(n.kind.label().to_string()),
                    S::muted(format!(" — {}", n.content)),
                ]));
            }
            blocks.push(Block::Empty);
        }

        // === Recent Processed ===
        let recent_processed: Vec<_> =
            SpineState::get(state).notifications.iter().filter(|n| n.is_processed()).rev().take(10).collect();

        if !recent_processed.is_empty() {
            blocks.push(Block::Line(vec![S::muted(format!("Processed ({})", recent_processed.len()))]));
            for n in &recent_processed {
                let type_sem = notification_type_semantic(n.kind);
                let ts = format_timestamp(n.timestamp_ms);
                blocks.push(Block::Line(vec![
                    S::styled(format!("{} ", n.id), type_sem),
                    S::muted(format!("{ts} ")),
                    S::muted(n.kind.label().to_string()),
                    S::muted(format!(" — {}", n.content)),
                ]));
            }
        }

        blocks.push(Block::Empty);

        // === Config Summary ===
        blocks.push(Block::Line(vec![S::styled("Config".into(), Semantic::Code)]));
        blocks.push(Block::KeyValue(vec![
            (
                vec![S::muted("  continue_until_todos_done".into())],
                vec![S::new(format!("{}", SpineState::get(state).config.continue_until_todos_done))],
            ),
            (
                vec![S::muted("  auto_continuations".into())],
                vec![S::new(format!("{}", SpineState::get(state).config.auto_continuation_count))],
            ),
        ]));

        // === Active Watchers ===
        if let Some(registry) = state.get_ext::<WatcherRegistry>() {
            let watchers = registry.active_watchers();
            if !watchers.is_empty() {
                blocks.push(Block::Empty);
                blocks.push(Block::Line(vec![S::accent(format!("Active Watchers ({})", watchers.len()))]));
                let now = now_ms();
                for w in watchers {
                    let age_s = cp_base::panels::time_arith::ms_to_secs(now.saturating_sub(w.registered_ms()));
                    let (mode_icon, mode_sem) =
                        if w.is_blocking() { ("⏳", Semantic::Warning) } else { ("👁", Semantic::Code) };
                    blocks.push(Block::Line(vec![
                        S::styled(format!("  {mode_icon} "), mode_sem),
                        S::new(w.description().to_string()),
                        S::muted(format!(" ({age_s}s)")),
                    ]));
                }
            }
        }

        blocks
    }
    fn title(&self, _state: &State) -> String {
        "Spine".to_string()
    }

    fn refresh(&self, state: &mut State) {
        let content = Self::format_notifications_for_context(state);
        let token_count = estimate_tokens(&content);

        for ctx in &mut state.context {
            if ctx.context_type.as_str() == Kind::SPINE {
                ctx.token_count = token_count;
                let _ = cp_base::panels::update_if_changed(ctx, &content);
                break;
            }
        }
    }

    fn max_freezes(&self) -> u8 {
        2
    }

    fn context(&self, state: &State) -> Vec<ContextItem> {
        let content = Self::format_notifications_for_context(state);
        let (id, last_refresh_ms) = state
            .context
            .iter()
            .find(|c| c.context_type.as_str() == Kind::SPINE)
            .map_or(("P9", 0), |c| (c.id.as_str(), c.last_refresh_ms));
        vec![ContextItem::new(id, "Spine", content, last_refresh_ms)]
    }
}

/// Map a notification type to its IR semantic token.
const fn notification_type_semantic(nt: NotificationType) -> cp_render::Semantic {
    match nt {
        NotificationType::UserMessage => cp_render::Semantic::Accent,
        NotificationType::ReloadResume | NotificationType::Custom => cp_render::Semantic::Code,
    }
}
