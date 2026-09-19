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
    const SURFACES: [&str; 9] = [
        "deploy/ansible/templates/context-pilot.service.j2",
        "deploy/ansible/templates/seed.env.j2",
        "deploy/docker/Dockerfile",
        "deploy/docker/docker-compose.yml",
        "deploy/docker/.env.example",
        "deploy/docker/providers.env.example",
        "deploy/env/on-premise-workstation.env.example",
        "deploy/env/on-premise-server.env.example",
        "deploy/env/embedded-photonicat.env.example",
    ];

    /// The complete per-profile templates: every variable present, every flag
    /// decided.
    const TEMPLATES: [&str; 3] = [
        "deploy/env/on-premise-workstation.env.example",
        "deploy/env/on-premise-server.env.example",
        "deploy/env/embedded-photonicat.env.example",
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

    /// A deployment profile spells out every flag: the binary's defaults are
    /// not a deployment decision (`docs/ENV.md`).
    #[test]
    fn deployment_profiles_set_every_flag() {
        let profiles =
            [("deploy/ansible/templates/context-pilot.service.j2", "Environment="), ("deploy/docker/Dockerfile", "")];
        for (surface, prefix) in profiles {
            let text = read(surface);
            for name in cp_env::render::profile_explicit_names() {
                let needle = format!("{prefix}{name}=");
                assert!(text.contains(&needle), "{surface} does not set {name}");
            }
        }
    }

    /// A profile template is COMPLETE: every variable the binaries read
    /// appears (set, or commented as `#NAME=`), and every feature flag is
    /// decided (set, not commented) - a profile leaves nothing to a default.
    #[test]
    fn profile_templates_are_complete() {
        let flags = cp_env::render::profile_explicit_names();
        for template in TEMPLATES {
            let text = read(template);
            let mentions = |name: &str| {
                text.lines().any(|line| {
                    line.trim_start_matches('#') == format!("{name}=")
                        || line.trim_start_matches('#').starts_with(&format!("{name}="))
                })
            };
            for spec in cp_env::specs::all() {
                if spec.scope == cp_env::spec::Scope::ChildOnly {
                    continue;
                }
                assert!(mentions(spec.name), "{template} does not mention {}", spec.name);
            }
            for name in &flags {
                let decided = text.lines().any(|line| line.starts_with(&format!("{name}=")));
                assert!(decided, "{template} leaves {name} undecided (commented out)");
            }
        }
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
