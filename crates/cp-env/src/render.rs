//! Text renderings of the table and of a resolved environment.

use std::fmt::Write as _;

use crate::resolve::Raw;
use crate::source::Source;
use crate::spec::{Group, Kind, Scope, Spec};
use crate::{invariants, specs};

/// The marker line at the top of the generated reference.
pub const GENERATED_MARKER: &str = "<!-- GENERATED from crates/cp-env/src/specs/ by `cargo test -p cp-env --test generate -- --ignored`. Do not edit by hand. -->";

/// `docs/ENV.md` - the complete reference, generated from the spec table.
#[must_use]
pub fn env_md() -> String {
    let mut out = String::new();
    header(&mut out);
    for group in Group::ALL {
        section(&mut out, group);
    }
    out
}

/// Preamble: rules every variable obeys.
fn header(out: &mut String) {
    out.push_str("# Environment variables\n\n");
    out.push_str(GENERATED_MARKER);
    out.push_str("\n\n");
    out.push_str(
        "Every variable either binary reads is declared once, in `crates/cp-env/src/specs/`. \
         Validation, the typed configuration and this page are derived from that table.\n\n",
    );
    out.push_str("## Rules\n\n");
    out.push_str(
        "- **Precedence**: the process environment, then the project `.env`, then `~/.context-pilot/.env` \
         (each file overrides what came before it - the global file is where the cockpit writes provider keys).\n",
    );
    out.push_str(
        "- **Strict**: at boot, both binaries validate everything at once and refuse to start on any \
         problem, listing every offending variable, the value received and what was expected. \
         `cp-orchestrator --check-env` and `cpilot --check-env` run the same validation and exit.\n",
    );
    out.push_str("- **Unknown names**: a `CP_*` variable absent from this table is an error.\n");
    out.push_str(
        "- **Empty means unset**: a variable set to the empty string counts as unset (blank a line to disable it).\n",
    );
    out.push_str("- **Booleans**: exactly `0`, `1`, `true` or `false`.\n");
    out.push_str(
        "- **Paths**: an explicitly set path must satisfy its precondition (existing file, directory, executable, parent). \
         A *defaulted* path is never checked; a missing default is logged as a warning.\n",
    );
    out.push_str("- **Scope**: each binary parses the variables of its own scope plus the shared ones; the other binary's names are known (never \"unknown\") but ignored. The orchestrator passes its whole environment to every agent it spawns.\n\n");
    out.push_str("## Invariants\n\n");
    out.push_str("Combinations rejected at boot:\n\n");
    for rule in invariants::DESCRIPTIONS {
        let _w = writeln!(out, "- {rule}");
    }
    out.push('\n');
}

/// One group: title, blurb, table.
fn section(out: &mut String, group: Group) {
    let _title = writeln!(out, "## {}\n", group.title());
    let _blurb = writeln!(out, "{}\n", group.blurb());
    out.push_str("| Variable | Type | Default | Scope | Required | Description |\n");
    out.push_str("|---|---|---|---|---|---|\n");
    for spec in specs::all().iter().filter(|spec| spec.group == group) {
        let required = if spec.required { "yes" } else { "" };
        let _row = writeln!(
            out,
            "| `{}` | {} | {} | {} | {} | {} |",
            spec.name,
            spec.kind.label(),
            default_cell(spec.fallback.label()),
            spec.scope.label(),
            required,
            spec.doc
        );
    }
    out.push('\n');
}

/// Defaults are code; the dash is prose.
fn default_cell(label: &str) -> String {
    if label == "-" { label.to_owned() } else { format!("`{label}`") }
}

/// The `--check-env` success report: every resolved value (secrets and
/// sensitive values redacted), then the warnings.
#[must_use]
pub fn check_report(raw: &Raw, src: &dyn Source) -> String {
    let mut out = String::new();
    let count = raw.names().count();
    let _head = writeln!(out, "environment OK (target: {}) - {count} variables resolved", raw.target().label());
    for spec in specs::all() {
        if let Some(line) = report_line(spec, raw, src) {
            let _line = writeln!(out, "  {line}");
        }
    }
    if !raw.warnings().is_empty() {
        out.push_str("warnings:\n");
        for warning in raw.warnings() {
            let _line = writeln!(out, "  - {warning}");
        }
    }
    out
}

/// One report line, or `None` when the spec has nothing to show for this
/// target: a secret shows presence only; a resolved value shows its origin.
fn report_line(spec: &Spec, raw: &Raw, src: &dyn Source) -> Option<String> {
    if spec.is_secret() {
        let parsed = spec.scope.parsed_for(raw.target());
        let state = if src.get(spec.name).is_some() { "set" } else { "unset" };
        return parsed.then(|| format!("{} = {state} (secret, resolved by the vault)", spec.name));
    }
    let text = raw.text(spec.name)?;
    let shown = if spec.sensitive { "[redacted]" } else { text };
    let origin = if raw.is_explicit(spec.name) { "" } else { " (default)" };
    Some(format!("{} = {shown}{origin}", spec.name))
}

/// The variables a deployment profile is expected to spell out explicitly
/// (used by the consistency test): every flag, since defaults are not a
/// deployment decision.
#[must_use]
pub fn profile_explicit_names() -> Vec<&'static str> {
    specs::all()
        .iter()
        .filter(|spec| spec.group == Group::Features && spec.scope == Scope::Orchestrator && spec.kind == Kind::Bool)
        .map(|spec| spec.name)
        .collect()
}
