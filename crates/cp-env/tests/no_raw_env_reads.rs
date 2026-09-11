//! The one-source-of-truth guard: nothing in the workspace reads the process
//! environment except the places that must, each listed here with its reason.
//!
//! A new `std::env::var` anywhere else is a variable that escaped the table -
//! undocumented, unvalidated, silently defaulted - which is exactly the state
//! this crate exists to end. Add the variable to `crates/cp-env/src/specs/`
//! and read it through `cp_env::env()` instead.

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use cp_env as _;
    use tempfile as _;

    /// Files allowed to read the environment directly, with why.
    const ALLOWED: [(&str, &str); 7] = [
        ("crates/cp-env/src/source.rs", "the process-environment Source: the single read point"),
        ("crates/cp-env/tests/no_raw_env_reads.rs", "this guard's own needle list"),
        ("crates/cp-vault/src/local.rs", "credential values belong to the vault, not the config layer"),
        ("crates/cp-orchestrator/src/main.rs", "HOME is needed to locate ~/.context-pilot/.env before validation"),
        ("src/state/persistence/boot.rs", "HOME is needed to locate ~/.context-pilot/.env before validation"),
        ("crates/cp-orchestrator/src/supervisor/proc.rs", "copies the whole validated environment onto spawned agents"),
        (
            "crates/cp-oplog/tests/crash_replay.rs",
            "test harness: child-process parameters (registered as external names)",
        ),
    ];

    /// The call shapes that read the environment (with or without the `std::`
    /// prefix - the bare forms are substrings of the qualified ones).
    const NEEDLES: [&str; 4] = ["env::var(", "env::var_os(", "env::vars(", "env::vars_os("];

    /// The workspace root (two levels above this crate).
    fn workspace_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    /// Every `.rs` file under `dir`, skipping build output.
    fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            if path.is_dir() {
                if name != "target" && name != "node_modules" {
                    rust_files(&path, out);
                }
                continue;
            }
            if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }

    /// Lines that read the environment, minus comments.
    fn raw_reads(text: &str) -> Vec<String> {
        text.lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .filter(|line| NEEDLES.iter().any(|needle| line.contains(needle)))
            .map(str::to_owned)
            .collect()
    }

    /// Every direct environment read outside the allow-list is a table escape.
    #[test]
    fn every_env_read_is_accounted_for() {
        let root = workspace_root().canonicalize().unwrap();
        let mut files = Vec::new();
        rust_files(&root.join("src"), &mut files);
        rust_files(&root.join("crates"), &mut files);
        let mut offenders = Vec::new();
        for file in files {
            let relative = file.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
            if ALLOWED.iter().any(|entry| entry.0 == relative) {
                continue;
            }
            let text = std::fs::read_to_string(&file).unwrap();
            for line in raw_reads(&text) {
                offenders.push(format!("{relative}: {}", line.trim()));
            }
        }
        assert!(
            offenders.is_empty(),
            "environment read outside cp-env - declare the variable in crates/cp-env/src/specs/ and read cp_env::env():\n{}",
            offenders.join("\n")
        );
    }

    /// The allow-list names real files: a stale entry would hide a regression.
    #[test]
    fn allow_list_entries_exist() {
        let root = workspace_root();
        for (allowed, _why) in ALLOWED {
            assert!(root.join(allowed).is_file(), "{allowed} is not a file");
        }
    }
}
