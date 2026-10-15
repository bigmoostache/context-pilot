//! Regression tests for classification and panel invalidation.

use super::invalidation::affects;
use super::{CommandClass, classify, validate_gh_command};

/// Classify a full `gh …` command line.
fn class_of(command: &str) -> CommandClass {
    classify(&validate_gh_command(command).expect("valid gh command"))
}

/// Args after `gh` of a full command line.
fn args(command: &str) -> Vec<String> {
    validate_gh_command(command).expect("valid gh command")
}

#[test]
fn api_body_fields_imply_post() {
    for cmd in [
        "gh api repos/o/r/issues -f title=x",
        "gh api repos/o/r/issues -ftitle=x",
        "gh api repos/o/r/issues -F n=1",
        "gh api repos/o/r/issues --field n=1",
        "gh api repos/o/r/issues --raw-field=title=x",
        "gh api repos/o/r/issues --input body.json",
        "gh api graphql -f query='mutation { x }'",
    ] {
        assert_eq!(class_of(cmd), CommandClass::Mutating, "{cmd}");
    }
}

#[test]
fn api_explicit_method_spellings() {
    for cmd in [
        "gh api repos/o/r -X PATCH",
        "gh api repos/o/r -XDELETE",
        "gh api repos/o/r --method post",
        "gh api repos/o/r --method=PUT",
    ] {
        assert_eq!(class_of(cmd), CommandClass::Mutating, "{cmd}");
    }
    assert_eq!(class_of("gh api repos/o/r/pulls"), CommandClass::ReadOnly);
    assert_eq!(class_of("gh api repos/o/r/issues -X GET -f state=open"), CommandClass::ReadOnly);
}

#[test]
fn run_once_commands_are_never_read_only() {
    for cmd in [
        "gh release download v1",
        "gh run download 42",
        "gh run watch 42",
        "gh codespace ssh",
        "gh browse",
        "gh pr view 12 --web",
        "gh run view 42 --watch",
    ] {
        assert_eq!(class_of(cmd), CommandClass::Mutating, "{cmd}");
    }
    assert_eq!(class_of("gh pr list"), CommandClass::ReadOnly);
}

#[test]
fn merge_with_flags_first_invalidates_its_view() {
    let m = args("gh pr merge --squash 12");
    assert!(affects(&m, "gh pr view 12"));
    assert!(affects(&m, "gh pr list --state open"));
    assert!(!affects(&m, "gh pr view 13"));
}

#[test]
fn merge_without_number_invalidates_every_view() {
    let m = args("gh pr merge --squash");
    assert!(affects(&m, "gh pr view 12"));
    assert!(affects(&m, "gh pr status"));
}

#[test]
fn numbers_match_across_spellings() {
    let m = args("gh pr comment https://github.com/o/r/pull/7 --body hi");
    assert!(affects(&m, "gh pr view '#7'"));
    assert!(!affects(&m, "gh pr view 8"));
}

#[test]
fn api_method_long_flag_invalidates_api_panels() {
    let m = args("gh api repos/o/r/issues/1 --method PATCH -f state=closed");
    assert!(affects(&m, "gh api repos/o/r/issues"));
    assert!(affects(&args("gh issue close 1"), "gh api repos/o/r/issues"));
}

#[test]
fn cross_group_and_run_once_panels() {
    assert!(affects(&args("gh pr merge 3"), "gh issue list"));
    assert!(affects(&args("gh label delete bug"), "gh issue list"));
    assert!(!affects(&args("gh label delete bug"), "gh release list"));
    // A panel holding a mutation's output must never be re-run.
    assert!(!affects(&args("gh pr close 3"), "gh pr create --fill"));
    assert!(!affects(&args("gh pr close 3"), "gh api repos/o/r/pulls -f title=x"));
}
