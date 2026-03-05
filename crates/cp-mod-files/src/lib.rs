//! Files module — read, edit, and write project files.
//!
//! Three tools: `Open` (read file into context panel with syntax highlighting),
//! `Edit` (`old_string/new_string` diff replacement), `Write` (create or fully
//! overwrite). File panels auto-refresh on filesystem changes via the watcher.

mod panel;
mod tools;

use cp_base::modules::ToolVisualizer;
use cp_base::panels::Panel;
use cp_base::state::{ContextType, State};
use cp_base::tools::{ParamType, ToolDefinition, ToolTexts};
use cp_base::tools::{ToolResult, ToolUse};

use self::panel::FilePanel;
use cp_base::modules::Module;
use cp_base::tools::PreFlightResult;

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

    fn dynamic_panel_types(&self) -> Vec<ContextType> {
        vec![ContextType::new(ContextType::FILE)]
    }

    fn create_panel(&self, context_type: &ContextType) -> Option<Box<dyn Panel>> {
        match context_type.as_str() {
            ContextType::FILE => Some(Box::new(FilePanel)),
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

    fn pre_flight(&self, tool: &ToolUse, state: &State) -> Option<PreFlightResult> {
        match tool.name.as_str() {
            "Open" => {
                let mut pf = PreFlightResult::new();
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
                let mut pf = PreFlightResult::new();
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
                            c.context_type == ContextType::FILE && c.get_meta_str("file_path") == Some(&canonical)
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
                let mut pf = PreFlightResult::new();
                if let Some(path_str) = tool.input.get("file_path").and_then(|v| v.as_str()) {
                    let p = std::path::Path::new(path_str);
                    if let Some(parent) = p.parent()
                        && !parent.as_os_str().is_empty()
                        && !parent.exists()
                    {
                        pf.errors.push(format!("Parent directory '{}' does not exist", parent.display()));
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

    fn context_type_metadata(&self) -> Vec<cp_base::state::ContextTypeMeta> {
        vec![cp_base::state::ContextTypeMeta {
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

    fn context_detail(&self, ctx: &cp_base::state::ContextElement) -> Option<String> {
        if ctx.context_type.as_str() == ContextType::FILE {
            Some(ctx.get_meta_str("file_path").unwrap_or("").to_string())
        } else {
            None
        }
    }

    fn tool_category_descriptions(&self) -> Vec<(&'static str, &'static str)> {
        vec![("File", "Read, write, and search files in the project")]
    }

    fn watch_paths(&self, state: &State) -> Vec<cp_base::panels::WatchSpec> {
        state
            .context
            .iter()
            .filter(|c| c.context_type.as_str() == ContextType::FILE)
            .filter_map(|c| c.get_meta_str("file_path").map(|p| cp_base::panels::WatchSpec::File(p.to_string())))
            .collect()
    }

    fn should_invalidate_on_fs_change(
        &self,
        ctx: &cp_base::state::ContextElement,
        changed_path: &str,
        is_dir_event: bool,
    ) -> bool {
        if is_dir_event {
            return false;
        }
        ctx.context_type.as_str() == ContextType::FILE && ctx.get_meta_str("file_path") == Some(changed_path)
    }
}

/// Visualizer for Edit and Write tool results.
/// Also reused by cp-mod-prompt for `Edit_prompt`.
/// Parses diff blocks and renders deleted lines in red, added lines in green.
/// Callback summary blocks get compact styled rendering (only status word colored).
/// Non-diff content is rendered in secondary text color.
#[must_use]
pub fn visualize_diff(content: &str, width: usize) -> Vec<ratatui::text::Line<'static>> {
    use ratatui::prelude::*;

    let error_color = Color::Rgb(255, 85, 85);
    let success_color = Color::Rgb(80, 250, 123);
    let secondary_color = Color::Rgb(150, 150, 170);
    let cb_blue = Color::Rgb(100, 160, 220);
    let cb_dim = Color::Rgb(110, 110, 130);

    let mut lines = Vec::new();
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

        // Skip empty lines inside callback blocks (no wasted vertical space)
        if line.is_empty() {
            lines.push(Line::from(""));
            continue;
        }

        if in_diff_block {
            let style = if line.starts_with("- ") {
                Style::default().fg(error_color)
            } else if line.starts_with("+ ") {
                Style::default().fg(success_color)
            } else {
                Style::default().fg(secondary_color)
            };
            let display = truncate_line(line, width);
            lines.push(Line::from(Span::styled(display, style)));
        } else if let Some(styled) = style_callback_line(line, width, cb_blue, success_color, error_color, cb_dim) {
            lines.push(styled);
        } else {
            // Non-diff content: plain secondary text
            let display = truncate_line(line, width);
            lines.push(Line::from(Span::styled(display, Style::default().fg(secondary_color))));
        }
    }

    lines
}

/// Truncate a line to fit within the given width.
fn truncate_line(line: &str, width: usize) -> String {
    if line.len() > width {
        format!("{}…", &line[..line.floor_char_boundary(width.saturating_sub(1))])
    } else {
        line.to_string()
    }
}

/// Style callback-related lines in tool results.
/// Format: "Callbacks:" header, "· name ✓ log: path", "· name ✗ P20", "    error line"
/// Only the status symbol (✓/✗/⏳) is colored. Rest is dim.
fn style_callback_line(
    line: &str,
    width: usize,
    blue: ratatui::style::Color,
    green: ratatui::style::Color,
    red: ratatui::style::Color,
    dim: ratatui::style::Color,
) -> Option<ratatui::text::Line<'static>> {
    use ratatui::prelude::*;

    let trimmed = line.trim();

    // "Callbacks:" header
    if trimmed == "Callbacks:" {
        return Some(Line::from(Span::styled(truncate_line(trimmed, width), Style::default().fg(dim))));
    }

    // "· name passed ..." or "· name FAILED ..." or "· name running"
    if let Some(rest) = trimmed.strip_prefix("· ") {
        let mut spans = Vec::new();
        spans.push(Span::styled("· ", Style::default().fg(dim)));

        // Find the status word and split around it
        if let Some(pos) = rest.find(" passed") {
            let name = &rest[..pos];
            let after = &rest[pos + 7..]; // skip " passed"
            spans.push(Span::styled(name.to_string(), Style::default().fg(dim)));
            spans.push(Span::styled(" passed", Style::default().fg(green)));
            if !after.is_empty() {
                spans.push(Span::styled(after.to_string(), Style::default().fg(dim)));
            }
        } else if let Some(pos) = rest.find(" FAILED") {
            let name = &rest[..pos];
            let after = &rest[pos + 7..]; // skip " FAILED"
            spans.push(Span::styled(name.to_string(), Style::default().fg(dim)));
            spans.push(Span::styled(" FAILED", Style::default().fg(red)));
            if !after.is_empty() {
                spans.push(Span::styled(after.to_string(), Style::default().fg(dim)));
            }
        } else if let Some(pos) = rest.find(" TIMED OUT") {
            let name = &rest[..pos];
            let after = &rest[pos + 10..]; // skip " TIMED OUT"
            spans.push(Span::styled(name.to_string(), Style::default().fg(dim)));
            spans.push(Span::styled(" TIMED OUT", Style::default().fg(red)));
            if !after.is_empty() {
                spans.push(Span::styled(after.to_string(), Style::default().fg(dim)));
            }
        } else if let Some(pos) = rest.find(" dispatched") {
            let name = &rest[..pos];
            spans.push(Span::styled(name.to_string(), Style::default().fg(dim)));
            spans.push(Span::styled(" dispatched", Style::default().fg(blue)));
        } else if let Some(pos) = rest.find(" skipped") {
            let name = &rest[..pos];
            let after = &rest[pos + 8..]; // skip " skipped"
            spans.push(Span::styled(name.to_string(), Style::default().fg(dim)));
            spans.push(Span::styled(" skipped", Style::default().fg(dim)));
            if !after.is_empty() {
                spans.push(Span::styled(after.to_string(), Style::default().fg(dim)));
            }
        } else {
            // Fallback: just dim
            spans.push(Span::styled(rest.to_string(), Style::default().fg(dim)));
        }
        return Some(Line::from(spans));
    }

    // Indented error lines (4 spaces)
    if line.starts_with("    ") && !line.trim().is_empty() {
        let display = truncate_line(line, width);
        return Some(Line::from(Span::styled(display, Style::default().fg(red))));
    }

    // [skip_callbacks warnings: ...]
    if trimmed.starts_with("[skip_callbacks warnings:") {
        let display = truncate_line(trimmed, width);
        return Some(Line::from(Span::styled(display, Style::default().fg(Color::Rgb(230, 180, 80)))));
    }

    None
}
