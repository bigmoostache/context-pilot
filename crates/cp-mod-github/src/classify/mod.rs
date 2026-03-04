//! Command classification for gh (GitHub CLI) commands.

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
                return Err(format!("Shell operator '{c}' is not allowed"));
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
///
/// # Errors
///
/// Returns `Err` if the command doesn't start with `gh`, contains shell operators,
/// or has unterminated quotes.
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

/// Groups where every subcommand is read-only — no per-action check needed.
const READ_ONLY_GROUPS: &[&str] = &["attestation", "browse", "completion", "help", "search", "status", "version"];

/// `(group, action)` pairs classified as read-only.
/// Sorted by group, then action — both for readability and `binary_search` lookups.
/// Unlisted pairs default to [`CommandClass::Mutating`] (safe fallback).
const READ_ONLY_ACTIONS: &[(&str, &str)] = &[
    ("alias", "list"),
    ("auth", "status"),
    ("auth", "token"),
    ("cache", "list"),
    ("codespace", "code"),
    ("codespace", "jupyter"),
    ("codespace", "list"),
    ("codespace", "logs"),
    ("codespace", "ports"),
    ("codespace", "ssh"),
    ("codespace", "view"),
    ("config", "get"),
    ("config", "list"),
    ("extension", "browse"),
    ("extension", "list"),
    ("extension", "search"),
    ("gist", "list"),
    ("gist", "view"),
    ("gpg-key", "list"),
    ("issue", "list"),
    ("issue", "status"),
    ("issue", "view"),
    ("label", "list"),
    ("org", "list"),
    ("pr", "checks"),
    ("pr", "diff"),
    ("pr", "list"),
    ("pr", "status"),
    ("pr", "view"),
    ("project", "field-list"),
    ("project", "item-list"),
    ("project", "list"),
    ("project", "view"),
    ("release", "download"),
    ("release", "list"),
    ("release", "view"),
    ("repo", "list"),
    ("repo", "view"),
    ("ruleset", "check"),
    ("ruleset", "list"),
    ("ruleset", "view"),
    ("run", "download"),
    ("run", "list"),
    ("run", "view"),
    ("run", "watch"),
    ("secret", "list"),
    ("ssh-key", "list"),
    ("variable", "get"),
    ("variable", "list"),
    ("workflow", "list"),
    ("workflow", "view"),
];

/// Classify a gh command (given as parsed args after "gh") as read-only or mutating.
///
/// Uses static lookup tables ([`READ_ONLY_GROUPS`] and [`READ_ONLY_ACTIONS`]) so
/// adding new subcommands is a one-line table entry. Unknown commands default to
/// [`CommandClass::Mutating`] as a safe fallback. The `api` subcommand gets special
/// handling since its classification depends on the `--method`/`-X` flag.
#[must_use]
pub fn classify_gh(args: &[String]) -> CommandClass {
    if args.is_empty() {
        return CommandClass::Mutating;
    }

    let group = args[0].as_str();

    // Entire group is read-only?
    if READ_ONLY_GROUPS.contains(&group) {
        return CommandClass::ReadOnly;
    }

    // `gh api` — read-only unless an explicit mutating HTTP method is passed
    if group == "api" {
        let rest: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
        let has_mutating_method = rest.windows(2).any(|w| {
            (w[0] == "--method" || w[0] == "-X")
                && matches!(w[1].to_uppercase().as_str(), "POST" | "PUT" | "PATCH" | "DELETE")
        });
        return if has_mutating_method { CommandClass::Mutating } else { CommandClass::ReadOnly };
    }

    // Per-action lookup (binary search on sorted table)
    let action = args.get(1).map_or("", |s| s.as_str());
    if READ_ONLY_ACTIONS.binary_search_by(|&(g, a)| g.cmp(group).then_with(|| a.cmp(action))).is_ok() {
        return CommandClass::ReadOnly;
    }

    // Safe default: anything we don't explicitly know is read-only gets treated as mutating
    CommandClass::Mutating
}
