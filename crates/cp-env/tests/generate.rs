//! `docs/ENV.md` generator (integration test, `--ignored`).
//!
//! Run: `cargo test -p cp-env --test generate -- --ignored`
//! Writes `docs/ENV.md` at the workspace root; `tests/consistency.rs` fails
//! whenever the committed file is behind the table.

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use tempfile as _;

    /// The workspace root (two levels above this crate).
    fn workspace_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    /// Rewrite `docs/ENV.md` from the spec table.
    #[test]
    #[ignore = "reference generator; run explicitly with --ignored to (re)write docs/ENV.md"]
    fn generate_env_md() {
        let path = workspace_root().join("docs/ENV.md");
        std::fs::write(&path, cp_env::render::env_md()).unwrap();
    }
}
