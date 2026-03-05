/// Command classification for git commands.
/// Determines whether a git command is read-only (safe to cache/auto-refresh)
/// or mutating (must execute and return output).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandClass {
    /// Command only reads repository state (safe to cache and auto-refresh).
    ReadOnly,
    /// Command modifies repository state (execute once, return output).
    Mutating,
}

/// Parse a command string into arguments, respecting single and double quotes.
/// Strips the leading command name (e.g. "git") and returns all tokens.
pub(crate) fn parse_shell_args(command: &str) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;

    for c in command.chars() {
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
pub(crate) fn check_shell_operators(command: &str) -> Result<(), String> {
    let mut in_single = false;
    let mut in_double = false;
    let chars: Vec<char> = command.chars().collect();

    for (i, &c) in chars.iter().enumerate() {
        match c {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            _ if in_single || in_double => {}
            '|' | ';' | '`' | '>' | '<' => {
                return Err(format!("Shell operator '{c}' is not allowed"));
            }
            '$' if chars.get(i.wrapping_add(1)) == Some(&'(') => {
                return Err("Shell operator '$(' is not allowed".to_string());
            }
            '&' if chars.get(i.wrapping_add(1)) == Some(&'&') => {
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
pub(crate) fn validate_git_command(command: &str) -> Result<Vec<String>, String> {
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
pub(crate) fn classify_git(args: &[String]) -> CommandClass {
    let Some(subcmd) = args.first().map(String::as_str) else {
        return CommandClass::Mutating; // safe default
    };

    let rest: Vec<&str> = args.get(1..).unwrap_or_default().iter().map(String::as_str).collect();

    match subcmd {
        // Always read-only
        "log" | "diff" | "show" | "status" | "blame" | "rev-parse" | "rev-list" | "ls-tree" | "ls-files"
        | "ls-remote" | "cat-file" | "for-each-ref" | "describe" | "shortlog" | "count-objects" | "fsck"
        | "check-ignore" | "check-attr" | "name-rev" | "grep" | "reflog" | "archive" | "format-patch" => {
            CommandClass::ReadOnly
        }

        // Context-dependent commands
        "branch" => {
            if rest.is_empty()
                || rest.iter().any(|a| {
                    matches!(*a, "-l" | "--list" | "-a" | "--all" | "-r" | "--remotes" | "-v" | "--verbose" | "-vv")
                })
            {
                CommandClass::ReadOnly
            } else {
                CommandClass::Mutating
            }
        }
        "stash" => match rest.first() {
            Some(&"list" | &"show") => CommandClass::ReadOnly,
            _ => CommandClass::Mutating,
        },
        "tag" => {
            if rest.is_empty() || rest.iter().any(|a| matches!(*a, "-l" | "--list")) {
                CommandClass::ReadOnly
            } else {
                CommandClass::Mutating
            }
        }
        "remote" => match rest.first() {
            None | Some(&"show" | &"get-url") => CommandClass::ReadOnly,
            _ if rest.iter().any(|a| matches!(*a, "-v" | "--verbose")) && rest.len() == 1 => CommandClass::ReadOnly,
            _ => CommandClass::Mutating,
        },
        "config" => {
            if rest.iter().any(|a| matches!(*a, "--get" | "--get-all" | "--list" | "-l" | "--get-regexp")) {
                CommandClass::ReadOnly
            } else {
                CommandClass::Mutating
            }
        }
        "notes" => match rest.first() {
            None | Some(&"show" | &"list") => CommandClass::ReadOnly,
            _ => CommandClass::Mutating,
        },
        "worktree" => match rest.first() {
            None | Some(&"list") => CommandClass::ReadOnly,
            _ => CommandClass::Mutating,
        },
        "submodule" => match rest.first() {
            None | Some(&"status" | &"summary") => CommandClass::ReadOnly,
            _ => CommandClass::Mutating,
        },

        // Additional context-dependent commands
        "sparse-checkout" => match rest.first() {
            Some(&"list") => CommandClass::ReadOnly,
            _ => CommandClass::Mutating,
        },
        "lfs" => match rest.first() {
            Some(&"ls-files" | &"status" | &"env" | &"logs") => CommandClass::ReadOnly,
            _ => CommandClass::Mutating,
        },
        "bisect" => match rest.first() {
            Some(&"log" | &"visualize") => CommandClass::ReadOnly,
            _ => CommandClass::Mutating,
        },
        "bundle" => match rest.first() {
            Some(&"verify" | &"list-heads") => CommandClass::ReadOnly,
            _ => CommandClass::Mutating,
        },
        "apply" => {
            if rest.iter().any(|a| matches!(*a, "--stat" | "--check")) {
                CommandClass::ReadOnly
            } else {
                CommandClass::Mutating
            }
        }
        "symbolic-ref" => {
            if rest.len() <= 1 || rest.contains(&"--short") {
                CommandClass::ReadOnly
            } else {
                CommandClass::Mutating
            }
        }
        "hash-object" => {
            if rest.contains(&"-w") {
                CommandClass::Mutating
            } else {
                CommandClass::ReadOnly
            }
        }

        // Always mutating
        "commit" | "push" | "pull" | "fetch" | "merge" | "rebase" | "cherry-pick" | "revert" | "reset" | "checkout"
        | "switch" | "add" | "rm" | "mv" | "restore" | "clean" | "init" | "clone" | "am" | "gc" | "prune"
        | "repack" | "update-index" | "filter-branch" | "filter-repo" | "replace" | "maintenance" => {
            CommandClass::Mutating
        }

        // Unknown -> Mutating (safe default)
        _ => CommandClass::Mutating,
    }
}
