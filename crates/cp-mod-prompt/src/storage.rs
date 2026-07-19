use std::fs;
use std::path::{Path, PathBuf};

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

/// Full path to the directory for a prompt type
#[must_use]
pub fn dir_for(pt: PromptType) -> PathBuf {
    PathBuf::from(constants::STORE_DIR).join(subdir_for(pt))
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

/// Load all prompts for a single type (from disk + built-ins merged).
/// Re-reads from disk every call — no caching.
#[must_use]
pub fn load_prompts_for(pt: PromptType) -> Vec<PromptItem> {
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
