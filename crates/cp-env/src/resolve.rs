//! The validator: every [`Spec`] against a [`Source`], all errors at once.
//!
//! [`resolve`] is a pure function of its inputs (plus the filesystem, for path
//! preconditions). It never reads the process environment itself, which keeps
//! every rule unit-testable with an in-memory map.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::source::Source;
use crate::spec::{Exist, Fallback, Kind, Spec, Target};
use crate::{invariants, specs};

/// One rejected variable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvError {
    /// Variable name.
    pub name: String,
    /// The value received, when the problem is the value (never a secret).
    pub got: Option<String>,
    /// What was expected, or the rule that was broken.
    pub expected: String,
    /// Extra qualification appended in parentheses (`not found`, …).
    pub detail: Option<String>,
}

impl EnvError {
    /// A rule violation with no offending value to echo.
    pub(crate) fn rule(name: &str, rule: &str) -> Self {
        Self { name: name.to_owned(), got: None, expected: rule.to_owned(), detail: None }
    }

    /// A value that failed its type or precondition check.
    pub(crate) fn bad(spec: &Spec, got: &str, expected: &str, detail: Option<String>) -> Self {
        let shown = if spec.sensitive { "[redacted]".to_owned() } else { got.to_owned() };
        Self { name: spec.name.to_owned(), got: Some(shown), expected: expected.to_owned(), detail }
    }
}

impl fmt::Display for EnvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let head = self
            .got
            .as_deref()
            .map_or_else(|| self.expected.clone(), |got| format!("expected {}, got \"{got}\"", self.expected));
        let tail = self.detail.as_deref().map_or_else(String::new, |detail| format!(" ({detail})"));
        write!(f, "{}: {head}{tail}", self.name)
    }
}

/// Every error found in one pass, rendered as one block.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Report {
    /// The errors, in table order (unknown names first, sorted).
    errors: Vec<EnvError>,
}

impl Report {
    /// The errors, in report order.
    #[must_use]
    pub fn errors(&self) -> &[EnvError] {
        &self.errors
    }

    /// Whether nothing was rejected.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    /// Whether `name` has at least one error.
    #[must_use]
    pub fn mentions(&self, name: &str) -> bool {
        self.errors.iter().any(|err| err.name == name)
    }

    /// Record one error.
    pub(crate) fn push(&mut self, err: EnvError) {
        self.errors.push(err);
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let count = self.errors.len();
        let plural = if count == 1 { "" } else { "s" };
        writeln!(f, "environment configuration is invalid ({count} error{plural}):")?;
        for err in &self.errors {
            writeln!(f, "  - {err}")?;
        }
        write!(f, "see docs/ENV.md for the full reference")
    }
}

/// Whether unknown `CP_*` names are rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strictness {
    /// Production: an unknown `CP_*` variable is an error.
    Strict,
    /// In-process fallback (tests): unknown names are ignored, every other
    /// rule still applies.
    Lenient,
}

/// One resolved value.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    /// Normalised text (`1`/`0` for booleans, trimmed otherwise).
    text: String,
    /// Whether the value came from the source rather than a literal default.
    explicit: bool,
}

/// The validated, normalised values, keyed by name.
///
/// Typed accessors re-parse the normalised text; after [`resolve`] succeeded
/// that cannot fail, so they return `None` only for variables that are
/// genuinely unset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Raw {
    /// Resolved values.
    values: BTreeMap<&'static str, Entry>,
    /// Non-fatal observations (a defaulted path that does not exist).
    warnings: Vec<String>,
    /// The binary this was resolved for.
    target: Target,
}

impl Raw {
    /// Empty set for `target`.
    #[must_use]
    pub(crate) const fn new(target: Target) -> Self {
        Self { values: BTreeMap::new(), warnings: Vec::new(), target }
    }

    /// The binary this was resolved for.
    #[must_use]
    pub const fn target(&self) -> Target {
        self.target
    }

    /// Normalised text of `name`, if resolved.
    #[must_use]
    pub fn text(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(|entry| entry.text.as_str())
    }

    /// Whether `name` was set by the source (as opposed to defaulted).
    #[must_use]
    pub fn is_explicit(&self, name: &str) -> bool {
        self.values.get(name).is_some_and(|entry| entry.explicit)
    }

    /// Boolean value of `name`, if resolved.
    #[must_use]
    pub fn flag(&self, name: &str) -> Option<bool> {
        self.text(name).map(|text| text == "1")
    }

    /// Integer value of `name`, if resolved.
    #[must_use]
    pub fn integer(&self, name: &str) -> Option<u64> {
        let text = self.text(name)?;
        text.parse().ok()
    }

    /// Path value of `name`, if resolved.
    #[must_use]
    pub fn path(&self, name: &str) -> Option<PathBuf> {
        self.text(name).map(PathBuf::from)
    }

    /// Resolved names, sorted.
    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.values.keys().copied()
    }

    /// Non-fatal observations gathered while resolving.
    #[must_use]
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Record a resolved value.
    fn insert(&mut self, name: &'static str, text: String, explicit: bool) {
        drop(self.values.insert(name, Entry { text, explicit }));
    }
}

/// Validate every variable `target` parses against `src`.
///
/// # Errors
///
/// A [`Report`] listing **every** problem: unknown `CP_*` names (strict mode),
/// missing required variables, values of the wrong shape, paths failing their
/// precondition, and cross-variable rules - never just the first one.
pub fn resolve(target: Target, src: &dyn Source, strictness: Strictness) -> Result<Raw, Report> {
    let mut report = Report::default();
    let mut raw = Raw::new(target);
    if strictness == Strictness::Strict {
        unknown_names(src, &mut report);
    }
    for spec in specs::all().iter().filter(|spec| spec.scope.parsed_for(target) && !spec.is_secret()) {
        resolve_one(spec, src, &mut raw, &mut report);
    }
    invariants::check(&raw, &mut report);
    if report.is_empty() { Ok(raw) } else { Err(report) }
}

/// Reject every `CP_*` name the table does not know.
fn unknown_names(src: &dyn Source, report: &mut Report) {
    let mut unknown: Vec<String> =
        src.names().into_iter().filter(|name| name.starts_with("CP_") && specs::find(name).is_none()).collect();
    unknown.sort();
    for name in unknown {
        report.push(EnvError::rule(&name, "unknown variable (not in the CP_* table; see docs/ENV.md)"));
    }
}

/// Resolve one spec: explicit value (validated) or its fallback.
fn resolve_one(spec: &Spec, src: &dyn Source, raw: &mut Raw, report: &mut Report) {
    let explicit = src.get(spec.name).map(|value| value.trim().to_owned()).filter(|value| !value.is_empty());
    let Some(value) = explicit else {
        apply_fallback(spec, raw, report);
        return;
    };
    match validate(spec, &value) {
        Ok(normalised) => raw.insert(spec.name, normalised, true),
        Err(err) => report.push(err),
    }
}

/// Apply the literal default, or flag a missing required variable.
fn apply_fallback(spec: &Spec, raw: &mut Raw, report: &mut Report) {
    match spec.fallback {
        Fallback::Literal(text) => {
            warn_missing_default(spec, text, raw);
            raw.insert(spec.name, text.to_owned(), false);
        }
        Fallback::Derived(_) | Fallback::None => {
            if spec.required {
                report.push(EnvError::rule(spec.name, "required but unset"));
            }
        }
    }
}

/// A defaulted path that fails its precondition is a warning, not an error.
fn warn_missing_default(spec: &Spec, text: &str, raw: &mut Raw) {
    if let Kind::Path(exist) = spec.kind
        && exist != Exist::No
        && path_detail(Path::new(text), exist).is_some()
    {
        raw.warnings
            .push(format!("{}: default path \"{text}\" does not exist yet (set it explicitly to silence)", spec.name));
    }
}

/// Type-check and normalise one explicit value.
fn validate(spec: &Spec, text: &str) -> Result<String, EnvError> {
    match spec.kind {
        Kind::Bool => parse_bool(text)
            .map(|on| if on { "1" } else { "0" }.to_owned())
            .ok_or_else(|| EnvError::bad(spec, text, "a boolean (0, 1, true or false)", None)),
        Kind::U16 => parse_int(text, 1, u64::from(u16::MAX))
            .map(|_| text.to_owned())
            .ok_or_else(|| EnvError::bad(spec, text, "an integer 1-65535", None)),
        Kind::U64 => parse_int(text, spec.min, u64::MAX)
            .map(|_| text.to_owned())
            .ok_or_else(|| EnvError::bad(spec, text, &format!("an integer of at least {}", spec.min), None)),
        Kind::Str | Kind::Secret => Ok(text.to_owned()),
        Kind::Path(exist) => check_path(spec, text, exist),
        Kind::Url => normalise_url(text).ok_or_else(|| EnvError::bad(spec, text, "an http(s) URL", None)),
    }
}

/// `0`/`1`/`true`/`false`, case-insensitive.
fn parse_bool(text: &str) -> Option<bool> {
    match text.to_ascii_lowercase().as_str() {
        "1" | "true" => Some(true),
        "0" | "false" => Some(false),
        _other => None,
    }
}

/// Unsigned integer within `min..=max`.
fn parse_int(text: &str, min: u64, max: u64) -> Option<u64> {
    text.parse::<u64>().ok().filter(|value| (min..=max).contains(value))
}

/// `http(s)://…` with no whitespace; trailing slashes dropped.
fn normalise_url(text: &str) -> Option<String> {
    let scheme_ok = text.starts_with("http://") || text.starts_with("https://");
    (scheme_ok && !text.chars().any(char::is_whitespace)).then(|| text.trim_end_matches('/').to_owned())
}

/// Check the path precondition of an explicit value.
fn check_path(spec: &Spec, text: &str, exist: Exist) -> Result<String, EnvError> {
    let expected = match exist {
        Exist::No => return Ok(text.to_owned()),
        Exist::File => "an existing file",
        Exist::Dir => "an existing directory",
        Exist::Parent => "a path whose parent directory exists",
        Exist::Executable => "an executable file",
    };
    path_detail(Path::new(text), exist)
        .map_or_else(|| Ok(text.to_owned()), |detail| Err(EnvError::bad(spec, text, expected, Some(detail))))
}

/// Why `path` fails `exist`, or `None` when it satisfies it.
fn path_detail(path: &Path, exist: Exist) -> Option<String> {
    match exist {
        Exist::No => None,
        Exist::File => missing_detail(path, path.is_file(), "not a regular file"),
        Exist::Dir => missing_detail(path, path.is_dir(), "not a directory"),
        Exist::Parent => {
            let parent = path
                .parent()
                .filter(|dir| !dir.as_os_str().is_empty())
                .map_or_else(|| Path::new(".").to_path_buf(), Path::to_path_buf);
            missing_detail(&parent, parent.is_dir(), "parent is not a directory")
        }
        Exist::Executable => {
            if !path.is_file() {
                return missing_detail(path, false, "not a regular file");
            }
            (!is_executable(path)).then(|| "not executable".to_owned())
        }
    }
}

/// `None` when `ok`; otherwise `not found` or the given qualification.
fn missing_detail(path: &Path, ok: bool, otherwise: &str) -> Option<String> {
    if ok {
        None
    } else if path.exists() {
        Some(otherwise.to_owned())
    } else {
        Some("not found".to_owned())
    }
}

/// Any execute bit set (owner, group or other).
#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::metadata(path).is_ok_and(|meta| (meta.permissions().mode() & 0o111) != 0)
}

/// Non-Unix platforms have no execute bit; existence is enough.
#[cfg(not(unix))]
fn is_executable(_path: &Path) -> bool {
    true
}
