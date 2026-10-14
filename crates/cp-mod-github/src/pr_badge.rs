//! Per-worktree PR badge (`#123 ●`, dot coloured by CI) for the TUI agents list.
//!
//! The list renders every frame, so [`badge`] never blocks: it registers the
//! workdir and returns the cached value. At most once per [`TTL`], one
//! background sweep resolves every registered workdir's branch locally
//! (`git rev-parse`) and asks GitHub once per repository (`gh pr list`), not
//! once per branch. A failed call (rate limit, network) keeps the previous
//! badges: stale beats blank. The last good answer per repository is also
//! saved under its git common dir, so a fresh process shows badges before
//! its first successful call.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::{Duration, Instant};

use cp_base::modules::run_with_timeout;
use serde_json::Value;

/// A workdir of a repository and its current branch.
type Member = (PathBuf, String);

/// Minimum gap between two sweeps (successful or not).
const TTL: Duration = Duration::from_mins(1);

/// A workdir not rendered for this long is dropped from the sweep.
const FORGET_AFTER: Duration = Duration::from_mins(10);

/// Timeout for the `git rev-parse` branch lookup.
const GIT_TIMEOUT_SECS: u64 = 5;

/// Open PRs fetched per repository and sweep.
const PR_LIST_LIMIT: &str = "200";

/// Last-good badges of a repository, keyed by branch (inside its git common dir).
const DISK_CACHE: &str = "cp-pr-badges.json";

/// State of an open PR, rendered as a coloured circle. Conflicts win over CI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckColor {
    /// Checks passing, or no checks at all.
    Green,
    /// At least one check still running.
    Yellow,
    /// At least one check failed.
    Red,
    /// Merge conflicts with the base branch.
    Blue,
}

impl CheckColor {
    /// Stable name for the disk cache.
    const fn name(self) -> &'static str {
        match self {
            Self::Green => "green",
            Self::Yellow => "yellow",
            Self::Red => "red",
            Self::Blue => "blue",
        }
    }

    /// Inverse of [`Self::name`].
    fn from_name(s: &str) -> Option<Self> {
        match s {
            "green" => Some(Self::Green),
            "yellow" => Some(Self::Yellow),
            "red" => Some(Self::Red),
            "blue" => Some(Self::Blue),
            _ => None,
        }
    }
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
    /// `#123` text; the caller renders the coloured `●` after it.
    #[must_use]
    pub fn label(&self) -> String {
        format!("#{}", self.number)
    }
}

/// Shared poller state.
#[derive(Default)]
struct Poller {
    /// Workdir → last time the UI asked for it.
    wanted: HashMap<PathBuf, Instant>,
    /// Workdir → badge (`None` = known to have no open PR). Absent = unknown.
    badges: HashMap<PathBuf, Option<Badge>>,
    /// Start of the last sweep (`None` = never).
    last_sweep: Option<Instant>,
    /// A sweep is running.
    in_flight: bool,
    /// GitHub token for the next sweep.
    token: String,
}

/// Process-wide poller.
static POLLER: LazyLock<Mutex<Poller>> = LazyLock::new(|| Mutex::new(Poller::default()));

/// Cached badge for `workdir`'s branch; starts a background sweep when the
/// last one is older than [`TTL`]. Never blocks on git/gh.
#[must_use]
pub fn badge(workdir: &Path, token: &str) -> Option<Badge> {
    let mut p = POLLER.lock().unwrap_or_else(PoisonError::into_inner);
    let now = Instant::now();
    let _prev = p.wanted.insert(workdir.to_path_buf(), now);
    let due = !p.in_flight && p.last_sweep.is_none_or(|t| now.duration_since(t) >= TTL);
    if due {
        p.in_flight = true;
        p.last_sweep = Some(now);
        token.clone_into(&mut p.token);
    }
    let cached = p.badges.get(workdir).copied().flatten();
    drop(p);
    if due {
        let _handle = std::thread::spawn(sweep);
    }
    cached
}

/// Background sweep: one `gh pr list` per repository, then store results.
fn sweep() {
    let (dirs, token) = {
        let mut p = POLLER.lock().unwrap_or_else(PoisonError::into_inner);
        let now = Instant::now();
        p.wanted.retain(|_, seen| now.duration_since(*seen) < FORGET_AFTER);
        let wanted: Vec<PathBuf> = p.wanted.keys().cloned().collect();
        p.badges.retain(|dir, _| wanted.contains(dir));
        (wanted, p.token.clone())
    };

    // Group workdirs by repository (git common dir), keeping each one's branch.
    let mut repos: BTreeMap<PathBuf, Vec<Member>> = BTreeMap::new();
    for dir in dirs {
        if let Some((common, branch)) = locate(&dir) {
            repos.entry(common).or_default().push((dir, branch));
        }
    }

    let mut updates: Vec<(PathBuf, Option<Badge>)> = Vec::new();
    let mut fallbacks: Vec<(PathBuf, Option<Badge>)> = Vec::new();
    for (common, members) in repos {
        let Some(first) = members.first().map(|m| &m.0) else { continue };
        if let Some(by_branch) = list_open_prs(first, &token) {
            save_disk(&common, &by_branch);
            updates.extend(members.into_iter().map(|(dir, br)| {
                let b = by_branch.get(&br).copied();
                (dir, b)
            }));
        } else {
            let by_branch = load_disk(&common);
            fallbacks.extend(members.into_iter().map(|(dir, br)| {
                let b = by_branch.get(&br).copied();
                (dir, b)
            }));
        }
    }

    let mut p = POLLER.lock().unwrap_or_else(PoisonError::into_inner);
    for (dir, b) in updates {
        let _prev = p.badges.insert(dir, b);
    }
    // Failed call: only fill workdirs with no in-memory value yet.
    for (dir, b) in fallbacks {
        let _slot = p.badges.entry(dir).or_insert(b);
    }
    p.in_flight = false;
}

/// `dir`'s absolute git common dir and current branch (`None` when detached).
fn locate(dir: &Path) -> Option<(PathBuf, String)> {
    let mut cmd = Command::new("git");
    let _r = cmd
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir", "--abbrev-ref", "HEAD"])
        .current_dir(dir)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE");
    let out = run_with_timeout(cmd, GIT_TIMEOUT_SECS).ok().filter(|o| o.status.success())?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines = text.lines().map(str::trim);
    let common = PathBuf::from(lines.next()?);
    let branch = lines.next()?.to_owned();
    if branch.is_empty() || branch == "HEAD" {
        return None;
    }
    Some((common, branch))
}

/// Every open PR of `dir`'s repository, keyed by head branch. `None` when the
/// call fails (rate limit, network, auth): callers keep what they had.
fn list_open_prs(dir: &Path, token: &str) -> Option<HashMap<String, Badge>> {
    let mut cmd = Command::new("gh");
    let _r = cmd
        .args([
            "pr",
            "list",
            "--state",
            "open",
            "--limit",
            PR_LIST_LIMIT,
            "--json",
            "number,headRefName,mergeable,statusCheckRollup",
        ])
        .current_dir(dir)
        .env("GITHUB_TOKEN", token)
        .env("GH_TOKEN", token)
        .env("GH_PROMPT_DISABLED", "1")
        .env("NO_COLOR", "1");
    let out = run_with_timeout(cmd, crate::GH_CMD_TIMEOUT_SECS).ok().filter(|o| o.status.success())?;
    let prs: Vec<Value> = serde_json::from_slice(&out.stdout).ok()?;
    Some(prs.iter().filter_map(pr_entry).collect())
}

/// `(head branch, badge)` of one `gh pr list` entry.
fn pr_entry(pr: &Value) -> Option<(String, Badge)> {
    let number = pr.get("number")?.as_u64()?;
    let branch = pr.get("headRefName")?.as_str()?.to_owned();
    let conflicting = pr.get("mergeable").and_then(Value::as_str) == Some("CONFLICTING");
    let checks = crate::parse::parse_checks_status(&pr.to_string());
    let color = match checks.as_deref() {
        _ if conflicting => CheckColor::Blue,
        Some("failing") => CheckColor::Red,
        Some("pending") => CheckColor::Yellow,
        _ => CheckColor::Green,
    };
    Some((branch, Badge { number, color }))
}

/// Persist a repository's last good answer (best effort).
fn save_disk(common: &Path, by_branch: &HashMap<String, Badge>) {
    let map: serde_json::Map<String, Value> = by_branch
        .iter()
        .map(|(br, b)| (br.clone(), Value::Array(vec![Value::from(b.number), Value::from(b.color.name())])))
        .collect();
    let _ignored = std::fs::write(common.join(DISK_CACHE), Value::Object(map).to_string());
}

/// Read a repository's last good answer; empty when missing or unreadable.
fn load_disk(common: &Path) -> HashMap<String, Badge> {
    let Ok(text) = std::fs::read_to_string(common.join(DISK_CACHE)) else { return HashMap::new() };
    let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&text) else { return HashMap::new() };
    map.into_iter()
        .filter_map(|(br, v)| {
            let number = v.get(0)?.as_u64()?;
            let color = CheckColor::from_name(v.get(1)?.as_str()?)?;
            Some((br, Badge { number, color }))
        })
        .collect()
}
