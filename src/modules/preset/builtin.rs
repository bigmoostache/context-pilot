use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::constants::{PRESETS_DIR, STORE_DIR};

use super::types::{Preset, PresetWorkerState};

/// YAML schema for presets.yaml
#[derive(Deserialize)]
struct PresetsYaml {
    presets: Vec<PresetYamlEntry>,
}

#[derive(Deserialize)]
struct PresetYamlEntry {
    name: String,
    description: String,
    system_prompt: Option<String>,
    active_modules: Vec<String>,
    #[serde(default)]
    disabled_tools: Vec<String>,
}

/// Ensure all built-in presets exist on disk. Creates missing ones.
pub fn ensure_builtin_presets() {
    let dir = Path::new(STORE_DIR).join(PRESETS_DIR);
    if let Err(e) = fs::create_dir_all(&dir) {
        eprintln!("Failed to create presets directory: {}", e);
        return;
    }

    for preset in builtin_preset_definitions() {
        let path = dir.join(format!("{}.json", preset.preset_name));
        if !path.exists()
            && let Ok(json) = serde_json::to_string_pretty(&preset)
        {
            let _ = fs::write(&path, json);
        }
    }
}

fn builtin_preset_definitions() -> Vec<Preset> {
    let yaml_str = include_str!("../../../yamls/presets.yaml");
    let yaml: PresetsYaml = match serde_yaml::from_str(yaml_str) {
        Ok(y) => y,
        Err(e) => {
            eprintln!("Failed to parse yamls/presets.yaml: {}", e);
            return vec![];
        }
    };

    yaml.presets
        .into_iter()
        .map(|entry| Preset {
            preset_name: entry.name,
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
