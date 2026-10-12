//! Per-worktree PR badge (`#123-🟢`) for the TUI threads list.
//!
//! The list renders every frame, so [`badge`] never blocks: it returns the
//! cached value and, when that is older than [`TTL`], refreshes it on a
//! background thread (`git rev-parse` + `gh pr view`, ~1s). Keyed by workdir,
//! so threads sharing a worktree share one poll.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::{Duration, Instant};

use cp_base::modules::run_with_timeout;

/// How long a fetched badge stays fresh before a background refresh.
const TTL: Duration = Duration::from_mins(1);

/// Timeout for the `git rev-parse` branch lookup.
const GIT_TIMEOUT_SECS: u64 = 5;

/// CI verdict of an open PR, rendered as a coloured circle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckColor {
    /// Checks passing, or no checks at all.
    Green,
    /// At least one check still running.
    Yellow,
    /// At least one check failed.
    Red,
}

/// Open PR linked to a worktree's branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Badge {
    /// PR number.
    pub number: u64,
    /// CI verdict.
    pub color: CheckColor,
}

impl Badge {
    /// `#123-🟢` form shown in the threads list.
    #[must_use]
    pub fn label(&self) -> String {
        let dot = match self.color {
            CheckColor::Green => "\u{1f7e2}",
            CheckColor::Yellow => "\u{1f7e1}",
            CheckColor::Red => "\u{1f534}",
        };
        format!("#{}-{dot}", self.number)
    }
}

/// Cached poll result for one workdir.
struct Slot {
    /// Last fetched badge (`None` = no open PR, or not fetched yet).
    badge: Option<Badge>,
    /// When the last fetch finished (`None` = never).
    fetched: Option<Instant>,
    /// A background refresh is running.
    in_flight: bool,
}

/// Workdir → cached badge.
static CACHE: LazyLock<Mutex<HashMap<PathBuf, Slot>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Cached badge for `workdir`'s branch; schedules a background refresh when
/// stale. Never blocks on git/gh.
#[must_use]
pub fn badge(workdir: &Path, token: &str) -> Option<Badge> {
    let mut cache = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
    let slot = cache.entry(workdir.to_path_buf()).or_insert(Slot { badge: None, fetched: None, in_flight: false });
    let spawn = !slot.in_flight && slot.fetched.is_none_or(|t| t.elapsed() >= TTL);
    if spawn {
        slot.in_flight = true;
    }
    let cached = slot.badge;
    drop(cache);
    if spawn {
        let (dir, tok) = (workdir.to_path_buf(), token.to_owned());
        let _handle = std::thread::spawn(move || refresh(&dir, &tok));
    }
    cached
}

/// Background body: fetch, then store the result and clear `in_flight`.
fn refresh(dir: &Path, token: &str) {
    let fetched = fetch(dir, token);
    if let Some(slot) = CACHE.lock().unwrap_or_else(PoisonError::into_inner).get_mut(dir) {
        slot.badge = fetched;
        slot.fetched = Some(Instant::now());
        slot.in_flight = false;
    }
}

/// Resolve `dir`'s branch, then its open PR and CI verdict.
fn fetch(dir: &Path, token: &str) -> Option<Badge> {
    let mut cmd = Command::new("git");
    let _r = cmd
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(dir)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE");
    let out = run_with_timeout(cmd, GIT_TIMEOUT_SECS).ok().filter(|o| o.status.success())?;
    let branch = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if branch.is_empty() || branch == "HEAD" {
        return None;
    }
    let (_hash, info) = crate::parse::poll_branch_pr(&branch, token, None)?;
    let pr = info.filter(|p| p.state == "OPEN")?;
    let color = match pr.checks_status.as_deref() {
        Some("failing") => CheckColor::Red,
        Some("pending") => CheckColor::Yellow,
        _ => CheckColor::Green,
    };
    Some(Badge { number: pr.number, color })
}
