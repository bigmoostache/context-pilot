//! Command classification for gh (GitHub CLI) commands.

/// Whether a `gh` subcommand reads or mutates state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CommandClass {
    /// Safe to auto-refresh in a panel (e.g., `gh pr list`).
    ReadOnly,
    /// Modifies remote state — execute once and return output (e.g., `gh pr create`).
    Mutating,
}

/// Accumulator state for the quote-aware shell-args lexer.
struct ShellLexer {
    /// Inside a single-quoted span (double quotes then lose their meaning).
    in_single: bool,
    /// Inside a double-quoted span (single quotes then lose their meaning).
    in_double: bool,
    /// Completed argument tokens.
    args: Vec<String>,
    /// Token currently being built.
    current: String,
}

impl ShellLexer {
    /// Fresh lexer with empty accumulators.
    const fn new() -> Self {
        Self { in_single: false, in_double: false, args: Vec::new(), current: String::new() }
    }

    /// Feed one character, updating quote state and flushing tokens on unquoted
    /// whitespace. Kept flat so [`parse_shell_args`] stays a plain iteration.
    fn feed(&mut self, c: char) {
        match c {
            '\'' if !self.in_double => self.in_single = !self.in_single,
            '"' if !self.in_single => self.in_double = !self.in_double,
            ws if ws.is_whitespace() && !self.in_single && !self.in_double => {
                if !self.current.is_empty() {
                    self.args.push(std::mem::take(&mut self.current));
                }
            }
            _ => self.current.push(c),
        }
    }

    /// Finalize: error on an unterminated quote, else flush the trailing token.
    fn finish(mut self) -> Result<Vec<String>, String> {
        if self.in_single {
            return Err("Unterminated single quote".to_owned());
        }
        if self.in_double {
            return Err("Unterminated double quote".to_owned());
        }
        if !self.current.is_empty() {
            self.args.push(self.current);
        }
        Ok(self.args)
    }
}

/// Parse a command string into arguments, respecting single and double quotes.
fn parse_shell_args(command: &str) -> Result<Vec<String>, String> {
    let mut lexer = ShellLexer::new();
    for c in command.chars() {
        lexer.feed(c);
    }
    lexer.finish()
}

/// Check for shell metacharacters outside of quoted strings.
fn check_shell_operators(command: &str) -> Result<(), String> {
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
            '$' if chars.get(i.saturating_add(1)) == Some(&'(') => {
                return Err("Shell operator '$(' is not allowed".to_owned());
            }
            '&' if chars.get(i.saturating_add(1)) == Some(&'&') => {
                return Err("Shell operator '&&' is not allowed".to_owned());
            }
            '\n' | '\r' => {
                return Err("Newlines are not allowed outside of quoted strings".to_owned());
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
        return Err("Command must start with 'gh '".to_owned());
    }

    check_shell_operators(trimmed)?;

    // Parse into args, skip "gh" prefix
    let all_args = parse_shell_args(trimmed)?;
    let args: Vec<String> = all_args.into_iter().skip(1).collect();

    if args.is_empty() {
        return Err("No gh subcommand specified".to_owned());
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
pub fn classify(args: &[String]) -> CommandClass {
    let Some(group_s) = args.first() else {
        return CommandClass::Mutating;
    };

    let group = group_s.as_str();

    // Entire group is read-only?
    if READ_ONLY_GROUPS.contains(&group) {
        return CommandClass::ReadOnly;
    }

    // `gh api` — read-only unless an explicit mutating HTTP method is passed
    if group == "api" {
        let rest: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
        let has_mutating_method = rest.windows(2).any(|w| {
            let (Some(flag), Some(method)) = (w.first(), w.get(1)) else {
                return false;
            };
            (*flag == "--method" || *flag == "-X")
                && matches!(method.to_uppercase().as_str(), "POST" | "PUT" | "PATCH" | "DELETE")
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
