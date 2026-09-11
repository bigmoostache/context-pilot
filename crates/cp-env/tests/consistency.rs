//! Drift guards between the spec table and everything derived from it by hand.
//!
//! * every `CP_*` name a deployment surface mentions must be in the table;
//! * `docs/ENV.md` must be the current rendering of the table.

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    use tempfile as _;

    /// The deployment files whose `CP_*` names must all be known.
    const SURFACES: [&str; 6] = [
        "deploy/ansible/templates/context-pilot.service.j2",
        "deploy/ansible/templates/seed.env.j2",
        "deploy/docker/Dockerfile",
        "deploy/docker/docker-compose.yml",
        "deploy/docker/.env.example",
        "deploy/docker/providers.env.example",
    ];

    /// The workspace root (two levels above this crate).
    fn workspace_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    /// Every maximal `CP_[A-Z0-9_]*` run in `text`, minus glob-like stubs
    /// (`CP_SEED_*` leaves a trailing underscore) which name families, not
    /// variables.
    fn cp_names(text: &str) -> BTreeSet<String> {
        let mut names = BTreeSet::new();
        let mut current = String::new();
        for ch in text.chars().chain(std::iter::once(' ')) {
            if ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_' {
                current.push(ch);
                continue;
            }
            if current.starts_with("CP_") && !current.ends_with('_') {
                let _new = names.insert(current.clone());
            }
            current.clear();
        }
        names
    }

    /// Read a workspace file.
    fn read(relative: &str) -> String {
        let path = workspace_root().join(relative);
        std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()))
    }

    /// A `CP_*` name in a deployment file that the table does not know is
    /// either a typo or an undeclared variable - both are what strict boot
    /// would reject on the box, so catch them here first.
    #[test]
    fn deployment_surfaces_only_use_known_names() {
        let mut unknown: Vec<String> = Vec::new();
        for surface in SURFACES {
            for name in cp_names(&read(surface)) {
                if cp_env::specs::find(&name).is_none() {
                    unknown.push(format!("{surface}: {name}"));
                }
            }
        }
        assert!(unknown.is_empty(), "names missing from crates/cp-env/src/specs/:\n{}", unknown.join("\n"));
    }

    /// The committed reference must match the table.
    #[test]
    fn env_md_is_up_to_date() {
        let committed = read("docs/ENV.md");
        assert!(
            committed == cp_env::render::env_md(),
            "docs/ENV.md is stale - run: cargo test -p cp-env --test generate -- --ignored"
        );
    }

    /// The generated marker is the first thing after the title, so a reader
    /// (or a grep) knows not to edit the file by hand.
    #[test]
    fn env_md_carries_the_generated_marker() {
        let rendered = cp_env::render::env_md();
        assert!(rendered.lines().nth(2) == Some(cp_env::render::GENERATED_MARKER), "{rendered}");
        assert!(Path::new(&workspace_root().join("docs/ENV.md")).is_file());
    }
}
