use std::fs;
use std::path::PathBuf;

use super::frontmatter::{format, parse, split, validate};
use super::{migrate_one_file, next_suffixed_path};
use crate::types::PromptType;

/// Fresh empty temp dir unique to this test.
fn tmp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("cp-mod-prompt-{tag}-{}", std::process::id()));
    drop(fs::remove_dir_all(&d));
    fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn format_round_trips_colon_values() {
    let desc = "Exhaustive debug paths for infra: DB, Redis # and 'quotes'";
    let file = format("Debug: Paths", desc, "# Body\ntext").unwrap();
    validate(&file).unwrap();
    let (name, d, body) = parse(&file);
    assert_eq!(name, "Debug: Paths");
    assert_eq!(d, desc);
    assert_eq!(body, "# Body\ntext");
}

#[test]
fn format_round_trips_multiline_description() {
    let file = format("n", "line one\nline two", "b").unwrap();
    validate(&file).unwrap();
    assert_eq!(parse(&file).1, "line one\nline two");
}

#[test]
fn parse_rescues_unquoted_colon_frontmatter() {
    let file = "---\nname: find-out-yourself\ndescription: The user can't answer your last question: decide yourself\n---\nBody";
    let (name, desc, body) = parse(file);
    assert_eq!(name, "find-out-yourself");
    assert_eq!(desc, "The user can't answer your last question: decide yourself");
    assert_eq!(body, "Body");
}

#[test]
fn parse_without_frontmatter_is_all_body() {
    assert_eq!(parse("just text"), (String::new(), String::new(), "just text".to_owned()));
    assert_eq!(parse("---\nname: x\nno close").2, "---\nname: x\nno close");
}

#[test]
fn split_shared_by_parse_and_validate() {
    assert_eq!(split("---\nname: a\n---\n\nbody"), Some(("\nname: a", "body")));
    assert_eq!(split("no frontmatter"), None);
}

#[test]
fn validate_rejects_bad_files() {
    assert!(validate("body only").is_err());
    assert!(validate("---\nname: a\n").is_err());
    assert!(validate("---\ndescription: d\n---\nx").is_err());
    assert!(validate("---\nname: a\ndescription: b: c\n---\nx").is_err());
    validate("---\nname: a\n---\nx").unwrap();
}

#[test]
fn prompt_type_from_arg_keeps_agent_alias() {
    assert_eq!(PromptType::from_arg("system"), Ok(PromptType::System));
    assert_eq!(PromptType::from_arg("agent"), Ok(PromptType::System));
    assert_eq!(PromptType::from_arg("skill"), Ok(PromptType::Skill));
    assert_eq!(PromptType::from_arg("command"), Ok(PromptType::Command));
    let _err = PromptType::from_arg("persona").unwrap_err();
}

#[test]
fn migrate_moves_new_file() {
    let root = tmp_dir("mig-new");
    let (local, shared) = (root.join("local"), root.join("shared"));
    fs::create_dir_all(&local).unwrap();
    let src = local.join("a.md");
    fs::write(&src, "x").unwrap();
    migrate_one_file(&src, &shared, "a");
    assert!(!src.exists());
    assert_eq!(fs::read(shared.join("a.md")).unwrap(), b"x");
    drop(fs::remove_dir_all(root));
}

#[test]
fn migrate_drops_identical_duplicate() {
    let root = tmp_dir("mig-same");
    let (local, shared) = (root.join("local"), root.join("shared"));
    fs::create_dir_all(&local).unwrap();
    fs::create_dir_all(&shared).unwrap();
    let src = local.join("a.md");
    fs::write(&src, "x").unwrap();
    fs::write(shared.join("a.md"), "x").unwrap();
    migrate_one_file(&src, &shared, "a");
    assert!(!src.exists());
    assert!(!shared.join("a-1.md").exists());
    drop(fs::remove_dir_all(root));
}

#[test]
fn migrate_suffixes_differing_collision() {
    let root = tmp_dir("mig-diff");
    let (local, shared) = (root.join("local"), root.join("shared"));
    fs::create_dir_all(&local).unwrap();
    fs::create_dir_all(&shared).unwrap();
    let src = local.join("a.md");
    fs::write(&src, "local").unwrap();
    fs::write(shared.join("a.md"), "shared").unwrap();
    fs::write(shared.join("a-1.md"), "taken").unwrap();
    assert_eq!(next_suffixed_path(&shared, "a"), shared.join("a-2.md"));
    migrate_one_file(&src, &shared, "a");
    assert!(!src.exists());
    assert_eq!(fs::read(shared.join("a.md")).unwrap(), b"shared");
    assert_eq!(fs::read(shared.join("a-2.md")).unwrap(), b"local");
    drop(fs::remove_dir_all(root));
}
