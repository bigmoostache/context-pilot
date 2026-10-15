//! Cross-variable rules - combinations that are individually valid but jointly
//! incoherent. Each one is an error, reported next to the per-variable ones.
//!
//! The pairwise rules are data ([`RULES`]): the check and the prose in
//! `docs/ENV.md` are both derived from the same entry, so they cannot drift.
//! A rule whose variables already failed validation is skipped - the value
//! is absent from [`Raw`] because it was rejected, not because it is unset,
//! and a second "requires X to be set" error would point at the wrong cause.

use std::fmt::Write as _;

use crate::resolve::{EnvError, Raw, Report};

/// The two seeded account prefixes.
const SEED_PREFIXES: [&str; 2] = ["CP_SEED_SUPERADMIN", "CP_SEED_ADMIN"];

/// What a rule demands of a variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Resolved to any value.
    Set,
    /// A boolean resolved to `1`.
    On,
}

/// `name` in state `when` ⇒ `dependency` in state `then`.
#[derive(Debug, Clone, Copy)]
struct Rule {
    /// The variable the rule constrains (errors are reported against it).
    name: &'static str,
    /// The state of `name` that triggers the rule.
    when: State,
    /// The variable `name` depends on.
    dependency: &'static str,
    /// The state `dependency` must be in.
    then: State,
}

/// Every pairwise rule, in docs order.
const RULES: [Rule; 5] = [
    Rule { name: "CP_CADDY_BIN", when: State::Set, dependency: "CP_CADDYFILE", then: State::Set },
    Rule { name: "CP_LLM_GATEWAY_KEY", when: State::Set, dependency: "CP_LLM_GATEWAY", then: State::Set },
    Rule { name: "CP_FEATURE_IT_PANE", when: State::On, dependency: "CP_CADDYFILE", then: State::Set },
    Rule { name: "CP_FEATURE_DAY0_SETUP", when: State::On, dependency: "CP_FEATURE_IT_PANE", then: State::On },
    Rule { name: "CP_FEATURE_UPDATER", when: State::On, dependency: "CP_WEB_ROOT", then: State::Set },
];

/// Prose of the seed rules, which span a name family rather than a pair.
const SEED_DESCRIPTIONS: [&str; 2] = [
    "`CP_SEED_<ROLE>_EMAIL` requires `CP_SEED_<ROLE>_PASSWORD` or `CP_SEED_<ROLE>_PASSWORD_FILE`.",
    "Any `CP_SEED_*` variable requires `CP_AUTH_ENABLED=1`.",
];

impl State {
    /// Whether `name` is in this state.
    fn holds(self, raw: &Raw, name: &str) -> bool {
        match self {
            Self::Set => raw.text(name).is_some(),
            Self::On => raw.flag(name) == Some(true),
        }
    }

    /// Markdown form of `name` in this state.
    fn prose(self, name: &str) -> String {
        match self {
            Self::Set => format!("`{name}`"),
            Self::On => format!("`{name}=1`"),
        }
    }
}

impl Rule {
    /// The error text, worded from the two states.
    fn message(self) -> String {
        let need = match self.then {
            State::Set => format!("requires {} to be set", self.dependency),
            State::On => format!("requires {}=1", self.dependency),
        };
        if self.when == State::On { format!("{need} when enabled") } else { need }
    }

    /// The docs line.
    fn description(self) -> String {
        format!("{} requires {}.", self.when.prose(self.name), self.then.prose(self.dependency))
    }
}

/// Prose form of every rule, for the generated reference.
pub(crate) fn descriptions() -> Vec<String> {
    let mut lines: Vec<String> = RULES.iter().map(|rule| rule.description()).collect();
    lines.extend(SEED_DESCRIPTIONS.iter().map(|&line| line.to_owned()));
    lines
}

/// Run every rule against the resolved values. `report` already holds the
/// per-variable errors; rules touching a rejected variable are skipped.
pub(crate) fn check(raw: &Raw, report: &mut Report) {
    for rule in RULES {
        let rejected = report.mentions(rule.name) || report.mentions(rule.dependency);
        if !rejected && rule.when.holds(raw, rule.name) && !rule.then.holds(raw, rule.dependency) {
            report.push(EnvError::rule(rule.name, &rule.message()));
        }
    }
    for prefix in SEED_PREFIXES {
        seed_rules(raw, report, prefix);
    }
}

/// An email needs a password (inline or file), and seeding needs auth.
fn seed_rules(raw: &Raw, report: &mut Report, prefix: &str) {
    let email = format!("{prefix}_EMAIL");
    let password = format!("{prefix}_PASSWORD");
    let password_file = format!("{prefix}_PASSWORD_FILE");
    let name = format!("{prefix}_NAME");
    let rejected = report.mentions(&password) || report.mentions(&password_file);
    if !rejected && raw.text(&email).is_some() && raw.text(&password).is_none() && raw.text(&password_file).is_none() {
        let mut rule = String::new();
        let _w = write!(rule, "requires {password} or {password_file} to be set");
        report.push(EnvError::rule(&email, &rule));
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The generated prose is the exact text the hand-written list carried
    /// before the rules became data, so `docs/ENV.md` is unchanged.
    #[test]
    fn descriptions_match_rules() {
        assert_eq!(
            descriptions().get(..5),
            Some(
                [
                    "`CP_CADDY_BIN` requires `CP_CADDYFILE`.".to_owned(),
                    "`CP_LLM_GATEWAY_KEY` requires `CP_LLM_GATEWAY`.".to_owned(),
                    "`CP_FEATURE_IT_PANE=1` requires `CP_CADDYFILE`.".to_owned(),
                    "`CP_FEATURE_DAY0_SETUP=1` requires `CP_FEATURE_IT_PANE=1`.".to_owned(),
                    "`CP_FEATURE_UPDATER=1` requires `CP_WEB_ROOT`.".to_owned(),
                ]
                .as_slice()
            )
        );
    }
}
