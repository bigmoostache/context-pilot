//! Cross-variable rules - combinations that are individually valid but jointly
//! incoherent. Each one is an error, reported next to the per-variable ones.

use crate::resolve::{EnvError, Raw, Report};

/// The two seeded account prefixes.
const SEED_PREFIXES: [&str; 2] = ["CP_SEED_SUPERADMIN", "CP_SEED_ADMIN"];

/// Prose form of every rule, for the generated reference. Keep in step with
/// [`check`].
pub(crate) const DESCRIPTIONS: &[&str] = &[
    "`CP_CADDY_BIN` requires `CP_CADDYFILE`.",
    "`CP_LLM_GATEWAY_KEY` requires `CP_LLM_GATEWAY`.",
    "`CP_FEATURE_IT_PANE=1` requires `CP_CADDYFILE`.",
    "`CP_FEATURE_DAY0_SETUP=1` requires `CP_FEATURE_IT_PANE=1`.",
    "`CP_FEATURE_UPDATER=1` requires `CP_WEB_ROOT`.",
    "`CP_SEED_<ROLE>_EMAIL` requires `CP_SEED_<ROLE>_PASSWORD` or `CP_SEED_<ROLE>_PASSWORD_FILE`.",
    "Any `CP_SEED_*` variable requires `CP_AUTH_ENABLED=1`.",
];

/// Run every rule against the resolved values.
pub(crate) fn check(raw: &Raw, report: &mut Report) {
    set_requires_set(raw, report, "CP_CADDY_BIN", "CP_CADDYFILE");
    set_requires_set(raw, report, "CP_LLM_GATEWAY_KEY", "CP_LLM_GATEWAY");
    on_requires_set(raw, report, "CP_FEATURE_IT_PANE", "CP_CADDYFILE");
    on_requires_on(raw, report, "CP_FEATURE_DAY0_SETUP", "CP_FEATURE_IT_PANE");
    on_requires_set(raw, report, "CP_FEATURE_UPDATER", "CP_WEB_ROOT");
    for prefix in SEED_PREFIXES {
        seed_rules(raw, report, prefix);
    }
}

/// `name` set ⇒ `dependency` set.
fn set_requires_set(raw: &Raw, report: &mut Report, name: &str, dependency: &str) {
    if raw.text(name).is_some() && raw.text(dependency).is_none() {
        report.push(EnvError::rule(name, &format!("requires {dependency} to be set")));
    }
}

/// `name=1` ⇒ `dependency` set.
fn on_requires_set(raw: &Raw, report: &mut Report, name: &str, dependency: &str) {
    if raw.flag(name) == Some(true) && raw.text(dependency).is_none() {
        report.push(EnvError::rule(name, &format!("requires {dependency} to be set when enabled")));
    }
}

/// `name=1` ⇒ `dependency=1`.
fn on_requires_on(raw: &Raw, report: &mut Report, name: &str, dependency: &str) {
    if raw.flag(name) == Some(true) && raw.flag(dependency) != Some(true) {
        report.push(EnvError::rule(name, &format!("requires {dependency}=1 when enabled")));
    }
}

/// An email needs a password (inline or file), and seeding needs auth.
fn seed_rules(raw: &Raw, report: &mut Report, prefix: &str) {
    let email = format!("{prefix}_EMAIL");
    let password = format!("{prefix}_PASSWORD");
    let password_file = format!("{prefix}_PASSWORD_FILE");
    let name = format!("{prefix}_NAME");
    if raw.text(&email).is_some() && raw.text(&password).is_none() && raw.text(&password_file).is_none() {
        report.push(EnvError::rule(&email, &format!("requires {password} or {password_file} to be set")));
    }
    if raw.flag("CP_AUTH_ENABLED") == Some(false) {
        for var in [email, name, password, password_file] {
            if raw.is_explicit(&var) {
                report.push(EnvError::rule(
                    &var,
                    "requires CP_AUTH_ENABLED=1 (accounts are only seeded when auth is enabled)",
                ));
            }
        }
    }
}
