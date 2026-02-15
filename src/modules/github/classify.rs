//! Command classification for gh (GitHub CLI) commands.
use crate::modules::git::classify::{CommandClass, check_shell_operators, parse_shell_args};

/// Validate a raw command string intended for `gh`.
/// Returns parsed args on success, or an error message on failure.
pub fn validate_gh_command(command: &str) -> Result<Vec<String>, String> {
    let trimmed = command.trim();
    if !trimmed.starts_with("gh ") && trimmed != "gh" {
        return Err("Command must start with 'gh '".to_string());
    }

    check_shell_operators(trimmed)?;

    // Parse into args, skip "gh" prefix
    let all_args = parse_shell_args(trimmed)?;
    let args: Vec<String> = all_args.into_iter().skip(1).collect();

    if args.is_empty() {
        return Err("No gh subcommand specified".to_string());
    }

    Ok(args)
}

/// Classify a gh command (given as parsed args after "gh") as read-only or mutating.
pub fn classify_gh(args: &[String]) -> CommandClass {
    if args.is_empty() {
        return CommandClass::Mutating;
    }

    let group = args[0].as_str();
    let action = args.get(1).map(|s| s.as_str()).unwrap_or("");
    let rest: Vec<&str> = args.iter().skip(1).map(|s| s.as_str()).collect();

    match group {
        // PR commands
        "pr" => match action {
            "list" | "view" | "status" | "checks" | "diff" => CommandClass::ReadOnly,
            "create" | "merge" | "close" | "reopen" | "edit" | "comment" | "review" | "ready" => CommandClass::Mutating,
            _ => CommandClass::Mutating,
        },

        // Issue commands
        "issue" => match action {
            "list" | "view" | "status" => CommandClass::ReadOnly,
            "create" | "close" | "reopen" | "edit" | "comment" | "delete" | "transfer" | "pin" | "unpin" | "lock"
            | "unlock" => CommandClass::Mutating,
            _ => CommandClass::Mutating,
        },

        // Repo commands
        "repo" => match action {
            "view" | "list" => CommandClass::ReadOnly,
            "create" | "clone" | "fork" | "delete" | "edit" | "rename" | "archive" | "unarchive" | "sync" => {
                CommandClass::Mutating
            }
            _ => CommandClass::Mutating,
        },

        // Release commands
        "release" => match action {
            "list" | "view" | "download" => CommandClass::ReadOnly,
            "create" | "delete" | "edit" | "upload" => CommandClass::Mutating,
            _ => CommandClass::Mutating,
        },

        // Run (Actions) commands
        "run" => match action {
            "list" | "view" | "download" | "watch" => CommandClass::ReadOnly,
            "rerun" | "cancel" | "delete" => CommandClass::Mutating,
            _ => CommandClass::Mutating,
        },

        // Workflow commands
        "workflow" => match action {
            "list" | "view" => CommandClass::ReadOnly,
            "run" | "enable" | "disable" => CommandClass::Mutating,
            _ => CommandClass::Mutating,
        },

        // Gist commands
        "gist" => match action {
            "list" | "view" => CommandClass::ReadOnly,
            "create" | "edit" | "delete" | "clone" | "rename" => CommandClass::Mutating,
            _ => CommandClass::Mutating,
        },

        // Search commands (always read-only)
        "search" => CommandClass::ReadOnly,

        // Auth commands
        "auth" => match action {
            "status" | "token" => CommandClass::ReadOnly,
            _ => CommandClass::Mutating,
        },

        // API command — special handling
        "api" => {
            // Check --method flag
            let has_mutating_method = rest.windows(2).any(|w| {
                (w[0] == "--method" || w[0] == "-X")
                    && matches!(w[1].to_uppercase().as_str(), "POST" | "PUT" | "PATCH" | "DELETE")
            });
            if has_mutating_method { CommandClass::Mutating } else { CommandClass::ReadOnly }
        }

        // Label commands
        "label" => match action {
            "list" => CommandClass::ReadOnly,
            _ => CommandClass::Mutating,
        },

        // Project commands
        "project" => match action {
            "list" | "view" | "field-list" | "item-list" => CommandClass::ReadOnly,
            _ => CommandClass::Mutating,
        },

        // SSH key, GPG key commands
        "ssh-key" | "gpg-key" => match action {
            "list" => CommandClass::ReadOnly,
            _ => CommandClass::Mutating,
        },

        // Always read-only groups
        "browse" | "status" | "completion" | "help" | "version" => CommandClass::ReadOnly,

        // Attestation (verify, download — always read-only)
        "attestation" => CommandClass::ReadOnly,

        // Config commands
        "config" => match action {
            "get" | "list" => CommandClass::ReadOnly,
            _ => CommandClass::Mutating,
        },

        // Secret commands
        "secret" => match action {
            "list" => CommandClass::ReadOnly,
            _ => CommandClass::Mutating,
        },

        // Variable commands
        "variable" => match action {
            "list" | "get" => CommandClass::ReadOnly,
            _ => CommandClass::Mutating,
        },

        // Cache commands
        "cache" => match action {
            "list" => CommandClass::ReadOnly,
            _ => CommandClass::Mutating,
        },

        // Ruleset commands
        "ruleset" => match action {
            "list" | "view" | "check" => CommandClass::ReadOnly,
            _ => CommandClass::Mutating,
        },

        // Org commands
        "org" => match action {
            "list" => CommandClass::ReadOnly,
            _ => CommandClass::Mutating,
        },

        // Extension commands
        "extension" => match action {
            "list" | "search" | "browse" => CommandClass::ReadOnly,
            _ => CommandClass::Mutating,
        },

        // Alias commands
        "alias" => match action {
            "list" => CommandClass::ReadOnly,
            _ => CommandClass::Mutating,
        },

        // Codespace commands
        "codespace" => match action {
            "list" | "view" | "ssh" | "code" | "jupyter" | "logs" | "ports" => CommandClass::ReadOnly,
            _ => CommandClass::Mutating,
        },

        // Unknown → Mutating (safe default)
        _ => CommandClass::Mutating,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_rejects_non_gh() {
        assert!(validate_gh_command("git log").is_err());
    }

    #[test]
    fn test_validate_accepts_valid() {
        let args = validate_gh_command("gh pr list --json number").unwrap();
        assert_eq!(args, vec!["pr", "list", "--json", "number"]);
    }

    #[test]
    fn test_pr_list_readonly() {
        let args = vec!["pr".to_string(), "list".to_string()];
        assert_eq!(classify_gh(&args), CommandClass::ReadOnly);
    }

    #[test]
    fn test_pr_create_mutating() {
        let args = vec!["pr".to_string(), "create".to_string()];
        assert_eq!(classify_gh(&args), CommandClass::Mutating);
    }

    #[test]
    fn test_api_get_readonly() {
        let args = vec!["api".to_string(), "/repos/foo/bar".to_string()];
        assert_eq!(classify_gh(&args), CommandClass::ReadOnly);
    }

    #[test]
    fn test_api_post_mutating() {
        let args =
            vec!["api".to_string(), "/repos/foo/bar/issues".to_string(), "--method".to_string(), "POST".to_string()];
        assert_eq!(classify_gh(&args), CommandClass::Mutating);
    }

    #[test]
    fn test_run_watch_readonly() {
        let args = vec!["run".to_string(), "watch".to_string()];
        assert_eq!(classify_gh(&args), CommandClass::ReadOnly);
    }

    #[test]
    fn test_codespace_list_readonly() {
        let args = vec!["codespace".to_string(), "list".to_string()];
        assert_eq!(classify_gh(&args), CommandClass::ReadOnly);
    }

    #[test]
    fn test_secret_set_mutating() {
        let args = vec!["secret".to_string(), "set".to_string()];
        assert_eq!(classify_gh(&args), CommandClass::Mutating);
    }

    #[test]
    fn test_project_field_list_readonly() {
        let args = vec!["project".to_string(), "field-list".to_string()];
        assert_eq!(classify_gh(&args), CommandClass::ReadOnly);
    }

    #[test]
    fn test_browse_readonly() {
        let args = vec!["browse".to_string()];
        assert_eq!(classify_gh(&args), CommandClass::ReadOnly);
    }

    #[test]
    fn test_variable_get_readonly() {
        let args = vec!["variable".to_string(), "get".to_string()];
        assert_eq!(classify_gh(&args), CommandClass::ReadOnly);
    }

    #[test]
    fn test_validate_quoted_args() {
        let args = validate_gh_command("gh issue create --title \"my issue\" --body \"details here\"").unwrap();
        assert_eq!(args, vec!["issue", "create", "--title", "my issue", "--body", "details here"]);
    }

    #[test]
    fn test_validate_allows_pipe_inside_quotes() {
        let args = validate_gh_command("gh api /repos --jq \".[] | .name\"").unwrap();
        assert_eq!(args, vec!["api", "/repos", "--jq", ".[] | .name"]);
    }

    #[test]
    fn test_issue_close_mutating() {
        let args = vec!["issue".to_string(), "close".to_string(), "42".to_string()];
        assert_eq!(classify_gh(&args), CommandClass::Mutating);
    }

    #[test]
    fn test_issue_status_readonly() {
        let args = vec!["issue".to_string(), "status".to_string()];
        assert_eq!(classify_gh(&args), CommandClass::ReadOnly);
    }

    #[test]
    fn test_repo_view_readonly() {
        let args = vec!["repo".to_string(), "view".to_string()];
        assert_eq!(classify_gh(&args), CommandClass::ReadOnly);
    }

    #[test]
    fn test_repo_create_mutating() {
        let args = vec!["repo".to_string(), "create".to_string()];
        assert_eq!(classify_gh(&args), CommandClass::Mutating);
    }

    #[test]
    fn test_release_list_readonly() {
        let args = vec!["release".to_string(), "list".to_string()];
        assert_eq!(classify_gh(&args), CommandClass::ReadOnly);
    }

    #[test]
    fn test_release_create_mutating() {
        let args = vec!["release".to_string(), "create".to_string()];
        assert_eq!(classify_gh(&args), CommandClass::Mutating);
    }

    #[test]
    fn test_label_list_readonly() {
        let args = vec!["label".to_string(), "list".to_string()];
        assert_eq!(classify_gh(&args), CommandClass::ReadOnly);
    }

    #[test]
    fn test_label_create_mutating() {
        let args = vec!["label".to_string(), "create".to_string()];
        assert_eq!(classify_gh(&args), CommandClass::Mutating);
    }

    #[test]
    fn test_api_delete_mutating() {
        let args = vec!["api".to_string(), "/repos/foo/bar".to_string(), "-X".to_string(), "DELETE".to_string()];
        assert_eq!(classify_gh(&args), CommandClass::Mutating);
    }

    #[test]
    fn test_api_put_mutating() {
        let args = vec!["api".to_string(), "/repos/foo/bar".to_string(), "--method".to_string(), "PUT".to_string()];
        assert_eq!(classify_gh(&args), CommandClass::Mutating);
    }

    #[test]
    fn test_search_always_readonly() {
        let args = vec!["search".to_string(), "repos".to_string(), "rust".to_string()];
        assert_eq!(classify_gh(&args), CommandClass::ReadOnly);
        let args = vec!["search".to_string(), "issues".to_string()];
        assert_eq!(classify_gh(&args), CommandClass::ReadOnly);
    }

    #[test]
    fn test_workflow_list_readonly() {
        let args = vec!["workflow".to_string(), "list".to_string()];
        assert_eq!(classify_gh(&args), CommandClass::ReadOnly);
    }

    #[test]
    fn test_workflow_run_mutating() {
        let args = vec!["workflow".to_string(), "run".to_string()];
        assert_eq!(classify_gh(&args), CommandClass::Mutating);
    }

    #[test]
    fn test_gist_view_readonly() {
        let args = vec!["gist".to_string(), "view".to_string()];
        assert_eq!(classify_gh(&args), CommandClass::ReadOnly);
    }

    #[test]
    fn test_gist_create_mutating() {
        let args = vec!["gist".to_string(), "create".to_string()];
        assert_eq!(classify_gh(&args), CommandClass::Mutating);
    }

    #[test]
    fn test_unknown_group_mutating() {
        let args = vec!["unknown-thing".to_string(), "do-stuff".to_string()];
        assert_eq!(classify_gh(&args), CommandClass::Mutating);
    }

    #[test]
    fn test_auth_status_readonly() {
        let args = vec!["auth".to_string(), "status".to_string()];
        assert_eq!(classify_gh(&args), CommandClass::ReadOnly);
    }

    #[test]
    fn test_auth_login_mutating() {
        let args = vec!["auth".to_string(), "login".to_string()];
        assert_eq!(classify_gh(&args), CommandClass::Mutating);
    }

    #[test]
    fn test_pr_checks_readonly() {
        let args = vec!["pr".to_string(), "checks".to_string(), "20".to_string()];
        assert_eq!(classify_gh(&args), CommandClass::ReadOnly);
    }

    #[test]
    fn test_pr_diff_readonly() {
        let args = vec!["pr".to_string(), "diff".to_string(), "20".to_string()];
        assert_eq!(classify_gh(&args), CommandClass::ReadOnly);
    }

    #[test]
    fn test_validate_rejects_semicolon() {
        assert!(validate_gh_command("gh pr list; rm -rf /").is_err());
    }

    #[test]
    fn test_validate_rejects_ampersand() {
        assert!(validate_gh_command("gh pr list && echo pwned").is_err());
    }

    #[test]
    fn test_validate_rejects_backtick() {
        assert!(validate_gh_command("gh pr list `whoami`").is_err());
    }

    #[test]
    fn test_validate_rejects_dollar_paren() {
        assert!(validate_gh_command("gh pr list $(whoami)").is_err());
    }

    #[test]
    fn test_validate_rejects_redirect() {
        assert!(validate_gh_command("gh pr list > output.txt").is_err());
    }

    #[test]
    fn test_validate_rejects_empty() {
        assert!(validate_gh_command("gh").is_err());
        assert!(validate_gh_command("gh ").is_err());
    }
}
