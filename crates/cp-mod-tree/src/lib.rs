//! Tree module — directory tree visualization with filtering and descriptions.
//!
//! Three tools: `tree_filter` (gitignore-style patterns), `tree_toggle`
//! (open/close folders), `tree_describe` (annotate files/folders). The tree
//! panel auto-refreshes on filesystem changes and provides @-autocomplete
//! with directory entries.

/// Panel implementation for the directory tree view.
mod panel;
/// Read-only tree-string rendering (directory walk), split from tools.rs.
mod render;
/// YAML-backed persistent storage for tree descriptions.
mod storage;
/// Tool implementations for tree filtering, toggling, and describing.
pub mod tools;
/// Tree state types: `TreeState`, `TreeFileDescription`.
pub mod types;

use types::TreeState;

use serde_json::json;

use cp_base::modules::ToolVisualizer;
use cp_base::panels::Panel;
use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::pre_flight::Verdict;
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
#[non_exhaustive]
pub struct TreeModule;

impl Default for TreeModule {
    fn default() -> Self {
        Self::new()
    }
}

impl TreeModule {
    /// Construct the module marker (funnels cross-crate construction of this
    /// `non_exhaustive` unit struct through an associated fn).
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

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
        state.set_ext(cp_base::state::autocomplete::Suggestions::new());
    }

    fn reset_state(&self, state: &mut State) {
        state.set_ext(TreeState::new());
        state.set_ext(cp_base::state::autocomplete::Suggestions::new());
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
            v.clone_into(&mut TreeState::get_mut(state).filter);
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
            if !ts.open_folders.contains(&".".to_owned()) {
                ts.open_folders.insert(0, ".".to_owned());
            }
        }
        // YAML backing store: migrate existing descriptions, then populate gaps
        let ts = TreeState::get_mut(state);
        storage::migrate_to_yaml(&ts.descriptions);
        storage::populate_from_yaml(&mut ts.descriptions);
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
            if !ts.open_folders.contains(&".".to_owned()) {
                ts.open_folders.insert(0, ".".to_owned());
            }
        }
    }

    fn fixed_panel_types(&self) -> Vec<Kind> {
        vec![Kind::new(Kind::TREE)]
    }

    fn fixed_panel_defaults(&self) -> Vec<(Kind, &'static str, bool)> {
        vec![(Kind::new(Kind::TREE), "Tree", true)]
    }

    fn create_panel(&self, context_type: &Kind) -> Option<Box<dyn Panel>> {
        match context_type.as_str() {
            Kind::TREE => Some(Box::new(TreePanel)),
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
                        ToolParam::new("close_panel", ParamType::Boolean).desc(
                            "Auto-close the file's open panel after describing it (default: true). Set false to keep viewing the file.",
                        ),
                    ]),
                    true,
                )
                .build(),
        ]
    }

    fn pre_flight(&self, tool: &ToolUse, _state: &State) -> Option<Verdict> {
        match tool.name.as_str() {
            "tree_toggle" => {
                let mut pf = Verdict::new();
                if let Some(paths) = tool.input.get("paths").and_then(|v| v.as_array()) {
                    for path_val in paths {
                        if let Some(path) = path_val.as_str() {
                            let p = std::path::Path::new(path);
                            if !p.exists() {
                                pf.warnings.push(format!("Path '{path}' does not exist"));
                            } else if !p.is_dir() {
                                pf.warnings.push(format!("'{path}' is not a directory"));
                            } else {
                                // Existing directory — valid, no warning.
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

    fn context_type_metadata(&self) -> Vec<cp_base::state::context::TypeMeta> {
        vec![cp_base::state::context::TypeMeta {
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
        ctx: &cp_base::state::context::Entry,
        changed_path: &str,
        is_dir_event: bool,
    ) -> bool {
        // A change under a high-churn control directory must NOT rebuild the
        // tree. The watcher subscribes to every *open* folder recursively, so
        // when the realm root is open it sees every write to the agent's own
        // bookkeeping dirs — `.context-pilot/` (config.json atomic save =
        // tmp-create + rename, fired on every bridge command + periodically),
        // `oplog/` (an append/roll per phase/cost/message delta when the bridge
        // is ON), `.git/`, `target/`, `.uploads/`. Left ungated, each of those
        // events triggered a full `generate_tree_string` → `build_tree_new` +
        // per-file `compute_file_hash` re-walk of the entire realm, pinning a
        // core while the agent was otherwise idle (T309). These dirs are pure
        // noise for a navigation tree — a coding agent never browses them and a
        // slightly-stale child count is harmless — so an event under any of
        // them is dropped before it can invalidate the panel.
        is_dir_event && ctx.context_type.as_str() == Kind::TREE && !path_under_control_dir(changed_path)
    }

    fn dependencies(&self) -> &[&'static str] {
        &[]
    }
    fn is_core(&self) -> bool {
        false
    }
    fn dynamic_panel_types(&self) -> Vec<Kind> {
        vec![]
    }
    fn context_display_name(&self, _context_type: &str) -> Option<&'static str> {
        None
    }
    fn context_detail(&self, _ctx: &cp_base::state::context::Entry) -> Option<String> {
        None
    }
    fn overview_context_section(&self, _state: &State) -> Option<String> {
        None
    }
    fn overview_render_sections(&self, _state: &State) -> Vec<(u8, Vec<cp_render::Block>)> {
        vec![]
    }
    fn on_close_context(
        &self,
        _ctx: &cp_base::state::context::Entry,
        _state: &mut State,
    ) -> Option<Result<String, String>> {
        None
    }
    fn on_user_message(&self, _state: &mut State) {}
    fn on_stream_stop(&self, _state: &mut State) {}

    fn on_stream_chunk(&self, _text: &str, _state: &mut State) {}

    fn on_tool_progress(&self, _tool_name: &str, _input_so_far: &str, _state: &mut State) {}

    fn on_tool_complete(&self, _tool_name: &str, _state: &mut State) {}
    fn watcher_immediate_refresh(&self) -> bool {
        true
    }
}

/// Directory names whose subtree is pure agent/build/VCS bookkeeping noise for
/// a navigation tree — a change anywhere under one of these never warrants a
/// tree rebuild (see [`path_under_control_dir`]).
///
/// `oplog` and `.context-pilot` are the agent's own high-frequency writers (the
/// append-only log and the tier-② persistence dir), so they dominate the churn
/// when the bridge is ON; the rest are the usual VCS/build/dependency dirs plus
/// the chat-upload drop.
const CONTROL_DIRS: &[&str] = &[
    ".git",
    ".context-pilot",
    "oplog",
    "target",
    "node_modules",
    "__pycache__",
    ".venv",
    "venv",
    "dist",
    "build",
    ".uploads",
];

/// Whether any path component of `changed_path` is a [control directory](CONTROL_DIRS).
///
/// Splits on the path separator and matches component-wise (rather than a raw
/// substring test) so it triggers on a genuine `…/oplog/…` segment but not on a
/// source file that merely *contains* the word (e.g. `src/oplog_view.rs`).
/// Accepts both absolute and realm-relative paths since either form may reach
/// the watcher.
fn path_under_control_dir(changed_path: &str) -> bool {
    std::path::Path::new(changed_path)
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(os) => os.to_str(),
            std::path::Component::Prefix(_)
            | std::path::Component::RootDir
            | std::path::Component::CurDir
            | std::path::Component::ParentDir => None,
        })
        .any(|seg| CONTROL_DIRS.contains(&seg))
}

/// Visualizer for tree tool results.
/// Shows tree operations with colored indicators and highlights changed descriptions.
fn visualize_tree_output(content: &str, width: usize) -> Vec<cp_render::Block> {
    use cp_render::{Block, Semantic, Span};

    content
        .lines()
        .map(|line| {
            if line.is_empty() {
                return Block::empty();
            }
            let semantic = if line.starts_with("Error:") {
                Semantic::Error
            } else if line.starts_with("Updated") || line.starts_with("Added") {
                Semantic::Success
            } else if line.starts_with("Opened") || line.contains("folder") {
                Semantic::Info
            } else if line.starts_with("Closed") || line.contains("[!]") || line.contains("Modified") {
                Semantic::Warning
            } else {
                Semantic::Default
            };
            let display = if line.len() > width {
                format!("{}...", line.get(..line.floor_char_boundary(width.saturating_sub(3))).unwrap_or(""))
            } else {
                line.to_owned()
            };
            Block::Line(vec![Span::styled(display, semantic)])
        })
        .collect()
}
