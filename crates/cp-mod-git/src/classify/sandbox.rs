//! Argument sandbox for the `Git` tool.
//!
//! No shell is involved, so the real code-execution vectors are git's own:
//! config keys that name a program (`alias.x=!sh`, `core.sshCommand`,
//! `diff.external`, `*.textconv`, filters, pagers...), flags that take a
//! program (`--upload-pack`, `--exec`, `rebase -x`), and subcommands that run
//! arbitrary commands (`bisect run`, `submodule foreach`, `difftool`). The
//! workdir guard (`-C`, `--git-dir`, `git worktree add`) lives here too.

use super::{CommandClass, classify_git, subcommand_index};

/// Config sections whose every key can run a program or pull in other config.
const DANGEROUS_SECTIONS: &[&str] =
    &["alias", "filter", "pager", "include", "includeif", "credential", "gpg", "sequence"];

/// Config key names (last dotted segment) that hold a program, a hook dir, or
/// redirect the work tree.
const DANGEROUS_NAMES: &[&str] = &[
    "command",
    "cmd",
    "driver",
    "textconv",
    "program",
    "helper",
    "uploadpack",
    "receivepack",
    "hookspath",
    "sshcommand",
    "pager",
    "editor",
    "fsmonitor",
    "gitproxy",
    "askpass",
    "external",
    "difffilter",
    "packobjectshook",
    "worktree",
    "vcs",
    "proxycommand",
];

/// Long flags (prefix match, so `--flag=value` is caught too) that name a program.
const DANGEROUS_FLAG_PREFIXES: &[&str] =
    &["--upload-pack", "--receive-pack", "--exec", "--extcmd", "--open-files-in-pager", "--config-env"];

/// Subcommands that exist to launch other programs.
const DENIED_SUBCMDS: &[&str] = &["difftool", "mergetool", "filter-branch", "send-email", "instaweb", "daemon"];

/// `true` when setting config `key` could make git execute a program.
pub(crate) fn is_dangerous_config_key(pair: &str) -> bool {
    let key = pair.split('=').next().unwrap_or("").to_ascii_lowercase();
    let section = key.split('.').next().unwrap_or("");
    let name = key.rsplit('.').next().unwrap_or("");
    DANGEROUS_SECTIONS.contains(&section) || (key.contains('.') && DANGEROUS_NAMES.contains(&name))
}

/// Refuse arguments that would let a `Git` call execute arbitrary programs.
///
/// # Errors
///
/// Returns a human-readable reason naming the offending argument.
pub(crate) fn check(args: &[String]) -> Result<(), String> {
    let sub_idx = subcommand_index(args);
    let globals = args.get(..sub_idx.unwrap_or(args.len())).unwrap_or_default();
    check_config_pairs(globals, &["-c"])?;
    check_flag_prefixes(args)?;
    let Some(idx) = sub_idx else { return Ok(()) };
    let subcmd = args.get(idx).map_or("", String::as_str);
    let rest = args.get(idx.saturating_add(1)..).unwrap_or_default();
    check_subcommand(subcmd, rest)?;
    if subcmd == "config" && classify_git(args) == CommandClass::Mutating {
        check_config_write(rest)?;
    }
    Ok(())
}

/// Reject `<flag> key=value` / `<flag>=key=value` pairs that set a dangerous key.
fn check_config_pairs(args: &[String], flags: &[&str]) -> Result<(), String> {
    let mut it = args.iter().map(String::as_str);
    while let Some(arg) = it.next() {
        let value = if flags.contains(&arg) {
            it.next()
        } else {
            flags.iter().find_map(|f| arg.strip_prefix(f)?.strip_prefix('='))
        };
        if let Some(key) = value.filter(|v| is_dangerous_config_key(v)) {
            return Err(format!("config '{key}' is not allowed: it can make git run arbitrary programs"));
        }
    }
    Ok(())
}

/// Reject any flag that names a program to run.
fn check_flag_prefixes(args: &[String]) -> Result<(), String> {
    args.iter()
        .find(|a| DANGEROUS_FLAG_PREFIXES.iter().any(|p| a.starts_with(p)))
        .map_or(Ok(()), |arg| Err(format!("'{arg}' is not allowed: it makes git run another program")))
}

/// Per-subcommand rules: denied subcommands, program-running actions and short flags.
fn check_subcommand(subcmd: &str, rest: &[String]) -> Result<(), String> {
    if DENIED_SUBCMDS.contains(&subcmd) {
        return Err(format!("'git {subcmd}' is not allowed: it runs external programs"));
    }
    let first = rest.first().map_or("", String::as_str);
    if matches!((subcmd, first), ("bisect", "run") | ("submodule", "foreach")) {
        return Err(format!("'git {subcmd} {first}' is not allowed: it runs arbitrary commands"));
    }
    let short: &[&str] = match subcmd {
        "rebase" => &["-x"],
        "clone" | "archive" | "ls-remote" => &["-u"],
        "grep" => &["-O"],
        _ => &[],
    };
    if let Some(flag) = rest.iter().find(|a| short.iter().any(|s| a.starts_with(s))) {
        return Err(format!("'git {subcmd} {flag}' is not allowed: it makes git run another program"));
    }
    if subcmd == "clone" {
        check_config_pairs(rest, &["-c", "--config"])?;
    }
    Ok(())
}

/// `git config <key> <value>` (any write form): refuse dangerous keys.
fn check_config_write(rest: &[String]) -> Result<(), String> {
    rest.iter().find(|a| !a.starts_with('-') && is_dangerous_config_key(a)).map_or(Ok(()), |key| {
        Err(format!("writing config '{key}' is not allowed: it can make git run arbitrary programs"))
    })
}

/// Refuse `Git` calls that would bypass the agent's workdir.
///
/// `-C`, `--git-dir` and `--work-tree` (global options, before the subcommand)
/// point git at another tree; `worktree add|remove|move|prune` changes
/// worktrees behind the `Worktree` tool's back, leaving agents aimed at
/// missing folders.
///
/// # Errors
///
/// Returns a human-readable reason naming the offending argument.
pub(crate) fn check_worktree_safety(args: &[String]) -> Result<(), String> {
    let sub_idx = subcommand_index(args);
    let globals = args.get(..sub_idx.unwrap_or(args.len())).unwrap_or_default();
    if let Some(arg) = globals.iter().find(|a| *a == "-C" || a.starts_with("--git-dir") || a.starts_with("--work-tree"))
    {
        return Err(format!(
            "'{arg}' is not allowed: git already runs in this agent's workdir. Use the Worktree tool to switch trees."
        ));
    }
    let Some(idx) = sub_idx else { return Ok(()) };
    let subcmd = args.get(idx).map_or("", String::as_str);
    match (subcmd, args.get(idx.saturating_add(1)).map(String::as_str)) {
        ("worktree", Some(a @ ("add" | "remove" | "move" | "prune"))) => {
            Err(format!("'git worktree {a}' is not allowed here; use the Worktree tool instead."))
        }
        _ => Ok(()),
    }
}
