//! Which `GithubResult` panels a mutating `gh` command makes stale.
//!
//! Structural, not textual: both commands are parsed into args, so flag order
//! (`gh pr merge --squash 12`), a missing number (`gh pr merge` on the current
//! branch) and every `--method` spelling are handled alike. Over-invalidating
//! costs one extra `gh` call; under-invalidating shows stale data, so every
//! doubt resolves to "stale".

use super::{CommandClass, classify, validate_gh_command};

/// Panel groups a mutation in `group` can change, besides `group` itself.
fn related_groups(group: &str) -> &'static [&'static str] {
    match group {
        "pr" => &["issue", "status", "search"],
        "issue" => &["pr", "status", "search"],
        "label" => &["issue", "pr"],
        "run" | "workflow" => &["run", "workflow", "pr"],
        "release" => &["repo"],
        "repo" => &["release"],
        _ => &[],
    }
}

/// Panel actions that show ONE issue/PR: only refreshed when the number matches.
const SINGLE_ITEM_ACTIONS: &[&str] = &["view", "diff", "checks"];

/// Issue/PR numbers referenced by `args`: `12`, `#12`, or a `/pull/12` or
/// `/issues/12` URL. Flag values are included on purpose (cheap, conservative).
fn item_numbers(args: &[String]) -> Vec<u64> {
    args.iter().filter_map(|a| item_number(a)).collect()
}

/// Issue/PR number in one argument, if any.
fn item_number(arg: &str) -> Option<u64> {
    let tail = arg
        .rsplit_once("/pull/")
        .or_else(|| arg.rsplit_once("/issues/"))
        .map_or(arg, |(_, n)| n)
        .trim_start_matches('#');
    let digits = tail.split(['/', '#', '?']).next().unwrap_or("");
    (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())).then(|| digits.parse().ok()).flatten()
}

/// Whether the mutation `mutating` (args after `gh`) can change what the
/// panel running `panel_command` shows.
#[must_use]
pub(crate) fn affects(mutating: &[String], panel_command: &str) -> bool {
    let Ok(panel) = validate_gh_command(panel_command) else { return false };
    if classify(&panel) != CommandClass::ReadOnly {
        return false; // never re-run a run-once command
    }
    let (Some(m_group), Some(p_group)) = (mutating.first(), panel.first()) else { return false };
    if m_group == "api" || p_group == "api" {
        return true; // raw API: endpoint mapping is unknowable, assume overlap
    }
    if m_group != p_group {
        return related_groups(m_group).contains(&p_group.as_str());
    }
    let p_action = panel.get(1).map_or("", String::as_str);
    if !SINGLE_ITEM_ACTIONS.contains(&p_action) {
        return true; // lists, status, etc.
    }
    let panel_numbers = item_numbers(panel.get(2..).unwrap_or_default());
    let mut_numbers = item_numbers(mutating.get(2..).unwrap_or_default());
    // Unknown on either side (current branch, branch name) → assume it's the same item.
    panel_numbers.is_empty() || mut_numbers.is_empty() || panel_numbers.iter().any(|n| mut_numbers.contains(n))
}
