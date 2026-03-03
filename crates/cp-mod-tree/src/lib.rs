mod panel;
mod tools;
pub mod types;

use serde_json::json;

use cp_base::modules::ToolVisualizer;
use cp_base::panels::Panel;
use cp_base::state::{ContextType, State};
use cp_base::tools::{ParamType, ToolDefinition, ToolParam};
use cp_base::tools::{ToolResult, ToolUse};

use self::panel::TreePanel;
use cp_base::modules::Module;

pub use types::{DEFAULT_TREE_FILTER, TreeFileDescription, TreeState};

// Re-export directory listing for autocomplete
pub use self::tools::list_dir_entries;

pub struct TreeModule;

impl Module for TreeModule {
    fn id(&self) -> &'static str {
        "tree"
    }
    fn name(&self) -> &'static str {
        "Tree"
    }
    fn description(&self) -> &'static str {
        "Directory tree view with filtering and descriptions"
    }
    fn is_global(&self) -> bool {
        true
    }

    fn init_state(&self, state: &mut State) {
        state.set_ext(TreeState::new());
        state.set_ext(cp_base::autocomplete::AutocompleteState::new());
    }

    fn reset_state(&self, state: &mut State) {
        state.set_ext(TreeState::new());
        state.set_ext(cp_base::autocomplete::AutocompleteState::new());
    }

    fn save_module_data(&self, state: &State) -> serde_json::Value {
        let ts = TreeState::get(state);
        json!({
            "tree_filter": ts.tree_filter,
            "tree_descriptions": ts.tree_descriptions,
        })
    }

    fn load_module_data(&self, data: &serde_json::Value, state: &mut State) {
        if let Some(v) = data.get("tree_filter").and_then(|v| v.as_str()) {
            TreeState::get_mut(state).tree_filter = v.to_string();
        }
        if let Some(arr) = data.get("tree_descriptions")
            && let Ok(v) = serde_json::from_value(arr.clone())
        {
            TreeState::get_mut(state).tree_descriptions = v;
        }
        // Legacy: load tree_open_folders from global config if present (migration)
        if let Some(arr) = data.get("tree_open_folders")
            && let Ok(v) = serde_json::from_value::<Vec<String>>(arr.clone())
        {
            let ts = TreeState::get_mut(state);
            ts.tree_open_folders = v;
            if !ts.tree_open_folders.contains(&".".to_string()) {
                ts.tree_open_folders.insert(0, ".".to_string());
            }
        }
    }

    fn save_worker_data(&self, state: &State) -> serde_json::Value {
        json!({
            "tree_open_folders": TreeState::get(state).tree_open_folders,
        })
    }

    fn load_worker_data(&self, data: &serde_json::Value, state: &mut State) {
        if let Some(arr) = data.get("tree_open_folders")
            && let Ok(v) = serde_json::from_value::<Vec<String>>(arr.clone())
        {
            let ts = TreeState::get_mut(state);
            ts.tree_open_folders = v;
            // Ensure root is always open
            if !ts.tree_open_folders.contains(&".".to_string()) {
                ts.tree_open_folders.insert(0, ".".to_string());
            }
        }
    }

    fn fixed_panel_types(&self) -> Vec<ContextType> {
        vec![ContextType::new(ContextType::TREE)]
    }

    fn fixed_panel_defaults(&self) -> Vec<(ContextType, &'static str, bool)> {
        vec![(ContextType::new(ContextType::TREE), "Tree", true)]
    }

    fn create_panel(&self, context_type: &ContextType) -> Option<Box<dyn Panel>> {
        match context_type.as_str() {
            ContextType::TREE => Some(Box::new(TreePanel)),
            _ => None,
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                id: "tree_filter".to_string(),
                name: "Tree Filter".to_string(),
                short_desc: "Configure directory filter".to_string(),
                description: "Edits the gitignore-style filter for the directory tree view.".to_string(),
                params: vec![
                    ToolParam::new("filter", ParamType::String)
                        .desc("Gitignore-style patterns, one per line")
                        .required(),
                ],
                enabled: true,
                reverie_allowed: true,
                category: "Tree".to_string(),
            },
            ToolDefinition {
                id: "tree_toggle".to_string(),
                name: "Tree Toggle".to_string(),
                short_desc: "Open/close folders".to_string(),
                description: "Opens or closes folders in the directory tree view. Closed folders show child count, open folders show contents.".to_string(),
                params: vec![
                    ToolParam::new("paths", ParamType::Array(Box::new(ParamType::String)))
                        .desc("Folder paths to toggle (e.g., ['src', 'src/ui'])")
                        .required(),
                    ToolParam::new("action", ParamType::String)
                        .desc("Action to perform")
                        .enum_vals(&["open", "close", "toggle"])
                        .default_val("toggle"),
                ],
                enabled: true,
                reverie_allowed: true,
                category: "Tree".to_string(),
            },
            ToolDefinition {
                id: "tree_describe".to_string(),
                name: "Tree Describe".to_string(),
                short_desc: "Add file/folder descriptions".to_string(),
                description: "Adds or updates descriptions for files and folders in the tree. Descriptions appear next to items. A [!] marker indicates the file changed since description was written.".to_string(),
                params: vec![
                    ToolParam::new("descriptions", ParamType::Array(Box::new(ParamType::Object(vec![
                        ToolParam::new("path", ParamType::String)
                            .desc("File or folder path")
                            .required(),
                        ToolParam::new("description", ParamType::String)
                            .desc("Description text"),
                        ToolParam::new("delete", ParamType::Boolean)
                            .desc("Set true to remove description"),
                    ]))))
                        .desc("Array of path descriptions")
                        .required(),
                ],
                enabled: true,
                reverie_allowed: true,
                category: "Tree".to_string(),
            },
        ]
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            "tree_filter" => Some(self::tools::execute_edit_filter(tool, state)),
            "tree_toggle" => Some(self::tools::execute_toggle_folders(tool, state)),
            "tree_describe" => Some(self::tools::execute_describe_files(tool, state)),
            _ => None,
        }
    }

    fn tool_visualizers(&self) -> Vec<(&'static str, ToolVisualizer)> {
        vec![
            ("tree_filter", visualize_tree_output as ToolVisualizer),
            ("tree_toggle", visualize_tree_output as ToolVisualizer),
            ("tree_describe", visualize_tree_output as ToolVisualizer),
        ]
    }

    fn context_type_metadata(&self) -> Vec<cp_base::state::ContextTypeMeta> {
        vec![cp_base::state::ContextTypeMeta {
            context_type: "tree",
            icon_id: "tree",
            is_fixed: true,
            needs_cache: true,
            fixed_order: Some(3),
            display_name: "tree",
            short_name: "tree",
            needs_async_wait: false,
        }]
    }

    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![("Tree", "Navigate and annotate the directory structure")]
    }

    fn watch_paths(&self, state: &State) -> Vec<cp_base::panels::WatchSpec> {
        TreeState::get(state).tree_open_folders.iter().map(|f| cp_base::panels::WatchSpec::Dir(f.clone())).collect()
    }

    fn should_invalidate_on_fs_change(
        &self,
        ctx: &cp_base::state::ContextElement,
        _changed_path: &str,
        is_dir_event: bool,
    ) -> bool {
        is_dir_event && ctx.context_type.as_str() == ContextType::TREE
    }
}

/// Visualizer for tree tool results.
/// Shows tree operations with colored indicators and highlights changed descriptions.
fn visualize_tree_output(content: &str, width: usize) -> Vec<ratatui::text::Line<'static>> {
    use ratatui::prelude::*;

    let success_color = Color::Rgb(80, 250, 123);
    let info_color = Color::Rgb(139, 233, 253);
    let warning_color = Color::Rgb(241, 250, 140);
    let error_color = Color::Rgb(255, 85, 85);

    let mut lines = Vec::new();

    for line in content.lines() {
        if line.is_empty() {
            lines.push(Line::from(""));
            continue;
        }

        let style = if line.starts_with("Error:") {
            Style::default().fg(error_color)
        } else if line.starts_with("Updated") || line.starts_with("Added") {
            Style::default().fg(success_color)
        } else if line.starts_with("Opened") || line.contains("folder") {
            Style::default().fg(info_color)
        } else if line.starts_with("Closed") {
            Style::default().fg(warning_color)
        } else if line.contains("[!]") || line.contains("Modified") {
            // Change indicator
            Style::default().fg(warning_color)
        } else {
            Style::default()
        };

        let display = if line.len() > width {
            format!("{}...", &line[..line.floor_char_boundary(width.saturating_sub(3))])
        } else {
            line.to_string()
        };
        lines.push(Line::from(Span::styled(display, style)));
    }

    lines
}
