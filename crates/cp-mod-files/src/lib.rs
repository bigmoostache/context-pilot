//! Files module — read, edit, and write project files.
//!
//! Three tools: `Open` (read file into context panel with syntax highlighting),
//! `Edit` (`old_string/new_string` diff replacement), `Write` (create or fully
//! overwrite). File panels auto-refresh on filesystem changes via the watcher.

/// File panel rendering and caching.
mod panel;
/// Tool implementations for Open, Edit, and Write.
mod tools;

use cp_base::modules::ToolVisualizer;
use cp_base::panels::Panel;
use cp_base::state::context::Kind;
use cp_base::state::runtime::State;
use cp_base::tools::{ParamType, ToolDefinition, ToolTexts};
use cp_base::tools::{ToolResult, ToolUse};

use self::panel::FilePanel;
use cp_base::modules::Module;
use cp_base::tools::pre_flight::Verdict;

/// Lazily parsed tool YAML definitions for the files module.
static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> =
    std::sync::LazyLock::new(|| ToolTexts::parse(include_str!("../../../yamls/tools/files.yaml")));

/// Files module: Open, Edit, Write tools for file manipulation.
#[derive(Debug, Clone, Copy)]
pub struct FilesModule;

impl Module for FilesModule {
    fn id(&self) -> &'static str {
        "files"
    }
    fn name(&self) -> &'static str {
        "Files"
    }
    fn description(&self) -> &'static str {
        "File open, edit, write, and create tools"
    }
    fn is_core(&self) -> bool {
        true
    }
    fn is_global(&self) -> bool {
        true
    }

    fn dynamic_panel_types(&self) -> Vec<Kind> {
        vec![Kind::new(Kind::FILE)]
    }

    fn create_panel(&self, context_type: &Kind) -> Option<Box<dyn Panel>> {
        match context_type.as_str() {
            Kind::FILE => Some(Box::new(FilePanel)),
            _ => None,
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let t = &*TOOL_TEXTS;
        vec![
            ToolDefinition::from_yaml("Open", t)
                .short_desc("Read file into context")
                .category("File")
                .reverie_allowed(true)
                .param_array("path", ParamType::String, true)
                .build(),
            ToolDefinition::from_yaml("Edit", t)
                .short_desc("Modify file content")
                .category("File")
                .param("file_path", ParamType::String, true)
                .param("old_string", ParamType::String, true)
                .param("new_string", ParamType::String, true)
                .param("replace_all", ParamType::Boolean, false)
                .param_array("skip_callbacks", ParamType::String, false)
                .build(),
            ToolDefinition::from_yaml("Write", t)
                .short_desc("Create or overwrite file")
                .category("File")
                .param("file_path", ParamType::String, true)
                .param("contents", ParamType::String, true)
                .param_array("skip_callbacks", ParamType::String, false)
                .build(),
        ]
    }

    fn pre_flight(&self, tool: &ToolUse, state: &State) -> Option<Verdict> {
        match tool.name.as_str() {
            "Open" => {
                let mut pf = Verdict::new();
                let paths: Vec<String> = match tool.input.get("path") {
                    Some(serde_json::Value::String(s)) => vec![s.clone()],
                    Some(serde_json::Value::Array(arr)) => {
                        arr.iter().filter_map(|v| v.as_str().map(String::from)).collect()
                    }
                    _ => return Some(pf),
                };
                for path in &paths {
                    let p = std::path::Path::new(path);
                    if !p.exists() {
                        pf.errors.push(format!("File '{path}' not found"));
                    } else if !p.is_file() {
                        pf.errors.push(format!("'{path}' is not a file"));
                    } else {
                        // Canonicalize for consistent comparison with stored paths
                        let canonical =
                            p.canonicalize().map_or_else(|_| path.clone(), |cp| cp.to_string_lossy().to_string());
                        if state.context.iter().any(|c| c.get_meta_str("file_path") == Some(&canonical)) {
                            pf.warnings.push(format!("File '{path}' is already open in context"));
                        }
                    }
                }
                Some(pf)
            }
            "Edit" => {
                let mut pf = Verdict::new();
                // File edits are destructive — auto-activate queue for batching
                pf.activate_queue = true;
                if let Some(path_str) = tool.input.get("file_path").and_then(|v| v.as_str()) {
                    let p = std::path::Path::new(path_str);
                    if !p.exists() {
                        pf.errors.push(format!("File '{path_str}' not found"));
                    } else if !p.is_file() {
                        pf.errors.push(format!("'{path_str}' is not a file"));
                    } else {
                        // Canonicalize for consistent comparison with stored paths
                        let canonical = p
                            .canonicalize()
                            .map_or_else(|_| path_str.to_string(), |cp| cp.to_string_lossy().to_string());
                        let is_open = state.context.iter().any(|c| {
                            c.context_type.as_str() == Kind::FILE && c.get_meta_str("file_path") == Some(&canonical)
                        });
                        if !is_open {
                            pf.warnings.push(format!("File '{path_str}' is not open in context. Edit will proceed if old_string has a unique match, but open the file to see current content."));
                        }
                        // Verify old_string actually matches file content
                        if let Some(old_string) = tool.input.get("old_string").and_then(|v| v.as_str())
                            && let Ok(content) = std::fs::read_to_string(p)
                            && tools::edit_file::find_normalized_match(&content, old_string).is_none()
                        {
                            pf.errors.push(format!(
                                "old_string not found in '{path_str}' — open the file to see current content"
                            ));
                        }
                    }
                }
                Some(pf)
            }
            "Write" => {
                let mut pf = Verdict::new();
                // File writes are destructive — auto-activate queue for batching
                pf.activate_queue = true;
                if let Some(path_str) = tool.input.get("file_path").and_then(|v| v.as_str()) {
                    let p = std::path::Path::new(path_str);
                    if let Some(parent) = p.parent()
                        && !parent.as_os_str().is_empty()
                        && !parent.exists()
                    {
                        pf.warnings.push(format!(
                            "Parent directory '{}' does not exist — it will be created automatically",
                            parent.display()
                        ));
                    }
                }
                Some(pf)
            }
            _ => None,
        }
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            "Open" => Some(tools::file::execute_open(tool, state)),
            "Edit" => Some(tools::edit_file::execute_edit(tool, state)),
            "Write" => Some(tools::write::execute(tool, state)),

            _ => None,
        }
    }

    fn tool_visualizers(&self) -> Vec<(&'static str, ToolVisualizer)> {
        vec![("Edit", visualize_diff), ("Write", visualize_diff)]
    }

    fn context_type_metadata(&self) -> Vec<cp_base::state::context::TypeMeta> {
        vec![cp_base::state::context::TypeMeta {
            context_type: "file",
            icon_id: "file",
            is_fixed: false,
            needs_cache: true,
            fixed_order: None,
            display_name: "file",
            short_name: "file",
            needs_async_wait: true,
        }]
    }

    fn context_detail(&self, ctx: &cp_base::state::context::Entry) -> Option<String> {
        (ctx.context_type.as_str() == Kind::FILE).then(|| ctx.get_meta_str("file_path").unwrap_or("").to_string())
    }

    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![("File", "Read, write, and search files in the project")]
    }

    fn watch_paths(&self, state: &State) -> Vec<cp_base::panels::WatchSpec> {
        state
            .context
            .iter()
            .filter(|c| c.context_type.as_str() == Kind::FILE)
            .filter_map(|c| c.get_meta_str("file_path").map(|p| cp_base::panels::WatchSpec::File(p.to_string())))
            .collect()
    }

    fn should_invalidate_on_fs_change(
        &self,
        ctx: &cp_base::state::context::Entry,
        changed_path: &str,
        is_dir_event: bool,
    ) -> bool {
        if is_dir_event {
            return false;
        }
        ctx.context_type.as_str() == Kind::FILE && ctx.get_meta_str("file_path") == Some(changed_path)
    }

    fn dependencies(&self) -> &[&'static str] {
        &[]
    }
    fn init_state(&self, _state: &mut State) {}
    fn reset_state(&self, _state: &mut State) {}
    fn save_module_data(&self, _state: &State) -> serde_json::Value {
        serde_json::Value::Null
    }
    fn load_module_data(&self, _data: &serde_json::Value, _state: &mut State) {}
    fn save_worker_data(&self, _state: &State) -> serde_json::Value {
        serde_json::Value::Null
    }
    fn load_worker_data(&self, _data: &serde_json::Value, _state: &mut State) {}
    fn fixed_panel_types(&self) -> Vec<Kind> {
        vec![]
    }
    fn fixed_panel_defaults(&self) -> Vec<(Kind, &'static str, bool)> {
        vec![]
    }
    fn context_display_name(&self, _context_type: &str) -> Option<&'static str> {
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

    fn on_tool_progress(&self, _tool_name: &str, _input_so_far: &str, _state: &mut State) {}

    fn on_tool_complete(&self, _tool_name: &str, _state: &mut State) {}
    fn watcher_immediate_refresh(&self) -> bool {
        true
    }
}

/// Visualizer for Edit and Write tool results.
///
/// Also reused by cp-mod-prompt for `Edit_prompt`.
/// Parses diff blocks and renders deleted lines in red, added lines in green.
/// Callback summary blocks get compact styled rendering (only status word colored).
/// Non-diff content is rendered in secondary text color.
#[must_use]
pub fn visualize_diff(content: &str, width: usize) -> Vec<cp_render::Block> {
    use cp_render::{Block, Semantic, Span};

    let mut blocks = Vec::new();
    let mut in_diff_block = false;

    for line in content.lines() {
        // Detect diff block markers
        if line.trim() == "```diff" {
            in_diff_block = true;
            continue;
        }
        if line.trim() == "```" && in_diff_block {
            in_diff_block = false;
            continue;
        }

        if line.is_empty() {
            blocks.push(Block::empty());
            continue;
        }

        if in_diff_block {
            let semantic = if line.starts_with("- ") {
                Semantic::DiffRemove
            } else if line.starts_with("+ ") {
                Semantic::DiffAdd
            } else {
                Semantic::Muted
            };
            blocks.push(Block::Line(vec![Span::styled(truncate_line(line, width), semantic)]));
        } else if let Some(styled) = style_callback_line_ir(line, width) {
            blocks.push(styled);
        } else {
            // Non-diff content: plain muted text
            blocks.push(Block::Line(vec![Span::muted(truncate_line(line, width))]));
        }
    }

    blocks
}

/// Truncate a line to fit within the given width.
fn truncate_line(line: &str, width: usize) -> String {
    if line.len() > width {
        format!("{}…", &line.get(..line.floor_char_boundary(width.saturating_sub(1))).unwrap_or(""))
    } else {
        line.to_string()
    }
}

/// Style callback-related lines in tool results using IR blocks.
/// Format: "Callbacks:" header, "· name passed/FAILED/TIMED OUT ...", "    error line"
fn style_callback_line_ir(line: &str, width: usize) -> Option<cp_render::Block> {
    use cp_render::{Block, Semantic, Span};

    let trimmed = line.trim();

    // "Callbacks:" header
    if trimmed == "Callbacks:" {
        return Some(Block::Line(vec![Span::muted(truncate_line(trimmed, width))]));
    }

    // "· name passed ..." or "· name FAILED ..." etc.
    if let Some(rest) = trimmed.strip_prefix("· ") {
        let mut spans = vec![Span::muted("· ".to_string())];

        let status_patterns: &[(&str, Semantic)] = &[
            (" passed", Semantic::Success),
            (" FAILED", Semantic::Error),
            (" TIMED OUT", Semantic::Error),
            (" dispatched", Semantic::Info),
            (" skipped", Semantic::Muted),
        ];

        let mut matched = false;
        for &(pattern, semantic) in status_patterns {
            if let Some(pos) = rest.find(pattern) {
                let name = rest.get(..pos).unwrap_or("");
                spans.push(Span::muted(name.to_string()));
                spans.push(Span::styled(pattern.to_string(), semantic));
                let after_start = pos.saturating_add(pattern.len());
                if let Some(after) = rest.get(after_start..)
                    && !after.is_empty()
                {
                    spans.push(Span::muted(after.to_string()));
                }
                matched = true;
                break;
            }
        }
        if !matched {
            spans.push(Span::muted(rest.to_string()));
        }
        return Some(Block::line(spans));
    }

    // Indented error lines (4 spaces)
    if line.starts_with("    ") && !line.trim().is_empty() {
        return Some(Block::Line(vec![Span::error(truncate_line(line, width))]));
    }

    // [skip_callbacks warnings: ...]
    if trimmed.starts_with("[skip_callbacks warnings:") {
        return Some(Block::Line(vec![Span::warning(truncate_line(trimmed, width))]));
    }

    None
}
