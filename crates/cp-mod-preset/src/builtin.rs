use std::collections::HashMap;
use std::fs;
use std::io::Write as _;
use std::path::Path;

use serde::Deserialize;

use crate::PRESETS_DIR;
use crate::types::{Preset, PresetWorkerState};
use cp_base::config::constants;

/// YAML schema for presets.yaml
#[derive(Deserialize)]
struct PresetsYaml {
    /// List of preset entries parsed from YAML.
    presets: Vec<PresetYamlEntry>,
}

/// A single preset entry as defined in presets.yaml.
#[derive(Deserialize)]
struct PresetYamlEntry {
    /// Preset identifier (alphanumeric + hyphens).
    name: String,
    /// Human-readable description.
    description: String,
    /// Optional system prompt / agent ID to activate.
    system_prompt: Option<String>,
    /// Module IDs to enable for this preset.
    active_modules: Vec<String>,
    /// Tool IDs to disable when this preset is loaded.
    #[serde(default)]
    disabled_tools: Vec<String>,
}

/// Ensure all built-in presets exist on disk. Creates missing ones.
pub fn ensure_builtin_presets() {
    let dir = Path::new(constants::STORE_DIR).join(PRESETS_DIR);
    if let Err(e) = fs::create_dir_all(&dir) {
        drop(writeln!(std::io::stderr(), "Failed to create presets directory: {e}"));
        return;
    }

    for preset in builtin_preset_definitions() {
        let path = dir.join(format!("{}.json", preset.name));
        if !path.exists()
            && let Ok(json) = serde_json::to_string_pretty(&preset)
        {
            let _r = fs::write(&path, json);
        }
    }
}

/// Parse built-in preset definitions from the embedded presets.yaml file.
fn builtin_preset_definitions() -> Vec<Preset> {
    let yaml_str = include_str!("../../../yamls/presets.yaml");
    let yaml: PresetsYaml = match serde_yaml::from_str(yaml_str) {
        Ok(y) => y,
        Err(e) => {
            drop(writeln!(std::io::stderr(), "Failed to parse yamls/presets.yaml: {e}"));
            return vec![];
        }
    };

    yaml.presets
        .into_iter()
        .map(|entry| Preset {
            name: entry.name,
            description: entry.description,
            built_in: true,
            worker_state: PresetWorkerState {
                active_agent_id: entry.system_prompt,
                active_modules: entry.active_modules,
                disabled_tools: entry.disabled_tools,
                loaded_skill_ids: vec![],
                modules: HashMap::new(),
                dynamic_panels: vec![],
            },
        })
        .collect()
}
