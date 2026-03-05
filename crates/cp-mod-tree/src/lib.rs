//! Tree module — directory tree visualization with filtering and descriptions.
//!
//! Three tools: `tree_filter` (gitignore-style patterns), `tree_toggle`
//! (open/close folders), `tree_describe` (annotate files/folders). The tree
//! panel auto-refreshes on filesystem changes and provides @-autocomplete
//! with directory entries.

/// Panel implementation for the directory tree view.
mod panel;
/// Tool implementations for tree filtering, toggling, and describing.
mod tools;
/// Tree state types: `TreeState`, `TreeFileDescription`.
pub mod types;

use types::{TreeFileDescription, TreeState};

use serde_json::json;

use cp_base::modules::ToolVisualizer;
use cp_base::panels::Panel;
use cp_base::state::context::ContextType;
use cp_base::state::runtime::State;
use cp_base::tools::pre_flight::PreFlightResult;
use cp_base::tools::{ParamType, ToolDefinition, ToolParam, ToolTexts};
use cp_base::tools::{ToolResult, ToolUse};

use self::panel::TreePanel;
use cp_base::modules::Module;

// Re-export directory listing for autocomplete

/// Lazily parsed tool definitions loaded from the YAML spec.
static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> =
    std::sync::LazyLock::new(|| ToolTexts::parse(include_str!("../../../yamls/tools/tree.yaml")));

/// Tree module: directory tree view with filtering, descriptions, and auto-refresh.
#[derive(Debug, Clone, Copy)]
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
        state.set_ext(cp_base::state::autocomplete::AutocompleteState::new());
    }

    fn reset_state(&self, state: &mut State) {
        state.set_ext(TreeState::new());
        state.set_ext(cp_base::state::autocomplete::AutocompleteState::new());
    }

    fn save_module_data(&self, state: &State) -> serde_json::Value {
        let ts = TreeState::get(state);
        json!({
            "tree_filter": ts.filter,
            "tree_descriptions": ts.descriptions,
        })
    }

    fn load_module_data(&self, data: &serde_json::Value, state: &mut State) {
        if let Some(v) = data.get("tree_filter").and_then(|v| v.as_str()) {
            TreeState::get_mut(state).filter = v.to_string();
        }
        if let Some(arr) = data.get("tree_descriptions")
            && let Ok(v) = serde_json::from_value(arr.clone())
        {
            TreeState::get_mut(state).descriptions = v;
        }
        // Legacy: load tree_open_folders from global config if present (migration)
        if let Some(arr) = data.get("tree_open_folders")
            && let Ok(v) = serde_json::from_value::<Vec<String>>(arr.clone())
        {
            let ts = TreeState::get_mut(state);
            ts.open_folders = v;
            if !ts.open_folders.contains(&".".to_string()) {
                ts.open_folders.insert(0, ".".to_string());
            }
        }
    }

    fn save_worker_data(&self, state: &State) -> serde_json::Value {
        json!({
            "tree_open_folders": TreeState::get(state).open_folders,
        })
    }

    fn load_worker_data(&self, data: &serde_json::Value, state: &mut State) {
        if let Some(arr) = data.get("tree_open_folders")
            && let Ok(v) = serde_json::from_value::<Vec<String>>(arr.clone())
        {
            let ts = TreeState::get_mut(state);
            ts.open_folders = v;
            // Ensure root is always open
            if !ts.open_folders.contains(&".".to_string()) {
                ts.open_folders.insert(0, ".".to_string());
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
        let t = &*TOOL_TEXTS;
        vec![
            ToolDefinition::from_yaml("tree_filter", t)
                .short_desc("Configure directory filter")
                .category("Tree")
                .reverie_allowed(true)
                .param("filter", ParamType::String, true)
                .build(),
            ToolDefinition::from_yaml("tree_toggle", t)
                .short_desc("Open/close folders")
                .category("Tree")
                .reverie_allowed(true)
                .param_array("paths", ParamType::String, true)
                .param_enum("action", &["open", "close", "toggle"], false)
                .build(),
            ToolDefinition::from_yaml("tree_describe", t)
                .short_desc("Add file/folder descriptions")
                .category("Tree")
                .reverie_allowed(true)
                .param_array(
                    "descriptions",
                    ParamType::Object(vec![
                        ToolParam::new("path", ParamType::String).desc("File or folder path").required(),
                        ToolParam::new("description", ParamType::String).desc("Description text"),
                        ToolParam::new("delete", ParamType::Boolean).desc("Set true to remove description"),
                    ]),
                    true,
                )
                .build(),
        ]
    }

    fn pre_flight(&self, tool: &ToolUse, _state: &State) -> Option<PreFlightResult> {
        match tool.name.as_str() {
            "tree_toggle" => {
                let mut pf = PreFlightResult::new();
                if let Some(paths) = tool.input.get("paths").and_then(|v| v.as_array()) {
                    for path_val in paths {
                        if let Some(path) = path_val.as_str() {
                            let p = std::path::Path::new(path);
                            if !p.exists() {
                                pf.warnings.push(format!("Path '{path}' does not exist"));
                            } else if !p.is_dir() {
                                pf.warnings.push(format!("'{path}' is not a directory"));
                            }
                        }
                    }
                }
                Some(pf)
            }
            _ => None,
        }
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            "tree_filter" => Some(tools::execute_edit_filter(tool, state)),
            "tree_toggle" => Some(tools::execute_toggle_folders(tool, state)),
            "tree_describe" => Some(tools::execute_describe_files(tool, state)),
            _ => None,
        }
    }

    fn tool_visualizers(&self) -> Vec<(&'static str, ToolVisualizer)> {
        vec![
            ("tree_filter", visualize_tree_output),
            ("tree_toggle", visualize_tree_output),
            ("tree_describe", visualize_tree_output),
        ]
    }

    fn context_type_metadata(&self) -> Vec<cp_base::state::context::ContextTypeMeta> {
        vec![cp_base::state::context::ContextTypeMeta {
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
        TreeState::get(state).open_folders.iter().map(|f| cp_base::panels::WatchSpec::Dir(f.clone())).collect()
    }

    fn should_invalidate_on_fs_change(
        &self,
        ctx: &cp_base::state::context::ContextElement,
        _changed_path: &str,
        is_dir_event: bool,
    ) -> bool {
        is_dir_event && ctx.context_type.as_str() == ContextType::TREE
    }

    fn dependencies(&self) -> &[&'static str] {
        &[]
    }
    fn is_core(&self) -> bool {
        false
    }
    fn dynamic_panel_types(&self) -> Vec<ContextType> {
        vec![]
    }
    fn context_display_name(&self, _context_type: &str) -> Option<&'static str> {
        None
    }
    fn context_detail(&self, _ctx: &cp_base::state::context::ContextElement) -> Option<String> {
        None
    }
    fn overview_context_section(&self, _state: &State) -> Option<String> {
        None
    }
    fn overview_render_sections(
        &self,
        _state: &State,
        _base_style: ratatui::prelude::Style,
    ) -> Vec<(u8, Vec<ratatui::text::Line<'static>>)> {
        vec![]
    }
    fn on_close_context(
        &self,
        _ctx: &cp_base::state::context::ContextElement,
        _state: &mut State,
    ) -> Option<Result<String, String>> {
        None
    }
    fn on_user_message(&self, _state: &mut State) {}
    fn on_stream_stop(&self, _state: &mut State) {}
    fn watcher_immediate_refresh(&self) -> bool {
        true
    }
}

/// Visualizer for tree tool results.
/// Shows tree operations with colored indicators and highlights changed descriptions.
fn visualize_tree_output(content: &str, width: usize) -> Vec<ratatui::text::Line<'static>> {
    use ratatui::prelude::{Color, Line, Span, Style};

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
            format!("{}...", &line.get(..line.floor_char_boundary(width.saturating_sub(3))).unwrap_or(""))
        } else {
            line.to_string()
        };
        lines.push(Line::from(Span::styled(display, style)));
    }

    lines
}
