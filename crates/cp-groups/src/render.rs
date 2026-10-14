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
/// message counts, members, and `social:` — the substates concerning `me`
/// in the user's notation, always present (`[]` when none).
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
    out.push_str(&social_line(group, me));
    out
}

/// Whether `sub` names `me`: as its agent, a gather member or origin, a
/// dispatch candidate (an empty dispatch concerns everyone), or a recurrent's
/// creator.
fn concerns(sub: &Substate, me: &str) -> bool {
    cp_macros::deref_match!(sub, {
        Substate::SimpleTurn { ref agent_id } | Substate::ComingBack { ref agent_id, .. } => agent_id == me,
        Substate::Gather { ref pending, ref origin_agent_id } => {
            origin_agent_id == me || pending.iter().any(|p| p == me)
        }
        Substate::Dispatch { ref candidates } => candidates.is_empty() || candidates.iter().any(|c| c == me),
        Substate::Recurrent { ref agent_id, ref creator_agent_id, .. } => {
            agent_id == me || creator_agent_id.as_deref() == Some(me)
        }
    })
}

/// `sub` in the user's notation: `simple_turn(A1)`,
/// `coming_back(A1, <local time>, "msg")`, `recurrent(A1, "cron", "msg")`,
/// `gather([A2, A3], A1)`, `dispatch([A2])`.
#[must_use]
pub fn substate_label(sub: &Substate) -> String {
    cp_macros::deref_match!(sub, {
        Substate::SimpleTurn { ref agent_id } => format!("simple_turn({agent_id})"),
        Substate::ComingBack { ref agent_id, at_ms, ref message, .. } => {
            let at = i64::try_from(at_ms)
                .ok()
                .and_then(cp_mod_utilities::time::epoch_ms_to_local_ymd_hms)
                .unwrap_or_else(|| at_ms.to_string());
            format!("coming_back({agent_id}, {at}, \"{}\")", yaml_escape(message))
        }
        Substate::Recurrent { ref agent_id, ref cron, ref message, .. } => {
            format!("recurrent({agent_id}, \"{cron}\", \"{}\")", yaml_escape(message))
        }
        Substate::Gather { ref pending, ref origin_agent_id } => {
            format!("gather([{}], {origin_agent_id})", pending.join(", "))
        }
        Substate::Dispatch { ref candidates } => format!("dispatch([{}])", candidates.join(", ")),
    })
}

/// `    social: [...]` line of one group entry: its substates concerning
/// `me`, e.g. `social: [simple_turn(A1), dispatch([])]`.
fn social_line(group: &Group, me: &str) -> String {
    let labels: Vec<String> = group.social.iter().filter(|s| concerns(s, me)).map(substate_label).collect();
    format!("    social: [{}]\n", labels.join(", "))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn group(id: &str, social: Vec<Substate>) -> Group {
        Group { id: id.into(), social, ..Group::default() }
    }

    #[test]
    fn social_lists_only_my_substates_in_user_notation() {
        let g3 = group(
            "G3",
            vec![
                Substate::SimpleTurn { agent_id: "A1".into() },
                Substate::SimpleTurn { agent_id: "A9".into() },
                Substate::Gather { pending: vec!["A2".into(), "A3".into()], origin_agent_id: "A1".into() },
                Substate::Dispatch { candidates: vec![] },
                Substate::Recurrent {
                    id: "coucou_1".into(),
                    agent_id: "A1".into(),
                    creator_agent_id: None,
                    cron: "0 9 * * 1-5".into(),
                    message: "say \"hi\"".into(),
                },
            ],
        );
        assert_eq!(
            social_line(&g3, "A1"),
            "    social: [simple_turn(A1), gather([A2, A3], A1), dispatch([]), recurrent(A1, \"0 9 * * 1-5\", \"say \\\"hi\\\"\")]\n"
        );
    }

    #[test]
    fn social_is_empty_list_when_nothing_concerns_me() {
        let g = group("G1", vec![Substate::Dispatch { candidates: vec!["A2".into()] }]);
        assert!(group_entry(&g, "A1").ends_with("    social: []\n"));
    }

    #[test]
    fn coming_back_shows_local_time_and_message() {
        let sub = Substate::ComingBack { agent_id: "A1".into(), at_ms: 0, message: "m".into(), from: None };
        let label = substate_label(&sub);
        assert!(label.starts_with("coming_back(A1, 19") && label.ends_with(", \"m\")"), "{label}");
    }
}
