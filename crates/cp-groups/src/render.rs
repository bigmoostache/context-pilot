//! LLM-facing YAML rendering of groups (the `groups` and `group` panels).

use std::fmt::Write as _;

use crate::chat::GroupMessage;
use crate::social::Substate;
use crate::types::{Group, GroupKind};

/// Maximum messages shown in a `group` panel.
const MAX_PANEL_MESSAGES: usize = 50;

/// Escape `\` and `"` for a double-quoted YAML scalar.
#[must_use]
pub fn yaml_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Relative age between two epoch-ms timestamps (`5s ago`, `3m ago`, `1h02m ago`).
#[must_use]
pub fn format_age(now_ms: u64, ts_ms: u64) -> String {
    let diff_s = now_ms.saturating_sub(ts_ms).wrapping_div(1000);
    if diff_s < 60 {
        return format!("{diff_s}s ago");
    }
    let mins = diff_s.wrapping_div(60);
    if mins < 60 {
        return format!("{mins}m ago");
    }
    format!("{}h{:02}m ago", mins.wrapping_div(60), mins.wrapping_rem(60))
}

/// Lowercase label of a [`GroupKind`].
#[must_use]
pub const fn kind_label(kind: GroupKind) -> &'static str {
    match kind {
        GroupKind::Human => "human",
        GroupKind::Coordination => "coordination",
        GroupKind::Custom => "custom",
    }
}

/// One `groups` panel YAML entry: id, name, description, kind, owner,
/// message counts, members, and the social substates naming `me`.
#[must_use]
pub fn group_entry(group: &Group, me: &str) -> String {
    let unread = group.unread_for(me);
    let mut out = String::new();
    _ = writeln!(out, "  - id: {}", group.id);
    _ = writeln!(out, "    name: \"{}\"", yaml_escape(&group.name));
    if !group.description.is_empty() {
        _ = writeln!(out, "    description: \"{}\"", yaml_escape(&group.description));
    }
    _ = writeln!(out, "    kind: {}", kind_label(group.kind));
    _ = writeln!(out, "    owner: {}", group.owner_agent_id);
    _ = writeln!(out, "    messages: {}", group.messages.len());
    if unread > 0 {
        _ = writeln!(out, "    unread: {unread}");
    }
    _ = writeln!(out, "    members: [{}]", group.member_ids.join(", "));
    let mine: Vec<String> = group.social.iter().filter_map(|s| substate_label(s, me)).collect();
    if !mine.is_empty() {
        _ = writeln!(out, "    social: [{}]", mine.join(", "));
    }
    out
}

/// Short label of `sub` when it concerns `me`.
fn substate_label(sub: &Substate, me: &str) -> Option<String> {
    cp_macros::deref_match!(sub, {
        Substate::SimpleTurn { ref agent_id } => (agent_id == me).then(|| "my_turn".to_owned()),
        Substate::ComingBack { ref agent_id, at_ms, .. } => {
            (agent_id == me).then(|| format!("coming_back_at_ms {at_ms}"))
        }
        Substate::Gather { ref pending, ref origin_agent_id } => {
            if pending.iter().any(|p| p == me) {
                Some(format!("gather_owed_to {origin_agent_id}"))
            } else {
                (origin_agent_id == me).then(|| format!("gathering [{}]", pending.join(" ")))
            }
        }
        Substate::Dispatch { ref candidates } => {
            candidates.iter().any(|c| c == me).then(|| "dispatch_candidate".to_owned())
        }
    })
}

/// Full `group` panel body: header, members, last [`MAX_PANEL_MESSAGES`]
/// non-auto messages (tool traces stay hidden from the LLM).
#[must_use]
pub fn group_body(group: &Group, now_ms: u64) -> String {
    let mut out = String::new();
    _ = writeln!(out, "group_id: {}", group.id);
    _ = writeln!(out, "name: \"{}\"", yaml_escape(&group.name));
    if !group.description.is_empty() {
        _ = writeln!(out, "description: \"{}\"", yaml_escape(&group.description));
    }
    _ = writeln!(out, "kind: {}", kind_label(group.kind));
    _ = writeln!(out, "owner: {}", group.owner_agent_id);
    _ = writeln!(out, "members: [{}]", group.member_ids.join(", "));

    let visible: Vec<&GroupMessage> = group.messages.iter().filter(|m| !m.auto).collect();
    let skip = visible.len().saturating_sub(MAX_PANEL_MESSAGES);
    if skip > 0 {
        _ = writeln!(out, "omitted: {skip}");
    }
    if visible.is_empty() {
        _ = writeln!(out, "messages: []");
        return out;
    }
    _ = writeln!(out, "messages:");
    for msg in visible.iter().skip(skip) {
        write_message(&mut out, msg, now_ms);
    }
    out
}

/// One YAML message entry (author, age, text block or inline, file).
fn write_message(out: &mut String, msg: &GroupMessage, now_ms: u64) {
    match msg.author_agent_id.as_deref() {
        Some(agent) => _ = writeln!(out, "  - author: {} ({agent})", msg.author),
        None => _ = writeln!(out, "  - author: {}", msg.author),
    }
    _ = writeln!(out, "    ts: \"{}\"", format_age(now_ms, msg.timestamp));
    let content = msg.content.as_deref().unwrap_or("[no text]");
    if content.contains('\n') {
        _ = writeln!(out, "    text: |");
        for line in content.lines() {
            _ = writeln!(out, "      {line}");
        }
    } else {
        _ = writeln!(out, "    text: \"{}\"", yaml_escape(content));
    }
    if let Some(fp) = msg.file_path.as_ref() {
        _ = writeln!(out, "    file: \"{}\"", yaml_escape(fp));
    }
}
