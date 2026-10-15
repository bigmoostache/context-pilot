//! Classifier and sandbox regression tests.

use super::{CommandClass, check_worktree_safety, classify_git, validate_git_command};

/// `true` when the workdir guard refuses `cmd`.
fn rejected(cmd: &str) -> bool {
    check_worktree_safety(&validate_git_command(cmd).expect("valid command")).is_err()
}

/// Classification of a command that passes validation.
fn class(cmd: &str) -> CommandClass {
    classify_git(&validate_git_command(cmd).expect("valid command"))
}

#[test]
fn worktree_guard_refuses_escapes() {
    assert!(rejected("git -C /tmp status"));
    assert!(rejected("git --git-dir=/x/.git log"));
    assert!(rejected("git --work-tree /x status"));
}

#[test]
fn worktree_guard_refuses_reshaping() {
    assert!(rejected("git worktree add ../x"));
    assert!(rejected("git worktree remove x"));
    assert!(rejected("git -c color.ui=never worktree prune"));
}

#[test]
fn worktree_guard_allows_normal_use() {
    assert!(!rejected("git worktree list"));
    assert!(!rejected("git log -C --stat"));
    assert!(!rejected("git status"));
}

#[test]
fn sandbox_refuses_program_running_config() {
    for cmd in [
        "git -c alias.x='!sh -c id' x",
        "git -c core.sshCommand=evil fetch",
        "git -c diff.external=/bin/evil diff",
        "git -c diff.foo.textconv=evil log -p",
        "git -c filter.lfs.smudge=evil checkout .",
        "git -c core.hooksPath=/tmp/h commit -m x",
        "git -c CORE.FSMONITOR=evil status",
        "git -c core.pager=evil log",
        "git -c credential.helper=evil push",
        "git --config-env=core.editor=EVIL commit",
        "git config alias.x '!sh'",
        "git config --global core.sshCommand evil",
        "git clone -c core.hooksPath=/tmp/h https://x/y",
        "git clone --config=core.fsmonitor=evil https://x/y",
    ] {
        assert!(validate_git_command(cmd).is_err(), "should refuse: {cmd}");
    }
}

#[test]
fn sandbox_refuses_program_running_flags_and_subcommands() {
    for cmd in [
        "git fetch --upload-pack=evil origin",
        "git push --receive-pack evil origin",
        "git ls-remote -u evil origin",
        "git clone -u evil https://x/y",
        "git rebase -x 'make test' main",
        "git rebase --exec=evil main",
        "git grep -Oevil foo",
        "git difftool --extcmd=evil",
        "git bisect run ./evil.sh",
        "git submodule foreach evil",
        "git filter-branch --tree-filter evil",
    ] {
        assert!(validate_git_command(cmd).is_err(), "should refuse: {cmd}");
    }
}

#[test]
fn sandbox_allows_harmless_config() {
    for cmd in [
        "git -c color.ui=always diff",
        "git -c user.name=bot commit -m x",
        "git config --get alias.co",
        "git config user.email bot@x",
        "git grep -n foo",
    ] {
        assert!(validate_git_command(cmd).is_ok(), "should allow: {cmd}");
    }
}

/// Assert every `(command, expected class)` row.
fn assert_classes(rows: &[(&str, CommandClass)]) {
    for &(cmd, want) in rows {
        assert_eq!(class(cmd), want, "{cmd}");
    }
}

#[test]
fn global_options_do_not_hide_the_subcommand() {
    assert_classes(&[
        ("git --no-pager log", CommandClass::ReadOnly),
        ("git -c color.ui=always diff", CommandClass::ReadOnly),
        ("git -P show HEAD", CommandClass::ReadOnly),
        ("git --no-pager commit -m x", CommandClass::Mutating),
        ("git --version", CommandClass::Mutating),
    ]);
}

#[test]
fn file_writing_reads_are_mutating() {
    assert_classes(&[
        ("git reflog", CommandClass::ReadOnly),
        ("git reflog show main", CommandClass::ReadOnly),
        ("git reflog expire --all", CommandClass::Mutating),
        ("git reflog delete HEAD@{1}", CommandClass::Mutating),
        ("git format-patch -1", CommandClass::Mutating),
        ("git format-patch -1 --stdout", CommandClass::ReadOnly),
        ("git archive HEAD", CommandClass::ReadOnly),
        ("git archive -o x.zip HEAD", CommandClass::Mutating),
        ("git log --output=x.txt", CommandClass::Mutating),
        ("git diff --output x.patch", CommandClass::Mutating),
    ]);
}

#[test]
fn contextual_table() {
    assert_classes(&[
        ("git branch", CommandClass::ReadOnly),
        ("git branch -D x", CommandClass::Mutating),
        ("git stash list", CommandClass::ReadOnly),
        ("git stash", CommandClass::Mutating),
        ("git remote -v", CommandClass::ReadOnly),
        ("git remote add o url", CommandClass::Mutating),
        ("git hash-object -w f", CommandClass::Mutating),
        ("git frobnicate", CommandClass::Mutating),
    ]);
}

#[test]
fn only_read_only_panels_rerun() {
    use crate::result_panel::is_rerunnable;
    for (cmd, want) in [
        ("git log --oneline", true),
        ("git --no-pager diff", true),
        ("git pull", false),
        ("git rebase main", false),
        ("git fetch --all", false),
        ("git format-patch -3", false),
        ("git -c core.pager=evil log", false),
    ] {
        assert_eq!(is_rerunnable(cmd), want, "{cmd}");
    }
}

#[test]
fn untracked_line_count_is_size_capped() {
    let dir = std::env::temp_dir().join(format!("cp-mod-git-linecap-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let small = dir.join("small.txt");
    let big = dir.join("big.txt");
    std::fs::write(&small, "a\nb\nc\n").expect("write small");
    std::fs::write(&big, "x\n".repeat(600 * 1024)).expect("write big");
    assert_eq!(crate::worker::count_lines_capped(&small), 3);
    assert_eq!(crate::worker::count_lines_capped(&big), 0);
    drop(std::fs::remove_dir_all(&dir));
}
