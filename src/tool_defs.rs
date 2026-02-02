use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Parameter type for tool inputs
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamType {
    String,
    Integer,
    Boolean,
    Array(Box<ParamType>),
    Object(Vec<ToolParam>),
}

impl ParamType {
    fn to_json_schema(&self) -> Value {
        match self {
            ParamType::String => json!({"type": "string"}),
            ParamType::Integer => json!({"type": "integer"}),
            ParamType::Boolean => json!({"type": "boolean"}),
            ParamType::Array(inner) => json!({
                "type": "array",
                "items": inner.to_json_schema()
            }),
            ParamType::Object(params) => {
                let mut properties = serde_json::Map::new();
                let mut required = Vec::new();
                for param in params {
                    let mut schema = param.param_type.to_json_schema();
                    if let Some(desc) = &param.description {
                        schema["description"] = json!(desc);
                    }
                    if let Some(enum_vals) = &param.enum_values {
                        schema["enum"] = json!(enum_vals);
                    }
                    properties.insert(param.name.clone(), schema);
                    if param.required {
                        required.push(param.name.clone());
                    }
                }
                json!({
                    "type": "object",
                    "properties": properties,
                    "required": required
                })
            }
        }
    }
}

/// A single tool parameter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolParam {
    pub name: String,
    pub param_type: ParamType,
    pub description: Option<String>,
    pub required: bool,
    pub enum_values: Option<Vec<String>>,
    pub default: Option<String>,
}

impl ToolParam {
    pub fn new(name: &str, param_type: ParamType) -> Self {
        Self {
            name: name.to_string(),
            param_type,
            description: None,
            required: false,
            enum_values: None,
            default: None,
        }
    }

    pub fn desc(mut self, d: &str) -> Self {
        self.description = Some(d.to_string());
        self
    }

    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    pub fn enum_vals(mut self, vals: &[&str]) -> Self {
        self.enum_values = Some(vals.iter().map(|s| s.to_string()).collect());
        self
    }

    pub fn default_val(mut self, val: &str) -> Self {
        self.default = Some(val.to_string());
        self
    }
}

/// A tool definition with its schema and prompt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Tool identifier (e.g., "open_file")
    pub id: String,
    /// Display name
    pub name: String,
    /// Short description for the sidebar
    pub short_desc: String,
    /// Full description for LLM prompt
    pub description: String,
    /// Structured parameters
    pub params: Vec<ToolParam>,
    /// Whether this tool is currently enabled
    pub enabled: bool,
    /// Category for grouping
    pub category: ToolCategory,
}

impl ToolDefinition {
    /// Build JSON Schema for API
    pub fn to_json_schema(&self) -> Value {
        let mut properties = serde_json::Map::new();
        let mut required = Vec::new();

        for param in &self.params {
            let mut schema = param.param_type.to_json_schema();
            if let Some(desc) = &param.description {
                schema["description"] = json!(desc);
            }
            if let Some(enum_vals) = &param.enum_values {
                schema["enum"] = json!(enum_vals);
            }
            properties.insert(param.name.clone(), schema);
            if param.required {
                required.push(param.name.clone());
            }
        }

        json!({
            "type": "object",
            "properties": properties,
            "required": required
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCategory {
    FileSystem,
    Context,
    Tmux,
    Tasks,
    Memory,
    Git,
}

/// Get all available tool definitions
pub fn get_all_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        // File System tools
        ToolDefinition {
            id: "open_file".to_string(),
            name: "Open File".to_string(),
            short_desc: "Read file into context".to_string(),
            description: "Opens a file and adds it to the context. The file content will be visible and can be referenced.".to_string(),
            params: vec![
                ToolParam::new("path", ParamType::String)
                    .desc("Path to the file to open")
                    .required(),
            ],
            enabled: true,
            category: ToolCategory::FileSystem,
        },
        ToolDefinition {
            id: "edit_file".to_string(),
            name: "Edit File".to_string(),
            short_desc: "Modify file content".to_string(),
            description: "Edits a file by applying one or more replacements sequentially. Each edit replaces old_string with new_string. The file must be in context first.".to_string(),
            params: vec![
                ToolParam::new("path", ParamType::String)
                    .desc("Path to the file to edit")
                    .required(),
                ToolParam::new("edits", ParamType::Array(Box::new(ParamType::Object(vec![
                    ToolParam::new("old_string", ParamType::String)
                        .desc("The exact string to find and replace")
                        .required(),
                    ToolParam::new("new_string", ParamType::String)
                        .desc("The string to replace with")
                        .required(),
                ]))))
                    .desc("Array of edits to apply sequentially")
                    .required(),
            ],
            enabled: true,
            category: ToolCategory::FileSystem,
        },
        ToolDefinition {
            id: "create_file".to_string(),
            name: "Create File".to_string(),
            short_desc: "Create new file".to_string(),
            description: "Creates a new file with the specified content. Will fail if file already exists.".to_string(),
            params: vec![
                ToolParam::new("path", ParamType::String)
                    .desc("Path for the new file")
                    .required(),
                ToolParam::new("content", ParamType::String)
                    .desc("Content to write to the file")
                    .required(),
            ],
            enabled: true,
            category: ToolCategory::FileSystem,
        },
        ToolDefinition {
            id: "create".to_string(),
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
            category: ToolCategory::FileSystem,
        },
        ToolDefinition {
            id: "glob".to_string(),
            name: "Glob Search".to_string(),
            short_desc: "Find files by pattern".to_string(),
            description: "Searches for files matching a glob pattern. Results are added to context.".to_string(),
            params: vec![
                ToolParam::new("pattern", ParamType::String)
                    .desc("Glob pattern (e.g., '**/*.rs', 'src/*.ts')")
                    .required(),
                ToolParam::new("path", ParamType::String)
                    .desc("Base path to search from")
                    .default_val("."),
            ],
            enabled: true,
            category: ToolCategory::FileSystem,
        },
        ToolDefinition {
            id: "grep".to_string(),
            name: "Grep Search".to_string(),
            short_desc: "Search file contents".to_string(),
            description: "Searches file contents for a regex pattern. Results show matching lines with file:line context. Results are added to context and update dynamically.".to_string(),
            params: vec![
                ToolParam::new("pattern", ParamType::String)
                    .desc("Regex pattern to search for")
                    .required(),
                ToolParam::new("path", ParamType::String)
                    .desc("Base path to search from")
                    .default_val("."),
                ToolParam::new("file_pattern", ParamType::String)
                    .desc("Glob pattern to filter files (e.g., '*.rs', '*.ts')"),
            ],
            enabled: true,
            category: ToolCategory::FileSystem,
        },
        ToolDefinition {
            id: "edit_tree_filter".to_string(),
            name: "Edit Tree Filter".to_string(),
            short_desc: "Configure directory filter".to_string(),
            description: "Edits the gitignore-style filter for the directory tree view.".to_string(),
            params: vec![
                ToolParam::new("filter", ParamType::String)
                    .desc("Gitignore-style patterns, one per line")
                    .required(),
            ],
            enabled: true,
            category: ToolCategory::FileSystem,
        },
        ToolDefinition {
            id: "tree_toggle_folders".to_string(),
            name: "Toggle Tree Folders".to_string(),
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
            category: ToolCategory::FileSystem,
        },
        ToolDefinition {
            id: "tree_describe_files".to_string(),
            name: "Describe Tree Items".to_string(),
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
            category: ToolCategory::FileSystem,
        },

        // Context tools
        ToolDefinition {
            id: "close_contexts".to_string(),
            name: "Close Contexts".to_string(),
            short_desc: "Remove items from context".to_string(),
            description: "Closes context elements by their IDs (e.g., P6, P7). Cannot close core elements (P1-P6).".to_string(),
            params: vec![
                ToolParam::new("ids", ParamType::Array(Box::new(ParamType::String)))
                    .desc("List of context IDs to close")
                    .required(),
            ],
            enabled: true,
            category: ToolCategory::Context,
        },
        ToolDefinition {
            id: "set_message_status".to_string(),
            name: "Set Message Status".to_string(),
            short_desc: "Manage message visibility".to_string(),
            description: "Changes message status to control what's sent to the LLM. Batched.".to_string(),
            params: vec![
                ToolParam::new("changes", ParamType::Array(Box::new(ParamType::Object(vec![
                    ToolParam::new("message_id", ParamType::String)
                        .desc("Message ID (e.g., U1, A3)")
                        .required(),
                    ToolParam::new("status", ParamType::String)
                        .desc("full | summarized | deleted")
                        .required(),
                    ToolParam::new("tl_dr", ParamType::String)
                        .desc("Required when status is 'summarized'"),
                ]))))
                    .desc("Array of status changes")
                    .required(),
            ],
            enabled: true,
            category: ToolCategory::Context,
        },

        // Tmux tools
        ToolDefinition {
            id: "create_tmux_pane".to_string(),
            name: "Create Tmux Pane".to_string(),
            short_desc: "Add terminal to context".to_string(),
            description: "Creates a tmux pane context element to monitor terminal output.".to_string(),
            params: vec![
                ToolParam::new("pane_id", ParamType::String)
                    .desc("Tmux pane ID (e.g., %0, %1)")
                    .required(),
                ToolParam::new("lines", ParamType::Integer)
                    .desc("Number of lines to capture")
                    .default_val("50"),
                ToolParam::new("description", ParamType::String)
                    .desc("Description of what this pane is for"),
            ],
            enabled: true,
            category: ToolCategory::Tmux,
        },
        ToolDefinition {
            id: "edit_tmux_config".to_string(),
            name: "Edit Tmux Config".to_string(),
            short_desc: "Update pane settings".to_string(),
            description: "Updates configuration for an existing tmux pane context.".to_string(),
            params: vec![
                ToolParam::new("context_id", ParamType::String)
                    .desc("Context ID of the tmux pane (e.g., P7)")
                    .required(),
                ToolParam::new("lines", ParamType::Integer)
                    .desc("Number of lines to capture"),
                ToolParam::new("description", ParamType::String)
                    .desc("New description"),
            ],
            enabled: true,
            category: ToolCategory::Tmux,
        },
        ToolDefinition {
            id: "tmux_send_keys".to_string(),
            name: "Tmux Send Keys".to_string(),
            short_desc: "Send keys to terminal".to_string(),
            description: "Sends keystrokes to a tmux pane. Use for running commands or interacting with terminal apps.".to_string(),
            params: vec![
                ToolParam::new("context_id", ParamType::String)
                    .desc("Context ID of the tmux pane (e.g., P7)")
                    .required(),
                ToolParam::new("keys", ParamType::String)
                    .desc("Keys to send (e.g., 'ls -la' or 'Enter' or 'C-c')")
                    .required(),
            ],
            enabled: true,
            category: ToolCategory::Tmux,
        },
        ToolDefinition {
            id: "sleep".to_string(),
            name: "Sleep".to_string(),
            short_desc: "Wait 2 seconds".to_string(),
            description: "Pauses execution for 2 seconds. Useful for waiting for terminal output or processes to complete.".to_string(),
            params: vec![],
            enabled: true,
            category: ToolCategory::Tmux,
        },

        // Task tools
        ToolDefinition {
            id: "create_todos".to_string(),
            name: "Create Todos".to_string(),
            short_desc: "Add task items".to_string(),
            description: "Creates one or more todo items. Supports nesting via parent_id.".to_string(),
            params: vec![
                ToolParam::new("todos", ParamType::Array(Box::new(ParamType::Object(vec![
                    ToolParam::new("name", ParamType::String)
                        .desc("Todo title")
                        .required(),
                    ToolParam::new("description", ParamType::String)
                        .desc("Detailed description"),
                    ToolParam::new("parent_id", ParamType::String)
                        .desc("Parent todo ID for nesting"),
                ]))))
                    .desc("Array of todos to create")
                    .required(),
            ],
            enabled: true,
            category: ToolCategory::Tasks,
        },
        ToolDefinition {
            id: "update_todos".to_string(),
            name: "Update Todos".to_string(),
            short_desc: "Modify task items".to_string(),
            description: "Updates existing todos: change status, name, description, or delete.".to_string(),
            params: vec![
                ToolParam::new("updates", ParamType::Array(Box::new(ParamType::Object(vec![
                    ToolParam::new("id", ParamType::String)
                        .desc("Todo ID (e.g., X1)")
                        .required(),
                    ToolParam::new("status", ParamType::String)
                        .desc("New status")
                        .enum_vals(&["pending", "in_progress", "done"]),
                    ToolParam::new("name", ParamType::String)
                        .desc("New name"),
                    ToolParam::new("description", ParamType::String)
                        .desc("New description"),
                    ToolParam::new("delete", ParamType::Boolean)
                        .desc("Set true to delete"),
                ]))))
                    .desc("Array of todo updates")
                    .required(),
            ],
            enabled: true,
            category: ToolCategory::Tasks,
        },

        // Memory tools
        ToolDefinition {
            id: "create_memories".to_string(),
            name: "Create Memories".to_string(),
            short_desc: "Store persistent notes".to_string(),
            description: "Creates memory items for important information to remember across the conversation.".to_string(),
            params: vec![
                ToolParam::new("memories", ParamType::Array(Box::new(ParamType::Object(vec![
                    ToolParam::new("content", ParamType::String)
                        .desc("Memory content")
                        .required(),
                    ToolParam::new("importance", ParamType::String)
                        .desc("Importance level")
                        .enum_vals(&["low", "medium", "high", "critical"])
                        .default_val("medium"),
                ]))))
                    .desc("Array of memories to create")
                    .required(),
            ],
            enabled: true,
            category: ToolCategory::Memory,
        },
        ToolDefinition {
            id: "update_memories".to_string(),
            name: "Update Memories".to_string(),
            short_desc: "Modify stored notes".to_string(),
            description: "Updates or deletes existing memory items.".to_string(),
            params: vec![
                ToolParam::new("updates", ParamType::Array(Box::new(ParamType::Object(vec![
                    ToolParam::new("id", ParamType::String)
                        .desc("Memory ID (e.g., M1)")
                        .required(),
                    ToolParam::new("content", ParamType::String)
                        .desc("New content"),
                    ToolParam::new("importance", ParamType::String)
                        .desc("New importance level")
                        .enum_vals(&["low", "medium", "high", "critical"]),
                    ToolParam::new("delete", ParamType::Boolean)
                        .desc("Set true to delete"),
                ]))))
                    .desc("Array of memory updates")
                    .required(),
            ],
            enabled: true,
            category: ToolCategory::Memory,
        },

        // Git tools
        ToolDefinition {
            id: "git_commit".to_string(),
            name: "Git Commit".to_string(),
            short_desc: "Commit changes".to_string(),
            description: "Stages specified files (or uses current staging) and creates a git commit. Returns the commit hash and summary of changes.".to_string(),
            params: vec![
                ToolParam::new("message", ParamType::String)
                    .desc("Commit message")
                    .required(),
                ToolParam::new("files", ParamType::Array(Box::new(ParamType::String)))
                    .desc("File paths to stage before committing. If empty, commits currently staged files."),
            ],
            enabled: true,
            category: ToolCategory::Git,
        },

        // Meta tools
        ToolDefinition {
            id: "manage_tools".to_string(),
            name: "Manage Tools".to_string(),
            short_desc: "Enable/disable tools".to_string(),
            description: "Enables or disables tools. This tool cannot be disabled. Use to customize available capabilities.".to_string(),
            params: vec![
                ToolParam::new("changes", ParamType::Array(Box::new(ParamType::Object(vec![
                    ToolParam::new("tool", ParamType::String)
                        .desc("Tool ID to change (e.g., 'edit_file', 'glob')")
                        .required(),
                    ToolParam::new("action", ParamType::String)
                        .desc("Action to perform")
                        .enum_vals(&["enable", "disable"])
                        .required(),
                ]))))
                    .desc("Array of tool changes")
                    .required(),
            ],
            enabled: true,
            category: ToolCategory::Context,
        },
    ]
}

/// Build the API tool definitions from enabled tools
pub fn build_api_tools(tools: &[ToolDefinition]) -> Value {
    let enabled: Vec<Value> = tools.iter()
        .filter(|t| t.enabled)
        .map(|t| {
            json!({
                "name": t.id,
                "description": t.description,
                "input_schema": t.to_json_schema()
            })
        })
        .collect();

    Value::Array(enabled)
}
