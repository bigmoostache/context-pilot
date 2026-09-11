//! Declarative environment configuration - one table, one validated load, typed
//! access everywhere.
//!
//! * [`specs`] is the table: every variable either binary reads, with its type,
//!   default, scope and documentation. `docs/ENV.md` is generated from it.
//! * [`resolve`] validates a [`source::Source`] against the table and reports
//!   **every** problem at once - unknown `CP_*` names, wrong shapes, missing
//!   paths, incoherent combinations.
//! * [`model::Env`] is the typed result the rest of the workspace reads through
//!   [`env`].
//!
//! # Boot protocol
//!
//! `main` calls [`load`] first thing (after the `.env` files are merged into
//! the process environment), prints the [`resolve::Report`] and exits on
//! failure, otherwise [`install`]s the result. Library code then reads
//! [`env`], which never fails. Outside a validated binary - unit tests - [`env`]
//! falls back to a lenient resolve of the process environment, so a developer's
//! stray `CP_*` export never breaks `cargo test`, while a genuinely mistyped
//! value still surfaces.
//!
//! Credentials are **not** read here: their names are in the table for the
//! docs and the unknown-name check, their values belong to the vault.

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::OnceLock;

// Dev-dependency used by the integration tests only; acknowledge it for the
// unit-test target, which links it too.
#[cfg(test)]
use tempfile as _;

mod invariants;
pub mod model;
pub mod render;
pub mod resolve;
pub mod source;
pub mod spec;
pub mod specs;

use model::Env;
use resolve::{Report, Strictness};
use source::ProcessEnv;
use spec::Target;

/// The configuration installed by `main` (or lazily resolved on first read).
static INSTALLED: OnceLock<Env> = OnceLock::new();

/// A successful strict load: the typed configuration plus the report `main`
/// logs so the journal shows the effective values.
#[derive(Debug)]
pub struct Loaded {
    /// The validated configuration.
    pub env: Env,
    /// Effective values and warnings, secrets redacted.
    pub report: String,
}

/// Validate the process environment strictly for `target`.
///
/// # Errors
///
/// The aggregated [`Report`] - the caller prints it and exits.
pub fn load(target: Target) -> Result<Loaded, Report> {
    let raw = resolve::resolve(target, &ProcessEnv, Strictness::Strict)?;
    let report = render::check_report(&raw, &ProcessEnv);
    Ok(Loaded { env: Env::from_raw(&raw, &cwd()), report })
}

/// Make `env` the configuration every later [`env`] call returns.
///
/// Returns `false` when a configuration was already installed (or already
/// lazily resolved) - the first one wins, so `main` must call this before any
/// library code reads [`env`].
#[must_use]
pub fn install(env: Env) -> bool {
    INSTALLED.set(env).is_ok()
}

/// The installed configuration.
///
/// Before [`install`] (tests, tooling) it resolves the process environment
/// leniently - unknown `CP_*` names ignored, every other rule enforced - and
/// aborts the process with the report if that fails, since no caller can do
/// anything useful with an invalid environment.
pub fn env() -> &'static Env {
    INSTALLED.get_or_init(|| {
        resolve::resolve(Target::Any, &ProcessEnv, Strictness::Lenient)
            .map_or_else(|report| fatal(&report), |raw| Env::from_raw(&raw, &cwd()))
    })
}

/// `--check-env`: the report text and whether the environment is valid.
#[must_use]
pub fn check(target: Target) -> (String, bool) {
    match resolve::resolve(target, &ProcessEnv, Strictness::Strict) {
        Ok(raw) => (render::check_report(&raw, &ProcessEnv), true),
        Err(report) => (report.to_string(), false),
    }
}

/// The environment was read before validation and it is invalid: nothing
/// downstream can proceed, so print the report and abort.
fn fatal(report: &Report) -> ! {
    let text = format!("cp-env: the environment was read before validation and it is invalid\n{report}\n");
    drop(std::io::stderr().write_all(text.as_bytes()));
    std::process::abort()
}

/// The working directory, or `.` when it cannot be determined.
fn cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_err| PathBuf::from("."))
}
