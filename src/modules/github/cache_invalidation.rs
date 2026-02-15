//! Cache invalidation heuristics for GitHub CLI commands.
//!
//! When a mutating `gh` command is executed (e.g., `gh issue close 22`),
//! we can proactively invalidate related read-only panels instead of
//! waiting for the next poll cycle. Each heuristic maps a mutating command
//! pattern to a list of panel command patterns that should be invalidated.
//!
//! The mutating command regex is matched against the full command string.
//! The invalidation regexes are matched against each GithubResult panel's
//! `result_command` field.

use regex::Regex;

/// A single invalidation rule: if a mutating command matches `trigger`,
/// then all panels whose command matches any entry in `invalidates` should
/// be marked as cache-deprecated.
pub struct InvalidationRule {
    pub trigger: Regex,
    /// Invalidation patterns as template strings. May contain backreferences
    /// like `\1`, `\2` which will be substituted with captured groups from
    /// the trigger at match time, then compiled into regexes.
    pub invalidates: Vec<String>,
}

impl InvalidationRule {
    fn new(trigger: &str, invalidates: &[&str]) -> Self {
        Self {
            trigger: Regex::new(trigger).expect("invalid trigger regex"),
            invalidates: invalidates.iter().map(|p| p.to_string()).collect(),
        }
    }
}

/// Build the full list of invalidation heuristics.
///
/// Each rule says: "if the mutating command matches this pattern,
/// then invalidate all panels matching these patterns."
pub fn build_invalidation_rules() -> Vec<InvalidationRule> {
    vec![
        // =====================================================================
        // Issues
        // =====================================================================

        // gh issue close/reopen/edit/delete/transfer/pin/unpin <number>
        // → invalidate: issue list (any flags), issue view <same number>
        InvalidationRule::new(
            r"^gh\s+issue\s+(close|reopen|edit|delete|transfer|pin|unpin)\s+(\d+)",
            &[
                r"^gh\s+issue\s+list",
                r"^gh\s+issue\s+view\s+\2\b", // backreference to issue number
                r"^gh\s+issue\s+status",
            ],
        ),
        // gh issue create → invalidate all issue lists + issue status
        InvalidationRule::new(r"^gh\s+issue\s+create", &[r"^gh\s+issue\s+list", r"^gh\s+issue\s+status"]),
        // gh issue comment <number> → invalidate issue view for that number
        InvalidationRule::new(r"^gh\s+issue\s+comment\s+(\d+)", &[r"^gh\s+issue\s+view\s+\1\b"]),
        // gh issue label add/remove → invalidate issue list + issue view + issue status
        InvalidationRule::new(
            r"^gh\s+issue\s+label\s+(add|remove)\s+(\d+)",
            &[r"^gh\s+issue\s+list", r"^gh\s+issue\s+view\s+\2\b", r"^gh\s+issue\s+status"],
        ),
        // gh issue assign/unassign → invalidate issue view + issue status
        InvalidationRule::new(
            r"^gh\s+issue\s+(assign|unassign)\s+(\d+)",
            &[r"^gh\s+issue\s+view\s+\2\b", r"^gh\s+issue\s+status"],
        ),
        // =====================================================================
        // Pull Requests
        // =====================================================================

        // gh pr close/reopen/edit/merge <number>
        // → invalidate: pr list, pr view <same number>, pr status
        InvalidationRule::new(
            r"^gh\s+pr\s+(close|reopen|edit|merge)\s+(\d+)",
            &[r"^gh\s+pr\s+list", r"^gh\s+pr\s+view\s+\2\b", r"^gh\s+pr\s+status"],
        ),
        // gh pr create → invalidate all pr lists + pr status
        InvalidationRule::new(r"^gh\s+pr\s+create", &[r"^gh\s+pr\s+list", r"^gh\s+pr\s+status"]),
        // gh pr review <number> → invalidate pr view
        InvalidationRule::new(r"^gh\s+pr\s+review\s+(\d+)", &[r"^gh\s+pr\s+view\s+\1\b"]),
        // gh pr comment <number> → invalidate pr view
        InvalidationRule::new(r"^gh\s+pr\s+comment\s+(\d+)", &[r"^gh\s+pr\s+view\s+\1\b"]),
        // gh pr ready/draft <number> → invalidate pr list + pr view + pr status
        InvalidationRule::new(
            r"^gh\s+pr\s+(ready|draft)\s+(\d+)",
            &[r"^gh\s+pr\s+list", r"^gh\s+pr\s+view\s+\2\b", r"^gh\s+pr\s+status"],
        ),
        // =====================================================================
        // Releases
        // =====================================================================

        // gh release create/edit/delete → invalidate release list + release view
        InvalidationRule::new(
            r"^gh\s+release\s+(create|edit|delete)",
            &[r"^gh\s+release\s+list", r"^gh\s+release\s+view"],
        ),
        // =====================================================================
        // Labels
        // =====================================================================

        // gh label create/edit/delete → invalidate label list + issue list
        InvalidationRule::new(r"^gh\s+label\s+(create|edit|delete)", &[r"^gh\s+label\s+list", r"^gh\s+issue\s+list"]),
        // =====================================================================
        // Repository
        // =====================================================================

        // gh repo edit → invalidate repo view
        InvalidationRule::new(r"^gh\s+repo\s+edit", &[r"^gh\s+repo\s+view"]),
        // =====================================================================
        // API catch-all: any gh api POST/PUT/PATCH/DELETE → invalidate all gh api panels
        // This is a broad fallback for direct API calls
        // =====================================================================
        InvalidationRule::new(r"^gh\s+api\s+.+\s+-(X|method)\s+(POST|PUT|PATCH|DELETE)", &[r"^gh\s+api\s+"]),
    ]
}

/// Given a mutating command string, find all panel command patterns that
/// should be invalidated. Returns a list of compiled regexes to match
/// against panel `result_command` fields.
///
/// Because regex backreferences (e.g., `\1`, `\2`) are not supported by
/// the `regex` crate, we extract capture groups from the trigger and
/// substitute them into the invalidation patterns manually.
pub fn find_invalidations(mutating_command: &str) -> Vec<Regex> {
    let rules = build_invalidation_rules();
    let mut result = Vec::new();

    for rule in &rules {
        if let Some(captures) = rule.trigger.captures(mutating_command) {
            for pattern_template in &rule.invalidates {
                // Substitute backreferences \1, \2, etc. with captured groups
                let mut resolved = pattern_template.clone();

                // Replace \1 through \9 with captured groups
                for i in 1..=9 {
                    let backref = format!("\\{}", i);
                    if let Some(group) = captures.get(i) {
                        resolved = resolved.replace(&backref, &regex::escape(group.as_str()));
                    }
                }

                if let Ok(re) = Regex::new(&resolved) {
                    result.push(re);
                }
            }
            // Don't break — multiple rules might match
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_issue_close_invalidates_list_and_view() {
        let invalidations = find_invalidations("gh issue close 42");
        assert!(!invalidations.is_empty());

        // Should invalidate issue list
        assert!(invalidations.iter().any(|re| re.is_match("gh issue list")));
        assert!(invalidations.iter().any(|re| re.is_match("gh issue list --state all")));

        // Should invalidate issue view 42
        assert!(invalidations.iter().any(|re| re.is_match("gh issue view 42")));

        // Should NOT invalidate issue view 43
        assert!(!invalidations.iter().any(|re| re.is_match("gh issue view 43")));
    }

    #[test]
    fn test_issue_create_invalidates_list() {
        let invalidations = find_invalidations("gh issue create --title \"test\" --body \"body\"");
        assert!(invalidations.iter().any(|re| re.is_match("gh issue list")));
    }

    #[test]
    fn test_pr_merge_invalidates_list_and_view() {
        let invalidations = find_invalidations("gh pr merge 20");
        assert!(invalidations.iter().any(|re| re.is_match("gh pr list")));
        assert!(invalidations.iter().any(|re| re.is_match("gh pr view 20")));
        assert!(invalidations.iter().any(|re| re.is_match("gh pr status")));
        assert!(!invalidations.iter().any(|re| re.is_match("gh pr view 21")));
    }

    #[test]
    fn test_no_invalidation_for_read_commands() {
        let invalidations = find_invalidations("gh issue list");
        assert!(invalidations.is_empty());

        let invalidations = find_invalidations("gh pr view 5");
        assert!(invalidations.is_empty());
    }

    #[test]
    fn test_label_create_invalidates_label_and_issue_list() {
        let invalidations = find_invalidations("gh label create bug --color FF0000");
        assert!(invalidations.iter().any(|re| re.is_match("gh label list")));
        assert!(invalidations.iter().any(|re| re.is_match("gh issue list")));
    }

    #[test]
    fn test_issue_close_invalidates_status() {
        let invalidations = find_invalidations("gh issue close 42");
        assert!(invalidations.iter().any(|re| re.is_match("gh issue status")));
    }

    #[test]
    fn test_pr_close_invalidates_status() {
        let invalidations = find_invalidations("gh pr close 20");
        assert!(invalidations.iter().any(|re| re.is_match("gh pr status")));
        assert!(invalidations.iter().any(|re| re.is_match("gh pr list")));
        assert!(invalidations.iter().any(|re| re.is_match("gh pr view 20")));
    }

    #[test]
    fn test_pr_create_invalidates_list_and_status() {
        let invalidations = find_invalidations("gh pr create --title \"test\" --base main");
        assert!(invalidations.iter().any(|re| re.is_match("gh pr list")));
        assert!(invalidations.iter().any(|re| re.is_match("gh pr status")));
    }

    #[test]
    fn test_issue_comment_only_invalidates_view() {
        let invalidations = find_invalidations("gh issue comment 21 --body \"test\"");
        assert!(invalidations.iter().any(|re| re.is_match("gh issue view 21")));
        // Should NOT invalidate issue list
        assert!(!invalidations.iter().any(|re| re.is_match("gh issue list")));
    }

    #[test]
    fn test_issue_edit_invalidates_list_view_and_status() {
        let invalidations = find_invalidations("gh issue edit 21 --title \"new title\"");
        assert!(invalidations.iter().any(|re| re.is_match("gh issue list")));
        assert!(invalidations.iter().any(|re| re.is_match("gh issue view 21")));
        assert!(invalidations.iter().any(|re| re.is_match("gh issue status")));
    }

    #[test]
    fn test_api_post_invalidates_all_api_panels() {
        let invalidations = find_invalidations("gh api /repos/owner/repo/issues -X POST");
        assert!(invalidations.iter().any(|re| re.is_match("gh api /repos/owner/repo/issues")));
        assert!(invalidations.iter().any(|re| re.is_match("gh api /repos/owner/repo")));
    }

    #[test]
    fn test_release_create_invalidates_release_panels() {
        let invalidations = find_invalidations("gh release create v1.0.0 --title \"Release 1.0\"");
        assert!(invalidations.iter().any(|re| re.is_match("gh release list")));
        assert!(invalidations.iter().any(|re| re.is_match("gh release view v1.0.0")));
    }
}
