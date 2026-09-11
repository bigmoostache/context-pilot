//! Validation rules, one case each, against an in-memory environment.
//!
//! Nothing here touches the process environment: every case builds a
//! `BTreeMap` source, so the suite runs in parallel and needs no `set_var`.

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;

    use cp_env::model::Env;
    use cp_env::model::features::{Feature, Features};
    use cp_env::render;
    use cp_env::resolve::{Raw, Report, Strictness, resolve};
    use cp_env::spec::Target;
    use tempfile::TempDir;

    /// An in-memory environment with `HOME` pointing at `home` plus `extra`.
    fn source(home: &Path, extra: &[(&str, &str)]) -> BTreeMap<String, String> {
        let mut map: BTreeMap<String, String> =
            extra.iter().copied().map(|(key, value)| (key.to_owned(), value.to_owned())).collect();
        drop(map.insert("HOME".to_owned(), home.to_string_lossy().into_owned()));
        map
    }

    /// Strict orchestrator resolve of `extra` under a fresh temporary home.
    fn orchestrator(extra: &[(&str, &str)]) -> (TempDir, Result<Raw, Report>) {
        let dir = tempfile::tempdir().unwrap();
        let result = resolve(Target::Orchestrator, &source(dir.path(), extra), Strictness::Strict);
        (dir, result)
    }

    /// The rendered lines of a failed resolve.
    fn errors(result: Result<Raw, Report>) -> Vec<String> {
        result.expect_err("expected a report").errors().iter().map(ToString::to_string).collect()
    }

    /// A temporary file with `content`, made executable when `exec` is set.
    fn file(dir: &Path, name: &str, content: &str, exec: bool) -> String {
        let path = dir.join(name);
        std::fs::write(&path, content).unwrap();
        #[cfg(unix)]
        if exec {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        path.to_string_lossy().into_owned()
    }

    /// With only `HOME`, every literal default applies.
    #[test]
    fn defaults_resolve_with_only_home() {
        let dir = tempfile::tempdir().unwrap();
        let env = Env::from_map(&source(dir.path(), &[]), Target::Orchestrator).unwrap();
        let observed = (
            env.orch.port,
            env.orch.bind.as_str(),
            env.orch.scan_interval.as_millis(),
            env.auth.enabled,
            env.auth.session_ttl.as_secs(),
            env.orch.web_root.clone(),
            env.gateway.is_active(),
            env.seed.superadmin.is_some(),
        );
        assert_eq!(observed, (7878, "127.0.0.1", 2000, false, 2_592_000, None, false, false));
        assert_eq!(env.features, Features::default());
    }

    /// Derived defaults hang off `HOME` and the agents directory.
    #[test]
    fn derived_defaults_follow_home() {
        let dir = tempfile::tempdir().unwrap();
        let env = Env::from_map(&source(dir.path(), &[]), Target::Orchestrator).unwrap();
        let home = dir.path();
        let observed = (env.core.agents_dir, env.orch.provision_flag, env.auth.db_path, env.orch.agents_root);
        let expected = (
            home.join(".context-pilot/agents"),
            home.join(".context-pilot/agents/.provisioned"),
            home.join(".context-pilot/orchestrator/auth.db"),
            home.join("code"),
        );
        assert_eq!(observed, expected);
    }

    /// `HOME` has no fallback: an empty environment is rejected by name.
    #[test]
    fn home_is_required() {
        let empty = BTreeMap::new();
        let lines = errors(resolve(Target::Orchestrator, &empty, Strictness::Strict));
        assert_eq!(lines, vec!["HOME: required but unset".to_owned()]);
    }

    /// A malformed port names the variable, the value and the expectation.
    #[test]
    fn bad_port_is_reported_with_its_value() {
        let (_dir, result) = orchestrator(&[("CP_ORCH_PORT", "abc")]);
        assert_eq!(errors(result), vec!["CP_ORCH_PORT: expected an integer 1-65535, got \"abc\"".to_owned()]);
    }

    /// Booleans accept exactly four spellings.
    #[test]
    fn bad_bool_is_rejected_and_good_ones_normalised() {
        let (_dir, result) = orchestrator(&[("CP_AUTH_ENABLED", "yes")]);
        assert_eq!(
            errors(result),
            vec!["CP_AUTH_ENABLED: expected a boolean (0, 1, true or false), got \"yes\"".to_owned()]
        );
        let (_dir2, result2) = orchestrator(&[("CP_AUTH_ENABLED", "TRUE")]);
        assert_eq!(result2.unwrap().text("CP_AUTH_ENABLED"), Some("1"));
    }

    /// Strict mode rejects a `CP_*` name the table does not know; lenient
    /// mode (the in-process test fallback) ignores it.
    #[test]
    fn unknown_cp_name_is_strict_only() {
        let dir = tempfile::tempdir().unwrap();
        let src = source(dir.path(), &[("CP_FOO", "1")]);
        let lines = errors(resolve(Target::Orchestrator, &src, Strictness::Strict));
        assert_eq!(lines, vec!["CP_FOO: unknown variable (not in the CP_* table; see docs/ENV.md)".to_owned()]);
        drop(resolve(Target::Orchestrator, &src, Strictness::Lenient).unwrap());
    }

    /// Names consumed by child processes or tooling are known but unparsed.
    #[test]
    fn child_only_names_are_tolerated_and_ignored() {
        let (_dir, result) = orchestrator(&[("CP_PORT", "9000"), ("CP_CHANGED_FILES", "a\nb")]);
        let raw = result.unwrap();
        assert!(raw.text("CP_PORT").is_none());
        assert!(raw.text("CP_CHANGED_FILES").is_none());
    }

    /// Every problem is listed at once, with a count in the header.
    #[test]
    fn errors_are_aggregated() {
        let (_dir, result) = orchestrator(&[("CP_ORCH_PORT", "0"), ("CP_FOO", "x"), ("CP_LLM_GATEWAY_KEY", "k")]);
        let report = result.expect_err("expected a report");
        assert_eq!(report.errors().len(), 3);
        let text = report.to_string();
        assert!(text.starts_with("environment configuration is invalid (3 errors):\n  - CP_FOO:"), "{text}");
        assert!(text.contains("  - CP_ORCH_PORT: expected an integer 1-65535, got \"0\"\n"), "{text}");
        assert!(text.contains("  - CP_LLM_GATEWAY_KEY: requires CP_LLM_GATEWAY to be set\n"), "{text}");
        assert!(text.ends_with("see docs/ENV.md for the full reference"), "{text}");
    }

    /// A tool gate must point at an executable file, and the report says why.
    #[cfg(unix)]
    #[test]
    fn executable_gate_checks_the_file() {
        let (_dir, missing) = orchestrator(&[("CP_NMCLI_BIN", "/nope/nmcli")]);
        assert_eq!(
            errors(missing),
            vec!["CP_NMCLI_BIN: expected an executable file, got \"/nope/nmcli\" (not found)".to_owned()]
        );

        let dir = tempfile::tempdir().unwrap();
        let plain = file(dir.path(), "plain", "", false);
        let lines =
            errors(resolve(Target::Orchestrator, &source(dir.path(), &[("CP_NMCLI_BIN", &plain)]), Strictness::Strict));
        assert_eq!(lines, vec![format!("CP_NMCLI_BIN: expected an executable file, got \"{plain}\" (not executable)")]);

        let tool = file(dir.path(), "nmcli", "#!/bin/sh\n", true);
        let env = Env::from_map(&source(dir.path(), &[("CP_NMCLI_BIN", &tool)]), Target::Orchestrator).unwrap();
        assert_eq!(env.appliance.nmcli_bin.as_deref(), Some(Path::new(&tool)));
    }

    /// Directory and parent preconditions.
    #[test]
    fn directory_and_parent_preconditions() {
        let (_dir, result) = orchestrator(&[("CP_WEB_ROOT", "/nope/web"), ("CP_CADDYFILE", "/nope/dir/Caddyfile")]);
        assert_eq!(
            errors(result),
            vec![
                "CP_WEB_ROOT: expected an existing directory, got \"/nope/web\" (not found)".to_owned(),
                "CP_CADDYFILE: expected a path whose parent directory exists, got \"/nope/dir/Caddyfile\" (not found)"
                    .to_owned(),
            ]
        );
        let dir = tempfile::tempdir().unwrap();
        let caddyfile = dir.path().join("Caddyfile").to_string_lossy().into_owned();
        let env = Env::from_map(&source(dir.path(), &[("CP_CADDYFILE", &caddyfile)]), Target::Orchestrator).unwrap();
        assert_eq!(env.appliance.caddyfile.as_deref(), Some(Path::new(&caddyfile)));
    }

    /// The Caddy binary is useless without a Caddyfile to reload.
    #[cfg(unix)]
    #[test]
    fn caddy_bin_requires_caddyfile() {
        let dir = tempfile::tempdir().unwrap();
        let caddy = file(dir.path(), "caddy", "#!/bin/sh\n", true);
        let lines =
            errors(resolve(Target::Orchestrator, &source(dir.path(), &[("CP_CADDY_BIN", &caddy)]), Strictness::Strict));
        assert_eq!(lines, vec!["CP_CADDY_BIN: requires CP_CADDYFILE to be set".to_owned()]);
    }

    /// Gateway: key needs URL, URL is normalised, scheme is checked, empty is off.
    #[test]
    fn gateway_rules() {
        let (_d1, key_alone) = orchestrator(&[("CP_LLM_GATEWAY_KEY", "k")]);
        assert_eq!(errors(key_alone), vec!["CP_LLM_GATEWAY_KEY: requires CP_LLM_GATEWAY to be set".to_owned()]);

        let (_d2, bad_scheme) = orchestrator(&[("CP_LLM_GATEWAY", "ftp://gw")]);
        assert_eq!(errors(bad_scheme), vec!["CP_LLM_GATEWAY: expected an http(s) URL, got \"ftp://gw\"".to_owned()]);

        let dir = tempfile::tempdir().unwrap();
        let env = Env::from_map(
            &source(dir.path(), &[("CP_LLM_GATEWAY", "http://litellm:4000///"), ("CP_LLM_GATEWAY_KEY", "sk-1")]),
            Target::Orchestrator,
        )
        .unwrap();
        assert_eq!(env.gateway.url(), Some("http://litellm:4000"));
        assert_eq!(env.gateway.key(), Some("sk-1"));
        assert!(env.gateway.is_active());

        let off = Env::from_map(&source(dir.path(), &[("CP_LLM_GATEWAY", "")]), Target::Orchestrator).unwrap();
        assert!(!off.gateway.is_active());
    }

    /// An email without any password form is an error; seeding needs auth.
    #[test]
    fn seed_rules() {
        let (_d1, no_password) = orchestrator(&[("CP_AUTH_ENABLED", "1"), ("CP_SEED_ADMIN_EMAIL", "a@b.c")]);
        assert_eq!(
            errors(no_password),
            vec![
                "CP_SEED_ADMIN_EMAIL: requires CP_SEED_ADMIN_PASSWORD or CP_SEED_ADMIN_PASSWORD_FILE to be set"
                    .to_owned()
            ]
        );

        let (_d2, auth_off) =
            orchestrator(&[("CP_SEED_SUPERADMIN_EMAIL", "v@x.y"), ("CP_SEED_SUPERADMIN_PASSWORD", "pw")]);
        let lines = errors(auth_off);
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(lines.iter().all(|line| line.contains("requires CP_AUTH_ENABLED=1")), "{lines:?}");
    }

    /// A seeded account exposes its email and resolves its password, the
    /// file form winning over the inline one.
    #[test]
    fn seed_account_password_prefers_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let pw_file = file(dir.path(), "pw", "from-file\n", false);
        let env = Env::from_map(
            &source(
                dir.path(),
                &[
                    ("CP_AUTH_ENABLED", "1"),
                    ("CP_SEED_SUPERADMIN_EMAIL", " vendor@x.y "),
                    ("CP_SEED_SUPERADMIN_PASSWORD", "inline"),
                    ("CP_SEED_SUPERADMIN_PASSWORD_FILE", &pw_file),
                    ("CP_SEED_ADMIN_EMAIL", "admin@x.y"),
                    ("CP_SEED_ADMIN_PASSWORD", "paper"),
                ],
            ),
            Target::Orchestrator,
        )
        .unwrap();
        let vendor = env.seed.superadmin.as_ref().unwrap();
        let admin = env.seed.admin.as_ref().unwrap();
        let observed =
            (vendor.email(), vendor.name(), vendor.password().unwrap(), admin.name(), admin.password().unwrap());
        assert_eq!(observed, ("vendor@x.y", "superadmin", "from-file".to_owned(), "admin", "paper".to_owned()));
        let shown = format!("{env:?}");
        let leaked = shown.contains("inline") || shown.contains("paper") || !shown.contains("[redacted]");
        assert!(!leaked, "secret leaked into Debug: {shown}");
    }

    /// Feature flags are checked against what they need to work.
    #[test]
    fn feature_invariants() {
        let (_d1, it_alone) = orchestrator(&[("CP_FEATURE_IT_PANE", "1")]);
        assert_eq!(
            errors(it_alone),
            vec!["CP_FEATURE_IT_PANE: requires CP_CADDYFILE to be set when enabled".to_owned()]
        );

        let (_d2, day0_alone) = orchestrator(&[("CP_FEATURE_DAY0_SETUP", "1")]);
        assert_eq!(
            errors(day0_alone),
            vec!["CP_FEATURE_DAY0_SETUP: requires CP_FEATURE_IT_PANE=1 when enabled".to_owned()]
        );

        let (_d3, updater_alone) = orchestrator(&[("CP_FEATURE_UPDATER", "1")]);
        assert_eq!(
            errors(updater_alone),
            vec!["CP_FEATURE_UPDATER: requires CP_WEB_ROOT to be set when enabled".to_owned()]
        );
    }

    /// A coherent appliance profile turns every gated flag on.
    #[test]
    fn feature_flags_resolve_together() {
        let dir = tempfile::tempdir().unwrap();
        let caddyfile = dir.path().join("Caddyfile").to_string_lossy().into_owned();
        let web = dir.path().to_string_lossy().into_owned();
        let env = Env::from_map(
            &source(
                dir.path(),
                &[
                    ("CP_CADDYFILE", &caddyfile),
                    ("CP_WEB_ROOT", &web),
                    ("CP_FEATURE_IT_PANE", "1"),
                    ("CP_FEATURE_DAY0_SETUP", "1"),
                    ("CP_FEATURE_UPDATER", "true"),
                    ("CP_FEATURE_CLAUDE_OAUTH", "0"),
                ],
            ),
            Target::Orchestrator,
        )
        .unwrap();
        let expected = Features::default()
            .with(Feature::ItPane, true)
            .with(Feature::Day0Setup, true)
            .with(Feature::Updater, true)
            .with(Feature::ClaudeOauth, false);
        assert_eq!(env.features, expected);
        assert_eq!(env.features.entries().first().copied(), Some((Feature::ClaudeOauth, false)));
    }

    /// Each binary validates its own scope; the other binary's names are
    /// known but ignored, so the orchestrator's full environment can be
    /// handed to an agent untouched.
    #[test]
    fn scope_filters_what_each_binary_parses() {
        let dir = tempfile::tempdir().unwrap();
        let src = source(dir.path(), &[("CP_ORCH_PORT", "abc"), ("CP_BRIDGE", "1")]);
        drop(resolve(Target::Agent, &src, Strictness::Strict).unwrap());
        let lines = errors(resolve(Target::Orchestrator, &src, Strictness::Strict));
        assert_eq!(lines, vec!["CP_ORCH_PORT: expected an integer 1-65535, got \"abc\"".to_owned()]);
    }

    /// `CP_BRIDGE` is a real boolean now: `0` is off, `yes` is an error.
    #[test]
    fn bridge_flag_is_a_boolean() {
        let dir = tempfile::tempdir().unwrap();
        let on = Env::from_map(&source(dir.path(), &[("CP_BRIDGE", "1")]), Target::Agent).unwrap();
        let off = Env::from_map(&source(dir.path(), &[("CP_BRIDGE", "0")]), Target::Agent).unwrap();
        assert_eq!(
            (on.bridge.enabled, on.bridge.url.as_str(), off.bridge.enabled),
            (true, "http://127.0.0.1:7878", false)
        );
        let bad = errors(resolve(Target::Agent, &source(dir.path(), &[("CP_BRIDGE", "yes")]), Strictness::Strict));
        assert_eq!(bad, vec!["CP_BRIDGE: expected a boolean (0, 1, true or false), got \"yes\"".to_owned()]);
    }

    /// Credentials are the vault's business: never parsed, never rejected.
    #[test]
    fn secrets_are_never_read() {
        let (_dir, result) = orchestrator(&[("ANTHROPIC_API_KEY", "sk-ant"), ("GITHUB_TOKEN", "")]);
        let raw = result.unwrap();
        assert!(raw.text("ANTHROPIC_API_KEY").is_none());
        assert!(raw.text("GITHUB_TOKEN").is_none());
    }

    /// The check report shows origins, hides secrets, reports presence.
    #[test]
    fn check_report_redacts_and_marks_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let src = source(
            dir.path(),
            &[
                ("CP_AUTH_ENABLED", "1"),
                ("CP_SEED_SUPERADMIN_EMAIL", "v@x.y"),
                ("CP_SEED_SUPERADMIN_PASSWORD", "hunter2"),
                ("ANTHROPIC_API_KEY", "sk-ant"),
            ],
        );
        let raw = resolve(Target::Orchestrator, &src, Strictness::Strict).unwrap();
        let text = render::check_report(&raw, &src);
        let expected = [
            "environment OK (target: orchestrator)",
            "  CP_AUTH_ENABLED = 1\n",
            "  CP_ORCH_PORT = 7878 (default)\n",
            "  CP_SEED_SUPERADMIN_PASSWORD = [redacted]\n",
            "  ANTHROPIC_API_KEY = set (secret, resolved by the vault)\n",
            "  BRAVE_API_KEY = unset (secret, resolved by the vault)\n",
        ];
        for needle in expected {
            assert!(text.contains(needle), "missing {needle:?} in:\n{text}");
        }
        assert!(!text.contains("hunter2"), "{text}");
    }

    /// An empty or blank value counts as unset: the default applies.
    #[test]
    fn empty_value_means_unset() {
        let (_dir, result) = orchestrator(&[("CP_ORCH_PORT", ""), ("CP_ORCH_BIND", "   ")]);
        let raw = result.unwrap();
        assert_eq!(raw.text("CP_ORCH_PORT"), Some("7878"));
        assert!(!raw.is_explicit("CP_ORCH_PORT"));
        assert_eq!(raw.text("CP_ORCH_BIND"), Some("127.0.0.1"));
    }

    /// Integer lower bounds are enforced with the bound in the message.
    #[test]
    fn integer_lower_bounds() {
        let (_dir, result) = orchestrator(&[("CP_SESSION_TTL_SECS", "5"), ("CP_SCAN_INTERVAL_MS", "0")]);
        assert_eq!(
            errors(result),
            vec![
                "CP_SCAN_INTERVAL_MS: expected an integer of at least 1, got \"0\"".to_owned(),
                "CP_SESSION_TTL_SECS: expected an integer of at least 60, got \"5\"".to_owned(),
            ]
        );
    }
}
