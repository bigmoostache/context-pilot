use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use cp_base::config::constants;

use crate::types::{PromptItem, PromptType};

/// Subdirectory names under .context-pilot/ for each prompt type
const fn subdir_for(pt: PromptType) -> &'static str {
    match pt {
        PromptType::Agent => "agents",
        PromptType::Skill => "skills",
        PromptType::Command => "commands",
    }
}

/// Fleet-shared directory for a prompt type (`~/.context-pilot/behaviours/<subdir>`).
///
/// Every agent-side reader/writer funnels through here, so this single resolver
/// is what makes behaviours fleet-shared (T651).
#[must_use]
pub fn dir_for(pt: PromptType) -> PathBuf {
    constants::home_behaviours_dir().join(subdir_for(pt))
}

/// The OLD per-realm behaviour dir a prompt type used to live in
/// (`./.context-pilot/<subdir>`, relative to the agent's realm). Migration
/// reads FROM here into the shared [`dir_for`] location.
fn legacy_local_dir(pt: PromptType) -> PathBuf {
    PathBuf::from(constants::STORE_DIR).join(subdir_for(pt))
}

/// Migrate any per-realm behaviour `.md` files into the fleet-shared home dir,
/// once, at boot (T651). Idempotent: after it runs the local dirs hold no `.md`
/// files, so re-runs are no-ops.
///
/// Per file: move it to `~/.context-pilot/behaviours/<subdir>/<id>.md`. On an
/// id collision with an already-shared file, compare bytes — identical means
/// the shared copy already IS this file (just drop the local one); differing
/// means the incoming local file is written under the next free `-<n>` suffix,
/// so the already-shared file keeps the plain id, untouched. The now-empty
/// local dirs are left in place (harmless; avoids dir-removal races).
pub fn migrate_local_to_shared() {
    for pt in [PromptType::Agent, PromptType::Skill, PromptType::Command] {
        let local = legacy_local_dir(pt);
        let Ok(entries) = fs::read_dir(&local) else { continue };
        let shared = dir_for(pt);
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|s| s.to_str()) else { continue };
            migrate_one_file(&path, &shared, id);
        }
    }
}

/// Move one local behaviour file into `shared`, applying the byte-compare
/// collision rule. Best-effort: any I/O error leaves the local file in place
/// (a later boot retries).
fn migrate_one_file(local_path: &Path, shared: &Path, id: &str) {
    let Ok(local_bytes) = fs::read(local_path) else { return };
    let target = shared.join(format!("{id}.md"));

    let dest = if target.exists() {
        match fs::read(&target) {
            // Same file already shared — just drop the local copy.
            Ok(existing) if existing == local_bytes => {
                drop(fs::remove_file(local_path));
                return;
            }
            // Differing collision — the incoming local file gets the suffix.
            _ => next_suffixed_path(shared, id),
        }
    } else {
        target
    };

    if fs::create_dir_all(shared).is_err() {
        return;
    }
    if fs::write(&dest, &local_bytes).is_ok() {
        drop(fs::remove_file(local_path));
    }
}

/// The lowest-free `<id>-<n>.md` path in `shared` (n starts at 1). Bounded to
/// `u32::MAX` candidates; the fallback is unreachable in practice.
fn next_suffixed_path(shared: &Path, id: &str) -> PathBuf {
    (1u32..=u32::MAX)
        .map(|n| shared.join(format!("{id}-{n}.md")))
        .find(|p| !p.exists())
        .unwrap_or_else(|| shared.join(format!("{id}-x.md")))
}

/// Parse a prompt .md file with YAML frontmatter.
/// Format:
/// ```text
/// ---
/// name: My Prompt
/// description: Short description
/// ---
/// Body content here...
/// `
/// Returns (name, description, body).
pub(crate) fn parse_prompt_file(content: &str) -> (String, String, String) {
    #[derive(serde::Deserialize, Default)]
    struct Frontmatter {
        #[serde(default)]
        name: String,
        #[serde(default)]
        description: String,
    }

    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        // No frontmatter — treat entire content as body
        return (String::new(), String::new(), content.to_owned());
    }

    // Find the closing ---
    let after_first = trimmed.get(3..).unwrap_or("");
    let Some(end) = after_first.find("\n---") else {
        // No closing --- found, treat as plain content
        return (String::new(), String::new(), content.to_owned());
    };

    let yaml_block = after_first.get(..end).unwrap_or("");
    let body_start = end.saturating_add(4); // skip \n---
    let body = after_first.get(body_start..).unwrap_or("").trim_start_matches('\n').to_owned();

    let fm: Frontmatter = serde_yaml::from_str(yaml_block).unwrap_or_default();
    (fm.name, fm.description, body)
}

/// Format a prompt item back to .md file with YAML frontmatter
pub(crate) fn format_prompt_file(name: &str, description: &str, content: &str) -> String {
    format!("---\nname: {name}\ndescription: {description}\n---\n{content}")
}

/// Load all .md prompt files from a directory
pub(crate) fn load_prompts_from_dir(dir: &Path, prompt_type: PromptType) -> Vec<PromptItem> {
    let mut items = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else { return items };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }

        let id = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_owned();

        if id.is_empty() {
            continue;
        }

        if let Ok(content) = fs::read_to_string(&path) {
            let (name, description, body) = parse_prompt_file(&content);
            items.push(PromptItem {
                id,
                name,
                description,
                content: body,
                prompt_type,
                is_builtin: false, // disk files are user-created; caller merges with built-ins
            });
        }
    }

    items
}

/// Generate a URL-safe slug from a name (e.g., "Code Reviewer" → "code-reviewer")
pub(crate) fn slugify(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Per-type prompt lists shared by every reader, plus the stat fingerprint of
/// the behaviour dirs they were built from.
struct PromptIndex {
    /// Fingerprint of the three behaviour dirs at build time (see [`dirs_fingerprint`]).
    fingerprint: u64,
    /// Agent, skill, command lists, indexed by [`type_slot`]; `None` = not built yet.
    lists: [Option<Arc<[PromptItem]>>; 3],
}

/// In-memory prompt index. Frame code (status bar, sidebar, threads view) reads
/// it on every render, so it must never touch the disk: it is invalidated by
/// [`revalidate_index`] (stat-only, on panel refresh) and [`invalidate_index`]
/// (after our own writes).
static INDEX: Mutex<PromptIndex> = Mutex::new(PromptIndex { fingerprint: 0, lists: [None, None, None] });

/// Slot of a prompt type in [`PromptIndex::lists`].
const fn type_slot(pt: PromptType) -> usize {
    match pt {
        PromptType::Agent => 0,
        PromptType::Skill => 1,
        PromptType::Command => 2,
    }
}

/// Order-independent stat fingerprint (name, size, mtime) of every file in the
/// three behaviour dirs. No file contents are read.
#[must_use]
pub fn dirs_fingerprint() -> u64 {
    use std::hash::{Hash as _, Hasher as _};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for pt in [PromptType::Agent, PromptType::Skill, PromptType::Command] {
        // Sum of per-file hashes: `read_dir` order is unspecified.
        let files = fs::read_dir(dir_for(pt)).map_or(0u64, |rd| {
            rd.flatten().fold(0u64, |acc, e| {
                let mut fh = std::collections::hash_map::DefaultHasher::new();
                e.file_name().hash(&mut fh);
                e.metadata().ok().map(|m| (m.len(), m.modified().ok())).hash(&mut fh);
                acc.wrapping_add(fh.finish())
            })
        });
        files.hash(&mut h);
    }
    h.finish()
}

/// Drop the cached lists when the behaviour dirs changed on disk since they were
/// built (external edit, another agent of the fleet). Returns the fingerprint.
pub fn revalidate_index() -> u64 {
    let fp = dirs_fingerprint();
    let mut idx = INDEX.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if idx.fingerprint != fp {
        idx.fingerprint = fp;
        idx.lists = [None, None, None];
    }
    fp
}

/// Drop the cached lists after we wrote a behaviour file ourselves.
pub fn invalidate_index() {
    let mut idx = INDEX.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    idx.lists = [None, None, None];
}

/// All prompts of a type (disk + built-ins merged), served from the in-memory
/// index. Disk is read only on the first call after an invalidation.
#[must_use]
pub fn load_prompts_for(pt: PromptType) -> Arc<[PromptItem]> {
    let slot = type_slot(pt);
    let cached = INDEX.lock().unwrap_or_else(std::sync::PoisonError::into_inner).lists.get(slot).cloned();
    if let Some(list) = cached.flatten() {
        return list;
    }
    let list: Arc<[PromptItem]> = Arc::from(read_prompts_for(pt));
    if let Some(entry) = INDEX.lock().unwrap_or_else(std::sync::PoisonError::into_inner).lists.get_mut(slot) {
        *entry = Some(Arc::clone(&list));
    }
    list
}

/// Load all prompts for a single type from disk, merged with built-ins
/// (disk wins on id).
fn read_prompts_for(pt: PromptType) -> Vec<PromptItem> {
    use cp_base::config::accessors::library;

    let mut items = load_prompts_from_dir(&dir_for(pt), pt);

    let builtins = match pt {
        PromptType::Agent => library::agents(),
        PromptType::Skill => library::skills(),
        PromptType::Command => library::commands(),
    };

    for builtin in builtins {
        if items.iter().any(|i| i.id == builtin.id) {
            if let Some(i) = items.iter_mut().find(|i| i.id == builtin.id) {
                i.is_builtin = true;
            }
        } else {
            items.push(PromptItem {
                id: builtin.id.clone(),
                name: builtin.name.clone(),
                description: builtin.description.clone(),
                content: builtin.content.clone(),
                prompt_type: pt,
                is_builtin: true,
            });
        }
    }

    items
}

/// Validate that content has correct `.md` frontmatter structure.
///
/// # Errors
///
/// Returns `Err(reason)` if the frontmatter is missing, malformed, or lacks a `name` field.
pub fn validate_frontmatter(content: &str) -> Result<(), String> {
    #[derive(serde::Deserialize)]
    struct Fm {
        name: Option<String>,
    }

    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return Err("File must start with YAML frontmatter (---)".to_owned());
    }
    let after_first = trimmed.get(3..).unwrap_or("");
    let Some(end) = after_first.find("\n---") else {
        return Err("Missing closing frontmatter delimiter (---)".to_owned());
    };
    let yaml_block = after_first.get(..end).unwrap_or("");

    let fm: Fm = serde_yaml::from_str(yaml_block).map_err(|e| format!("Invalid YAML frontmatter: {e}"))?;
    if fm.name.as_deref().unwrap_or("").is_empty() {
        return Err("Frontmatter must include a non-empty 'name' field".to_owned());
    }
    Ok(())
}
