/// Command classification for git commands.
/// Determines whether a git command is read-only (safe to cache/auto-refresh)
/// or mutating (must execute and return output).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandClass {
    ReadOnly,
    Mutating,
}

/// Parse a command string into arguments, respecting single and double quotes.
/// Strips the leading command name (e.g. "git" or "gh") and returns the rest.
pub fn parse_shell_args(command: &str) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut chars = command.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\'' if !in_double => {
                in_single = !in_single;
            }
            '"' if !in_single => {
                in_double = !in_double;
            }
            c if c.is_whitespace() && !in_single && !in_double => {
                if !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                }
            }
            _ => {
                current.push(c);
            }
        }
    }

    if in_single {
        return Err("Unterminated single quote".to_string());
    }
    if in_double {
        return Err("Unterminated double quote".to_string());
    }
    if !current.is_empty() {
        args.push(current);
    }

    Ok(args)
}

/// Check for shell metacharacters outside of quoted strings.
pub fn check_shell_operators(command: &str) -> Result<(), String> {
    let mut in_single = false;
    let mut in_double = false;
    let chars: Vec<char> = command.chars().collect();
    let len = chars.len();

    for i in 0..len {
        let c = chars[i];
        match c {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            _ if in_single || in_double => {} // inside quotes, skip
            '|' | ';' | '`' | '>' | '<' => {
                return Err(format!("Shell operator '{}' is not allowed", c));
            }
            '$' if i + 1 < len && chars[i + 1] == '(' => {
                return Err("Shell operator '$(' is not allowed".to_string());
            }
            '&' if i + 1 < len && chars[i + 1] == '&' => {
                return Err("Shell operator '&&' is not allowed".to_string());
            }
            '\n' | '\r' => {
                return Err("Newlines are not allowed outside of quoted strings".to_string());
            }
            _ => {}
        }
    }
    Ok(())
}

/// Validate a raw command string intended for `git`.
/// Returns parsed args on success, or an error message on failure.
pub fn validate_git_command(command: &str) -> Result<Vec<String>, String> {
    let trimmed = command.trim();
    if !trimmed.starts_with("git ") && trimmed != "git" {
        return Err("Command must start with 'git '".to_string());
    }

    check_shell_operators(trimmed)?;

    // Parse into args, skip "git" prefix
    let all_args = parse_shell_args(trimmed)?;
    let args: Vec<String> = all_args.into_iter().skip(1).collect();

    if args.is_empty() {
        return Err("No git subcommand specified".to_string());
    }

    Ok(args)
}

/// Classify a git command (given as parsed args after "git") as read-only or mutating.
pub fn classify_git(args: &[String]) -> CommandClass {
    if args.is_empty() {
        return CommandClass::Mutating; // safe default
    }

    let subcmd = args[0].as_str();
    let rest: Vec<&str> = args[1..].iter().map(|s| s.as_str()).collect();

    match subcmd {
        // Always read-only
        "log" | "diff" | "show" | "status" | "blame" | "rev-parse" | "rev-list"
        | "ls-tree" | "ls-files" | "ls-remote" | "cat-file" | "for-each-ref"
        | "describe" | "shortlog" | "count-objects" | "fsck" | "check-ignore"
        | "check-attr" | "name-rev" | "grep" | "reflog" | "archive"
        | "format-patch" => CommandClass::ReadOnly,

        // Context-dependent commands
        "branch" => {
            // branch with no args or list flags → RO
            if rest.is_empty() || rest.iter().any(|a| matches!(*a, "-l" | "--list" | "-a" | "--all" | "-r" | "--remotes" | "-v" | "--verbose" | "-vv")) {
                CommandClass::ReadOnly
            } else {
                CommandClass::Mutating
            }
        }
        "stash" => {
            if rest.is_empty() {
                // bare "stash" = stash push = mutating
                CommandClass::Mutating
            } else {
                match rest[0] {
                    "list" | "show" => CommandClass::ReadOnly,
                    _ => CommandClass::Mutating,
                }
            }
        }
        "tag" => {
            // tag with no args or list flags → RO
            if rest.is_empty() || rest.iter().any(|a| matches!(*a, "-l" | "--list")) {
                CommandClass::ReadOnly
            } else {
                CommandClass::Mutating
            }
        }
        "remote" => {
            if rest.is_empty() {
                CommandClass::ReadOnly
            } else {
                match rest[0] {
                    "show" | "get-url" => CommandClass::ReadOnly,
                    _ if rest.iter().any(|a| matches!(*a, "-v" | "--verbose")) && rest.len() == 1 => {
                        CommandClass::ReadOnly
                    }
                    _ => CommandClass::Mutating,
                }
            }
        }
        "config" => {
            if rest.iter().any(|a| matches!(*a, "--get" | "--get-all" | "--list" | "-l" | "--get-regexp")) {
                CommandClass::ReadOnly
            } else {
                CommandClass::Mutating
            }
        }
        "notes" => {
            if rest.is_empty() {
                CommandClass::ReadOnly
            } else {
                match rest[0] {
                    "show" | "list" => CommandClass::ReadOnly,
                    _ => CommandClass::Mutating,
                }
            }
        }
        "worktree" => {
            if rest.is_empty() {
                CommandClass::ReadOnly
            } else {
                match rest[0] {
                    "list" => CommandClass::ReadOnly,
                    _ => CommandClass::Mutating,
                }
            }
        }
        "submodule" => {
            if rest.is_empty() {
                CommandClass::ReadOnly
            } else {
                match rest[0] {
                    "status" | "summary" => CommandClass::ReadOnly,
                    _ => CommandClass::Mutating,
                }
            }
        }

        // Additional context-dependent commands
        "sparse-checkout" => {
            match rest.first() {
                Some(&"list") => CommandClass::ReadOnly,
                _ => CommandClass::Mutating,
            }
        }
        "lfs" => {
            match rest.first() {
                Some(&"ls-files") | Some(&"status") | Some(&"env") | Some(&"logs") => CommandClass::ReadOnly,
                _ => CommandClass::Mutating,
            }
        }
        "bisect" => {
            match rest.first() {
                Some(&"log") | Some(&"visualize") => CommandClass::ReadOnly,
                _ => CommandClass::Mutating,
            }
        }
        "bundle" => {
            match rest.first() {
                Some(&"verify") | Some(&"list-heads") => CommandClass::ReadOnly,
                _ => CommandClass::Mutating,
            }
        }
        "apply" => {
            if rest.iter().any(|a| matches!(*a, "--stat" | "--check")) {
                CommandClass::ReadOnly
            } else {
                CommandClass::Mutating
            }
        }
        "symbolic-ref" => {
            // Query (no set action) = ReadOnly; with --short or single arg
            if rest.len() <= 1 || rest.iter().any(|a| *a == "--short") {
                CommandClass::ReadOnly
            } else {
                CommandClass::Mutating
            }
        }
        "hash-object" => {
            if rest.iter().any(|a| *a == "-w") {
                CommandClass::Mutating
            } else {
                CommandClass::ReadOnly
            }
        }

        // Always mutating
        "commit" | "push" | "pull" | "fetch" | "merge" | "rebase" | "cherry-pick"
        | "revert" | "reset" | "checkout" | "switch" | "add" | "rm" | "mv"
        | "restore" | "clean" | "init" | "clone" | "am" | "gc" | "prune"
        | "repack" | "update-index" | "filter-branch" | "filter-repo"
        | "replace" | "maintenance" => {
            CommandClass::Mutating
        }

        // Unknown → Mutating (safe default)
        _ => CommandClass::Mutating,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_rejects_non_git() {
        assert!(validate_git_command("ls -la").is_err());
    }

    #[test]
    fn test_validate_rejects_pipes() {
        assert!(validate_git_command("git log | head").is_err());
    }

    #[test]
    fn test_validate_accepts_valid() {
        let args = validate_git_command("git log --oneline -5").unwrap();
        assert_eq!(args, vec!["log", "--oneline", "-5"]);
    }

    #[test]
    fn test_classify_readonly() {
        let args = vec!["log".to_string(), "--oneline".to_string()];
        assert_eq!(classify_git(&args), CommandClass::ReadOnly);
    }

    #[test]
    fn test_classify_mutating() {
        let args = vec!["commit".to_string(), "-m".to_string(), "msg".to_string()];
        assert_eq!(classify_git(&args), CommandClass::Mutating);
    }

    #[test]
    fn test_branch_list_readonly() {
        let args = vec!["branch".to_string()];
        assert_eq!(classify_git(&args), CommandClass::ReadOnly);
        let args = vec!["branch".to_string(), "-a".to_string()];
        assert_eq!(classify_git(&args), CommandClass::ReadOnly);
    }

    #[test]
    fn test_branch_create_mutating() {
        let args = vec!["branch".to_string(), "new-branch".to_string()];
        assert_eq!(classify_git(&args), CommandClass::Mutating);
    }

    #[test]
    fn test_archive_readonly() {
        let args = vec!["archive".to_string(), "HEAD".to_string()];
        assert_eq!(classify_git(&args), CommandClass::ReadOnly);
    }

    #[test]
    fn test_sparse_checkout_list_readonly() {
        let args = vec!["sparse-checkout".to_string(), "list".to_string()];
        assert_eq!(classify_git(&args), CommandClass::ReadOnly);
    }

    #[test]
    fn test_sparse_checkout_set_mutating() {
        let args = vec!["sparse-checkout".to_string(), "set".to_string()];
        assert_eq!(classify_git(&args), CommandClass::Mutating);
    }

    #[test]
    fn test_apply_stat_readonly() {
        let args = vec!["apply".to_string(), "--stat".to_string(), "file.patch".to_string()];
        assert_eq!(classify_git(&args), CommandClass::ReadOnly);
    }

    #[test]
    fn test_apply_mutating() {
        let args = vec!["apply".to_string(), "file.patch".to_string()];
        assert_eq!(classify_git(&args), CommandClass::Mutating);
    }

    #[test]
    fn test_lfs_status_readonly() {
        let args = vec!["lfs".to_string(), "status".to_string()];
        assert_eq!(classify_git(&args), CommandClass::ReadOnly);
    }

    #[test]
    fn test_hash_object_readonly() {
        let args = vec!["hash-object".to_string(), "file.txt".to_string()];
        assert_eq!(classify_git(&args), CommandClass::ReadOnly);
    }

    #[test]
    fn test_hash_object_write_mutating() {
        let args = vec!["hash-object".to_string(), "-w".to_string(), "file.txt".to_string()];
        assert_eq!(classify_git(&args), CommandClass::Mutating);
    }

    #[test]
    fn test_validate_quoted_commit_message() {
        let args = validate_git_command("git commit -m \"hello world\"").unwrap();
        assert_eq!(args, vec!["commit", "-m", "hello world"]);
    }

    #[test]
    fn test_validate_single_quoted_args() {
        let args = validate_git_command("git log --format='%H %s'").unwrap();
        assert_eq!(args, vec!["log", "--format=%H %s"]);
    }

    #[test]
    fn test_validate_rejects_pipe_outside_quotes() {
        assert!(validate_git_command("git log | head").is_err());
    }

    #[test]
    fn test_validate_allows_pipe_inside_quotes() {
        let args = validate_git_command("git log --format=\"%H | %s\"").unwrap();
        assert_eq!(args, vec!["log", "--format=%H | %s"]);
    }

    #[test]
    fn test_validate_rejects_unterminated_quote() {
        assert!(validate_git_command("git commit -m \"unterminated").is_err());
    }
}
