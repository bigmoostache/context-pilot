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
/// Strips the leading command name (e.g. "git") and returns all tokens.
pub(crate) fn parse_shell_args(command: &str) -> Result<Vec<String>, String> {
    let mut lexer = ShellLexer::new();
    for c in command.chars() {
        lexer.feed(c);
    }
    lexer.finish()
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
                return Err("Shell operator '$(' is not allowed".to_owned());
            }
            '&' if chars.get(i.wrapping_add(1)) == Some(&'&') => {
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

/// Validate a raw command string intended for `git`.
/// Returns parsed args on success, or an error message on failure.
pub(crate) fn validate_git_command(command: &str) -> Result<Vec<String>, String> {
    let trimmed = command.trim();
    if !trimmed.starts_with("git ") && trimmed != "git" {
        return Err("Command must start with 'git '".to_owned());
    }

    check_shell_operators(trimmed)?;

    // Parse into args, skip "git" prefix
    let all_args = parse_shell_args(trimmed)?;
    let args: Vec<String> = all_args.into_iter().skip(1).collect();

    if args.is_empty() {
        return Err("No git subcommand specified".to_owned());
    }

    Ok(args)
}

/// Git subcommands that only ever read repository state.
fn is_read_only_subcmd(subcmd: &str) -> bool {
    matches!(
        subcmd,
        "log"
            | "diff"
            | "show"
            | "status"
            | "blame"
            | "rev-parse"
            | "rev-list"
            | "ls-tree"
            | "ls-files"
            | "ls-remote"
            | "cat-file"
            | "for-each-ref"
            | "describe"
            | "shortlog"
            | "count-objects"
            | "fsck"
            | "check-ignore"
            | "check-attr"
            | "name-rev"
            | "grep"
            | "reflog"
            | "archive"
            | "format-patch"
    )
}

/// Git subcommands that always mutate repository state.
fn is_always_mutating_subcmd(subcmd: &str) -> bool {
    matches!(
        subcmd,
        "commit"
            | "push"
            | "pull"
            | "fetch"
            | "merge"
            | "rebase"
            | "cherry-pick"
            | "revert"
            | "reset"
            | "checkout"
            | "switch"
            | "add"
            | "rm"
            | "mv"
            | "restore"
            | "clean"
            | "init"
            | "clone"
            | "am"
            | "gc"
            | "prune"
            | "repack"
            | "update-index"
            | "filter-branch"
            | "filter-repo"
            | "replace"
            | "maintenance"
    )
}

/// Read-only iff the first sub-arg is a known read verb (or absent, when `empty_ok`).
fn classify_by_first_arg(rest: &[&str], read_verbs: &[&str], empty_ok: bool) -> CommandClass {
    match rest.first() {
        None if empty_ok => CommandClass::ReadOnly,
        Some(verb) if read_verbs.contains(verb) => CommandClass::ReadOnly,
        _ => CommandClass::Mutating,
    }
}

/// Read-only iff `rest` is empty or carries any of the read-only listing flags.
fn classify_empty_or_flag(rest: &[&str], read_flags: &[&str]) -> CommandClass {
    if rest.is_empty() || rest.iter().any(|a| read_flags.contains(a)) {
        CommandClass::ReadOnly
    } else {
        CommandClass::Mutating
    }
}

/// Read-only iff `rest` carries any of the read-only flags.
fn classify_any_flag(rest: &[&str], read_flags: &[&str]) -> CommandClass {
    if rest.iter().any(|a| read_flags.contains(a)) { CommandClass::ReadOnly } else { CommandClass::Mutating }
}

/// `git remote`: read-only for show/get-url, or a bare `-v`/`--verbose` listing.
fn classify_remote(rest: &[&str]) -> CommandClass {
    match rest.first() {
        None | Some(&"show" | &"get-url") => CommandClass::ReadOnly,
        _ if rest.iter().any(|a| matches!(*a, "-v" | "--verbose")) && rest.len() == 1 => CommandClass::ReadOnly,
        _ => CommandClass::Mutating,
    }
}

/// `git symbolic-ref`: read-only when querying (≤1 arg or `--short`).
fn classify_symbolic_ref(rest: &[&str]) -> CommandClass {
    if rest.len() <= 1 || rest.contains(&"--short") { CommandClass::ReadOnly } else { CommandClass::Mutating }
}

/// Classify the context-dependent subcommands (those whose read/write nature
/// depends on their arguments). Each arm delegates to a small predicate helper.
fn classify_contextual(subcmd: &str, rest: &[&str]) -> CommandClass {
    match subcmd {
        "branch" => {
            classify_empty_or_flag(rest, &["-l", "--list", "-a", "--all", "-r", "--remotes", "-v", "--verbose", "-vv"])
        }
        "stash" => classify_by_first_arg(rest, &["list", "show"], false),
        "tag" => classify_empty_or_flag(rest, &["-l", "--list"]),
        "remote" => classify_remote(rest),
        "config" => classify_any_flag(rest, &["--get", "--get-all", "--list", "-l", "--get-regexp"]),
        "notes" => classify_by_first_arg(rest, &["show", "list"], true),
        "worktree" => classify_by_first_arg(rest, &["list"], true),
        "submodule" => classify_by_first_arg(rest, &["status", "summary"], true),
        "sparse-checkout" => classify_by_first_arg(rest, &["list"], false),
        "lfs" => classify_by_first_arg(rest, &["ls-files", "status", "env", "logs"], false),
        "bisect" => classify_by_first_arg(rest, &["log", "visualize"], false),
        "bundle" => classify_by_first_arg(rest, &["verify", "list-heads"], false),
        "apply" => classify_any_flag(rest, &["--stat", "--check"]),
        "symbolic-ref" => classify_symbolic_ref(rest),
        "hash-object" => {
            if rest.contains(&"-w") {
                CommandClass::Mutating
            } else {
                CommandClass::ReadOnly
            }
        }
        // Unknown -> Mutating (safe default)
        _ => CommandClass::Mutating,
    }
}

/// Classify a git command (given as parsed args after "git") as read-only or mutating.
pub(crate) fn classify_git(args: &[String]) -> CommandClass {
    let Some(subcmd) = args.first().map(String::as_str) else {
        return CommandClass::Mutating; // safe default
    };

    if is_read_only_subcmd(subcmd) {
        return CommandClass::ReadOnly;
    }
    if is_always_mutating_subcmd(subcmd) {
        return CommandClass::Mutating;
    }

    let rest: Vec<&str> = args.get(1..).unwrap_or_default().iter().map(String::as_str).collect();
    classify_contextual(subcmd, &rest)
}
