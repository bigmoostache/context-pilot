use regex::Regex;

/// A rule mapping a mutating git command pattern to panels it should invalidate.
pub(crate) struct InvalidationRule {
    /// Regex matching the mutating git command.
    pub trigger: Regex,
    /// Patterns of git commands whose cached results become stale.
    pub invalidates: Vec<String>,
}

impl InvalidationRule {
    /// Create a rule from a trigger pattern and a list of invalidation patterns.
    fn new(trigger: &str, invalidates: &[&str]) -> Option<Self> {
        Some(Self {
            trigger: Regex::new(trigger).ok()?,
            invalidates: invalidates.iter().map(ToString::to_string).collect(),
        })
    }
}

/// Build the full list of cache invalidation rules for git commands.
pub(crate) fn build_invalidation_rules() -> Vec<InvalidationRule> {
    vec![
        // NUCLEAR — invalidate ALL GitResult panels
        InvalidationRule::new(
            r"^git\s+(checkout|switch|merge|rebase|reset|pull|filter-branch|filter-repo)\b",
            &[r"^git\s+"],
        ),
        // COMMIT-LIKE — new commit on current branch
        InvalidationRule::new(
            r"^git\s+(commit|cherry-pick|revert|am)\b",
            &[
                r"^git\s+(log|diff|show|status|blame|shortlog|rev-list|rev-parse|ls-tree|for-each-ref|describe|reflog|cat-file|format-patch)\b",
            ],
        ),
        // STAGING — index/working tree changes
        InvalidationRule::new(
            r"^git\s+(add|restore|rm|mv|clean|update-index)\b",
            &[r"^git\s+(diff|status|ls-files|grep|blame)\b"],
        ),
        // STASH_MODIFY — stash push/pop/apply changes working tree + stash list
        InvalidationRule::new(
            r"^git\s+stash(\s+(push|pop|apply)|\s*$)",
            &[r"^git\s+(diff|status|stash|ls-files|grep)\b"],
        ),
        // STASH_REMOVE — stash drop/clear only affects stash list
        InvalidationRule::new(r"^git\s+stash\s+(drop|clear)\b", &[r"^git\s+stash\b"]),
        // PUSH — only remote tracking changes
        InvalidationRule::new(r"^git\s+push\b", &[r"^git\s+log\b"]),
        // FETCH — updates remote refs
        InvalidationRule::new(r"^git\s+fetch\b", &[r"^git\s+(log|branch|tag|for-each-ref)\b"]),
        // BRANCH_MGMT — create/delete/rename branches
        InvalidationRule::new(r"^git\s+branch\s+(-d|-D|-m|-M|-c|-C|[^-])", &[r"^git\s+(branch|for-each-ref|reflog)\b"]),
        // TAG_MGMT — create/delete tags
        InvalidationRule::new(r"^git\s+tag\s+(-d|[^-])", &[r"^git\s+(tag|for-each-ref|describe)\b"]),
        // CONFIG — config changes
        InvalidationRule::new(r"^git\s+config\b", &[r"^git\s+config\b"]),
        // REMOTE — remote management
        InvalidationRule::new(
            r"^git\s+remote\s+(add|remove|rm|rename|set-url|set-head|prune)\b",
            &[r"^git\s+remote\b"],
        ),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Return regexes matching git commands whose caches should be invalidated by `mutating_command`.
pub(crate) fn find_invalidations(mutating_command: &str) -> Vec<Regex> {
    let rules = build_invalidation_rules();
    let mut result = Vec::new();
    for rule in &rules {
        if rule.trigger.is_match(mutating_command) {
            for pattern in &rule.invalidates {
                if let Ok(re) = Regex::new(pattern) {
                    result.push(re);
                }
            }
        }
    }
    result
}
