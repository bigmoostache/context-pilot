//! Command classification for gh (GitHub CLI) commands.

#[cfg(test)]
mod tests;

/// Whether a `gh` subcommand reads or mutates state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandClass {
    /// Safe to auto-refresh in a panel (e.g., `gh pr list`).
    ReadOnly,
    /// Modifies remote state — execute once and return output (e.g., `gh pr create`).
    Mutating,
}

/// Parse a command string into arguments, respecting single and double quotes.
fn parse_shell_args(command: &str) -> Result<Vec<String>, String> {
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
fn check_shell_operators(command: &str) -> Result<(), String> {
    let mut in_single = false;
    let mut in_double = false;
    let chars: Vec<char> = command.chars().collect();
    let len = chars.len();

    for i in 0..len {
        let c = chars[i];
        match c {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            _ if in_single || in_double => {}
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
    let action = args.get(1).map_or("", |s| s.as_str());
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
            _ => CommandClass::Mutating,
        },

        // Repo commands
        "repo" => match action {
            "view" | "list" => CommandClass::ReadOnly,
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
