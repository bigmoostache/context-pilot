use std::fs;
use std::path::{Path, PathBuf};

use cp_base::constants::STORE_DIR;

use crate::types::{PromptItem, PromptType};

/// Subdirectory names under .context-pilot/ for each prompt type
fn subdir_for(pt: PromptType) -> &'static str {
    match pt {
        PromptType::Agent => "agents",
        PromptType::Skill => "skills",
        PromptType::Command => "commands",
    }
}

/// Full path to the directory for a prompt type
pub fn dir_for(pt: PromptType) -> PathBuf {
    PathBuf::from(STORE_DIR).join(subdir_for(pt))
}

/// Parse a prompt .md file with YAML frontmatter.
/// Format:
/// ```text
/// ---
/// name: My Prompt
/// description: Short description
/// ---
/// Body content here...
/// ```
/// Returns (name, description, body).
pub fn parse_prompt_file(content: &str) -> (String, String, String) {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        // No frontmatter — treat entire content as body
        return (String::new(), String::new(), content.to_string());
    }

    // Find the closing ---
    let after_first = &trimmed[3..];
    if let Some(end) = after_first.find("\n---") {
        let yaml_block = &after_first[..end];
        let body_start = end + 4; // skip \n---
        let body = after_first[body_start..].trim_start_matches('\n').to_string();

        // Parse YAML frontmatter
        #[derive(serde::Deserialize, Default)]
        struct Frontmatter {
            #[serde(default)]
            name: String,
            #[serde(default)]
            description: String,
        }

        let fm: Frontmatter = serde_yaml::from_str(yaml_block).unwrap_or_default();
        (fm.name, fm.description, body)
    } else {
        // No closing --- found, treat as plain content
        (String::new(), String::new(), content.to_string())
    }
}

/// Format a prompt item back to .md file with YAML frontmatter
pub fn format_prompt_file(name: &str, description: &str, content: &str) -> String {
    format!("---\nname: {}\ndescription: {}\n---\n{}", name, description, content)
}

/// Load all .md prompt files from a directory
pub fn load_prompts_from_dir(dir: &Path, prompt_type: PromptType) -> Vec<PromptItem> {
    let mut items = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return items,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }

        let id = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();

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

/// Save a prompt item to its directory as {id}.md
pub fn save_prompt_to_dir(dir: &Path, item: &PromptItem) {
    fs::create_dir_all(dir).ok();
    let path = dir.join(format!("{}.md", item.id));
    let content = format_prompt_file(&item.name, &item.description, &item.content);
    fs::write(path, content).ok();
}

/// Delete a prompt file from its directory
pub fn delete_prompt_from_dir(dir: &Path, id: &str) {
    let path = dir.join(format!("{}.md", id));
    if path.exists() {
        fs::remove_file(path).ok();
    }
}

/// Load all prompts from all three directories + built-ins from library.yaml.
/// Returns (agents, skills, commands).
pub fn load_all_prompts() -> (Vec<PromptItem>, Vec<PromptItem>, Vec<PromptItem>) {
    use cp_base::constants::library;

    let mut agents = load_prompts_from_dir(&dir_for(PromptType::Agent), PromptType::Agent);
    let mut skills = load_prompts_from_dir(&dir_for(PromptType::Skill), PromptType::Skill);
    let mut commands = load_prompts_from_dir(&dir_for(PromptType::Command), PromptType::Command);

    // Merge built-in agents (from library.yaml) — don't duplicate if user has same ID on disk
    for builtin in library::agents() {
        if !agents.iter().any(|a| a.id == builtin.id) {
            agents.push(PromptItem {
                id: builtin.id.clone(),
                name: builtin.name.clone(),
                description: builtin.description.clone(),
                content: builtin.content.clone(),
                prompt_type: PromptType::Agent,
                is_builtin: true,
            });
        } else {
            // Mark disk version as builtin
            if let Some(a) = agents.iter_mut().find(|a| a.id == builtin.id) {
                a.is_builtin = true;
            }
        }
    }

    // Merge built-in skills
    for builtin in library::skills() {
        if !skills.iter().any(|s| s.id == builtin.id) {
            skills.push(PromptItem {
                id: builtin.id.clone(),
                name: builtin.name.clone(),
                description: builtin.description.clone(),
                content: builtin.content.clone(),
                prompt_type: PromptType::Skill,
                is_builtin: true,
            });
        } else if let Some(s) = skills.iter_mut().find(|s| s.id == builtin.id) {
            s.is_builtin = true;
        }
    }

    // Merge built-in commands
    for builtin in library::commands() {
        if !commands.iter().any(|c| c.id == builtin.id) {
            commands.push(PromptItem {
                id: builtin.id.clone(),
                name: builtin.name.clone(),
                description: builtin.description.clone(),
                content: builtin.content.clone(),
                prompt_type: PromptType::Command,
                is_builtin: true,
            });
        } else if let Some(c) = commands.iter_mut().find(|c| c.id == builtin.id) {
            c.is_builtin = true;
        }
    }

    (agents, skills, commands)
}

/// Generate a URL-safe slug from a name (e.g., "Code Reviewer" → "code-reviewer")
pub fn slugify(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}
