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

/// Append a labeled `[id] time type — content` list for one notification group.
fn push_notif_list(output: &mut String, header: Option<&str>, notifs: &[&crate::types::Notification]) {
    if let Some(h) = header {
        output.push_str(h);
    }
    for n in notifs {
        let ts = format_timestamp(n.timestamp_ms);
        let _r = writeln!(output, "[{}] {} {} — {}", n.id, ts, n.kind.label(), n.content);
    }
}

/// Append the spine config summary (continuation flags + counters + retry cap).
fn push_config_summary(output: &mut String, state: &State) {
    output.push_str("\n=== Spine Config ===\n");
    let cfg = &SpineState::get(state).config;
    let _r1 = writeln!(output, "continue_until_todos_done: {}", cfg.continue_until_todos_done);
    let _r2 = writeln!(output, "auto_continuation_count: {}", cfg.auto_continuation_count);
    if let Some(v) = cfg.max_auto_retries {
        let _r3 = writeln!(output, "max_auto_retries: {v}");
    }
}

/// Append the active-watchers list (mode, recurrence, age), if any.
fn push_watchers_summary(output: &mut String, state: &State) {
    let Some(registry) = state.get_ext::<WatcherRegistry>() else {
        return;
    };
    let watchers = registry.active_watchers();
    if watchers.is_empty() {
        return;
    }
    output.push_str("\n=== Active Watchers ===\n");
    let now = now_ms();
    for w in watchers {
        let age_s = cp_base::panels::time_arith::ms_to_secs(now.saturating_sub(w.registered_ms()));
        let mode = if w.is_blocking() { "blocking" } else { "async" };
        let recurrence = if w.interval_ms() > 0 {
            w.recurrence_label().map_or_else(|| " recurrent".to_owned(), |l| format!(" {l}"))
        } else {
            String::new()
        };
        let _r4 = writeln!(output, "[{}] {} ({}{}, {}s ago)", w.id(), w.description(), mode, recurrence, age_s);
    }
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
            push_notif_list(&mut output, None, &unprocessed);
        }
        if !blocked.is_empty() {
            push_notif_list(&mut output, Some("\n=== Blocked (awaiting guard rail clearance) ===\n"), &blocked);
        }
        if !recent_processed.is_empty() {
            push_notif_list(&mut output, Some("\n=== Recent Processed ===\n"), &recent_processed);
        }
        push_config_summary(&mut output, state);
        push_watchers_summary(&mut output, state);

        output.trim_end().to_owned()
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
        use cp_render::{Block, Span as S};

        let mut blocks = Vec::new();

        // === Unprocessed Notifications ===
        let unprocessed: Vec<_> = SpineState::get(state).notifications.iter().filter(|n| n.is_unprocessed()).collect();
        if unprocessed.is_empty() {
            blocks.push(Block::Line(vec![S::muted("No unprocessed notifications.".into()).italic()]));
        } else {
            push_notif_table(&mut blocks, "Unprocessed", cp_render::Semantic::Accent, &unprocessed);
        }

        // === Blocked Notifications ===
        let blocked: Vec<_> = SpineState::get(state)
            .notifications
            .iter()
            .filter(|n| n.status == crate::types::NotificationStatus::Blocked)
            .collect();
        if !blocked.is_empty() {
            blocks.push(Block::Empty);
            push_notif_table(&mut blocks, "Blocked", cp_render::Semantic::Warning, &blocked);
        }

        // === Recent Processed ===
        let recent_processed: Vec<_> =
            SpineState::get(state).notifications.iter().filter(|n| n.is_processed()).rev().take(10).collect();
        if !recent_processed.is_empty() {
            blocks.push(Block::Empty);
            push_notif_table(&mut blocks, "Recent Processed", cp_render::Semantic::Muted, &recent_processed);
        }

        blocks.push(Block::Empty);
        push_config_blocks(&mut blocks, state);
        push_watcher_blocks(&mut blocks, state);

        blocks
    }
    fn title(&self, _state: &State) -> String {
        "Spine".to_owned()
    }

    fn refresh(&self, state: &mut State) {
        let content = Self::format_notifications_for_context(state);
        let token_count = estimate_tokens(&content);

        for ctx in &mut state.context {
            if ctx.context_type.as_str() == Kind::SPINE {
                ctx.token_count = token_count;
                let _changed = cp_base::panels::update_if_changed(ctx, &content);
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

/// The 4-column layout (ID/Time/Type/Content) shared by every notification table.
fn notif_columns() -> Vec<cp_render::Column> {
    use cp_render::{Align, Column};
    vec![
        Column { header: "ID".to_owned(), align: Align::Left },
        Column { header: "Time".to_owned(), align: Align::Left },
        Column { header: "Type".to_owned(), align: Align::Left },
        Column { header: "Content".to_owned(), align: Align::Left },
    ]
}

/// Push a titled notification table (header with count + rows) into `blocks`.
fn push_notif_table(
    blocks: &mut Vec<cp_render::Block>,
    title: &str,
    header_sem: cp_render::Semantic,
    notifs: &[&crate::types::Notification],
) {
    use cp_render::{Block, Cell, Span as S};
    blocks
        .push(Block::Header(vec![S::styled(title.to_owned(), header_sem), S::muted(format!("  ({})", notifs.len()))]));
    let row_sem = if title == "Unprocessed" { None } else { Some(header_sem) };
    let rows: Vec<Vec<Cell>> = notifs
        .iter()
        .map(|n| notification_row(n, row_sem.unwrap_or_else(|| notification_type_semantic(n.kind))))
        .collect();
    blocks.push(Block::Table { columns: notif_columns(), rows });
}

/// Push the config summary (continuation flag + counter) into `blocks`.
fn push_config_blocks(blocks: &mut Vec<cp_render::Block>, state: &State) {
    use cp_render::{Block, Semantic, Span as S};
    let cfg = &SpineState::get(state).config;
    blocks.push(Block::Line(vec![S::styled("Config".into(), Semantic::Code)]));
    blocks.push(Block::KeyValue(vec![
        (
            vec![S::muted("  continue_until_todos_done".into())],
            vec![S::new(format!("{}", cfg.continue_until_todos_done))],
        ),
        (vec![S::muted("  auto_continuations".into())], vec![S::new(format!("{}", cfg.auto_continuation_count))]),
    ]));
}

/// Push the active-watchers list (mode icon, recurrence, age) into `blocks`, if any.
fn push_watcher_blocks(blocks: &mut Vec<cp_render::Block>, state: &State) {
    use cp_render::{Block, Semantic, Span as S};
    let Some(registry) = state.get_ext::<WatcherRegistry>() else {
        return;
    };
    let watchers = registry.active_watchers();
    if watchers.is_empty() {
        return;
    }
    blocks.push(Block::Empty);
    blocks.push(Block::Header(vec![S::accent(format!("Active Watchers ({})", watchers.len()))]));
    let now = now_ms();
    for w in watchers {
        let age_s = cp_base::panels::time_arith::ms_to_secs(now.saturating_sub(w.registered_ms()));
        let (mode_icon, mode_sem) = if w.is_blocking() { ("⏳", Semantic::Warning) } else { ("👁", Semantic::Code) };
        let recurrence_span = if w.interval_ms() > 0 {
            format!(" [{}]", w.recurrence_label().unwrap_or("recurrent"))
        } else {
            String::new()
        };
        blocks.push(Block::Line(vec![
            S::styled(format!("  {mode_icon} "), mode_sem),
            S::new(w.description().to_owned()),
            S::styled(recurrence_span, Semantic::Accent),
            S::muted(format!(" ({age_s}s)")),
        ]));
    }
}

/// Map a notification type to its IR semantic token.
const fn notification_type_semantic(nt: NotificationType) -> cp_render::Semantic {
    match nt {
        NotificationType::UserMessage => cp_render::Semantic::Accent,
        NotificationType::ReloadResume | NotificationType::Custom => cp_render::Semantic::Code,
    }
}

/// Build a table row for a single notification.
fn notification_row(n: &crate::types::Notification, semantic: cp_render::Semantic) -> Vec<cp_render::Cell> {
    use cp_render::Cell;

    let ts = format_timestamp(n.timestamp_ms);
    // Truncate content to keep the table from overflowing.
    let truncated = truncate_str(&n.content, 60);

    vec![
        Cell::styled(n.id.clone(), semantic),
        Cell::styled(ts, cp_render::Semantic::Muted),
        Cell::styled(n.kind.label().to_owned(), semantic),
        Cell::styled(truncated, cp_render::Semantic::Default),
    ]
}

/// Truncate a string to `max_len` characters, appending "…" if truncated.
fn truncate_str(s: &str, max_len: usize) -> String {
    let trimmed = s.replace('\n', " ");
    if trimmed.chars().count() <= max_len {
        trimmed
    } else {
        let mut result: String = trimmed.chars().take(max_len.saturating_sub(1)).collect();
        result.push('…');
        result
    }
}
