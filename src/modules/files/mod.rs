mod panel;
mod tools;

use crate::core::panels::Panel;
use crate::state::{ContextType, State};
use crate::tool_defs::{ToolDefinition, ToolParam, ParamType, ToolCategory};
use crate::tools::{ToolUse, ToolResult};

use self::panel::FilePanel;
use super::Module;

pub struct FilesModule;

impl Module for FilesModule {
    fn id(&self) -> &'static str { "files" }
    fn name(&self) -> &'static str { "Files" }
    fn description(&self) -> &'static str { "File open, edit, write, and create tools" }
    fn is_core(&self) -> bool { true }
    fn is_global(&self) -> bool { true }

    fn dynamic_panel_types(&self) -> Vec<ContextType> {
        vec![ContextType::File]
    }

    fn create_panel(&self, context_type: ContextType) -> Option<Box<dyn Panel>> {
        match context_type {
            ContextType::File => Some(Box::new(FilePanel)),
            _ => None,
        }
    }

    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                id: "file_open".to_string(),
                name: "Open File".to_string(),
                short_desc: "Read file into context".to_string(),
                description: "Opens a file and adds it to context so you can see its content. ALWAYS use this BEFORE file_edit to see current content - you need exact text for edits.".to_string(),
                params: vec![
                    ToolParam::new("path", ParamType::String)
                        .desc("Path to the file to open")
                        .required(),
                ],
                enabled: true,
                category: ToolCategory::File,
            },
            ToolDefinition {
                id: "file_edit".to_string(),
                name: "Edit File".to_string(),
                short_desc: "Modify file content".to_string(),
                description: "Edits a file by replacing exact text. PREFERRED over file_write for any modification — only use file_write to create new files or completely replace all content. IMPORTANT: 1) Use file_open FIRST to see current content. 2) old_string must be EXACT text from file (copy from context). 3) To append, use the last line as old_string and include it + new content in new_string.".to_string(),
                params: vec![
                    ToolParam::new("file_path", ParamType::String)
                        .desc("Absolute path to the file to edit")
                        .required(),
                    ToolParam::new("old_string", ParamType::String)
                        .desc("Exact text to find and replace (copy from file context)")
                        .required(),
                    ToolParam::new("new_string", ParamType::String)
                        .desc("Replacement text")
                        .required(),
                    ToolParam::new("replace_all", ParamType::Boolean)
                        .desc("Replace all occurrences (default: false)"),
                ],
                enabled: true,
                category: ToolCategory::File,
            },
            ToolDefinition {
                id: "file_write".to_string(),
                name: "Write File".to_string(),
                short_desc: "Create or overwrite file".to_string(),
                description: "Writes complete contents to a file, creating it if it doesn't exist or replacing all content if it does. Use ONLY for creating new files or completely replacing file content. For targeted edits (changing specific sections, appending, inserting), ALWAYS prefer file_edit instead — it is safer and more precise.".to_string(),
                params: vec![
                    ToolParam::new("file_path", ParamType::String)
                        .desc("Path to the file to write")
                        .required(),
                    ToolParam::new("contents", ParamType::String)
                        .desc("Complete file contents to write")
                        .required(),
                ],
                enabled: true,
                category: ToolCategory::File,
            },
            ToolDefinition {
                id: "file_create".to_string(),
                name: "Create File".to_string(),
                short_desc: "Create new file".to_string(),
                description: "Creates a NEW file. Fails if file exists - use file_edit to modify existing files.".to_string(),
                params: vec![
                    ToolParam::new("path", ParamType::String)
                        .desc("Path for the new file")
                        .required(),
                    ToolParam::new("content", ParamType::String)
                        .desc("Content to write to the file")
                        .required(),
                ],
                enabled: true,
                category: ToolCategory::File,
            },
            ToolDefinition {
                id: "file_batch_create".to_string(),
                name: "Batch Create".to_string(),
                short_desc: "Create files/folders".to_string(),
                description: "Creates multiple files and/or folders in one call. Fails for items that already exist. Parent directories are created automatically.".to_string(),
                params: vec![
                    ToolParam::new("items", ParamType::Array(Box::new(ParamType::Object(vec![
                        ToolParam::new("type", ParamType::String)
                            .desc("Item type: 'file' or 'folder'")
                            .enum_vals(&["file", "folder"])
                            .required(),
                        ToolParam::new("path", ParamType::String)
                            .desc("Path to create")
                            .required(),
                        ToolParam::new("content", ParamType::String)
                            .desc("File content (only for type='file', optional)"),
                    ]))))
                        .desc("Array of items to create")
                        .required(),
                ],
                enabled: true,
                category: ToolCategory::File,
            },
        ]
    }

    fn execute_tool(&self, tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
        match tool.name.as_str() {
            "file_open" => Some(self::tools::file::execute_open(tool, state)),
            "file_edit" => Some(self::tools::edit_file::execute_edit(tool, state)),
            "file_write" => Some(self::tools::write::execute(tool, state)),
            "file_create" => Some(self::tools::edit_file::execute_create(tool, state)),
            "file_batch_create" => Some(self::tools::create::execute(tool, state)),
            _ => None,
        }
    }
}
