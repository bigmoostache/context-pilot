//! Command classification for gh (GitHub CLI) commands.

/// Which panels a mutating command makes stale.
pub(crate) mod invalidation;
#[cfg(test)]
mod tests;

/// Whether a `gh` subcommand reads or mutates state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandClass {
    /// Safe to re-run in a refreshing panel (e.g., `gh pr list`).
    ReadOnly,
    /// Must run exactly once: changes remote or local state, opens a browser,
    /// or blocks (e.g., `gh pr create`, `gh run download`, `gh pr view --web`).
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

/// Reject shell metacharacters outside quotes. `gh` runs without a shell, so
/// they would reach it as literal arguments; refusing them keeps the tool's
/// contract explicit instead of silently passing `|` to gh.
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
/// `browse` is absent on purpose: it opens a browser on every run.
const READ_ONLY_GROUPS: &[&str] = &["attestation", "completion", "help", "search", "status", "version"];

/// Flags that make any command run-once: `--web` opens a browser, `--watch`
/// blocks until the run ends.
const RUN_ONCE_FLAGS: &[&str] = &["--web", "--watch"];

/// `(group, action)` pairs classified as read-only.
/// Sorted by group, then action — both for readability and `binary_search` lookups.
/// Unlisted pairs default to [`CommandClass::Mutating`] (safe fallback).
/// Downloads (they write files), `run watch` and interactive codespace
/// actions (`ssh`, `code`, `jupyter`) are deliberately absent.
const READ_ONLY_ACTIONS: &[(&str, &str)] = &[
    ("alias", "list"),
    ("auth", "status"),
    ("auth", "token"),
    ("cache", "list"),
    ("codespace", "list"),
    ("codespace", "logs"),
    ("codespace", "ports"),
    ("codespace", "view"),
    ("config", "get"),
    ("config", "list"),
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
    ("release", "list"),
    ("release", "view"),
    ("repo", "list"),
    ("repo", "view"),
    ("ruleset", "check"),
    ("ruleset", "list"),
    ("ruleset", "view"),
    ("run", "list"),
    ("run", "view"),
    ("secret", "list"),
    ("ssh-key", "list"),
    ("variable", "get"),
    ("variable", "list"),
    ("workflow", "list"),
    ("workflow", "view"),
];

/// Request-body flags of `gh api`: any of them switches the default method to POST.
const API_BODY_FLAGS: &[&str] = &["-f", "-F", "--field", "--raw-field", "--input"];

/// Explicit method carried by one `gh api` argument (`-XPOST`, `--method=PUT`);
/// `None` for the bare `-X`/`--method` forms, whose value is the next argument.
fn inline_method(arg: &str) -> Option<String> {
    arg.strip_prefix("--method=")
        .or_else(|| arg.strip_prefix("-X").map(|m| m.trim_start_matches('=')))
        .filter(|m| !m.is_empty())
        .map(str::to_uppercase)
}

/// Whether `arg` passes request-body fields (`-f k=v`, `-fk=v`, `--field=k=v`, …).
fn is_body_flag(arg: &str) -> bool {
    API_BODY_FLAGS.iter().any(|flag| {
        arg == *flag
            || arg.strip_prefix(flag).is_some_and(|rest| rest.starts_with('=') || (flag.len() == 2 && !rest.is_empty()))
    })
}

/// HTTP method `gh api` will use: an explicit `-X`/`--method` wins; otherwise
/// body fields imply POST (gh's own rule); otherwise GET.
pub(crate) fn api_method(rest: &[String]) -> String {
    let mut explicit = None;
    let mut has_body = false;
    let mut iter = rest.iter().map(String::as_str);
    while let Some(arg) = iter.next() {
        if arg == "-X" || arg == "--method" {
            explicit = iter.next().map(str::to_uppercase);
        } else if let Some(method) = inline_method(arg) {
            explicit = Some(method);
        } else if is_body_flag(arg) {
            has_body = true;
        } else {
            // Positional endpoint or an unrelated flag.
        }
    }
    explicit.unwrap_or_else(|| if has_body { "POST" } else { "GET" }.to_owned())
}

/// Classify a gh command (given as parsed args after "gh") as read-only or mutating.
///
/// Uses static lookup tables ([`READ_ONLY_GROUPS`] and [`READ_ONLY_ACTIONS`]) so
/// adding new subcommands is a one-line table entry. Unknown commands default to
/// [`CommandClass::Mutating`] as a safe fallback. `gh api` is read-only only
/// when its effective method ([`api_method`]) is GET or HEAD.
#[must_use]
pub fn classify(args: &[String]) -> CommandClass {
    let Some(group_s) = args.first() else {
        return CommandClass::Mutating;
    };
    let group = group_s.as_str();

    if args.iter().any(|a| RUN_ONCE_FLAGS.contains(&a.as_str())) {
        return CommandClass::Mutating;
    }

    if READ_ONLY_GROUPS.contains(&group) {
        return CommandClass::ReadOnly;
    }

    if group == "api" {
        let method = api_method(args.get(1..).unwrap_or_default());
        return if method == "GET" || method == "HEAD" { CommandClass::ReadOnly } else { CommandClass::Mutating };
    }

    // Per-action lookup (binary search on sorted table)
    let action = args.get(1).map_or("", |s| s.as_str());
    if READ_ONLY_ACTIONS.binary_search_by(|&(g, a)| g.cmp(group).then_with(|| a.cmp(action))).is_ok() {
        return CommandClass::ReadOnly;
    }

    // Safe default: anything not explicitly known to be read-only is mutating.
    CommandClass::Mutating
}
