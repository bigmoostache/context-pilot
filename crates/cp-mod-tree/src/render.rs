//! Tree string rendering — the directory-walk that produces the annotated
//! tree text for the Tree panel.
//!
//! Split from `tools.rs` for the line budget. `tools.rs` owns the mutating
//! tool dispatch (toggle / describe / filter); this module is the pure
//! read-only renderer plus the shared `normalize_path` helper.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use ignore::gitignore::GitignoreBuilder;

use crate::tools::{SHOW_CONTEXT_PILOT, compute_file_hash};
use crate::types::TreeFileDescription;

/// Generate tree string without mutating state (for read-only rendering)
pub(crate) fn generate_tree_string(
    tree_filter: &str,
    tree_open_folders: &[String],
    tree_descriptions: &[TreeFileDescription],
) -> String {
    let root = PathBuf::from(".");

    // Build gitignore matcher from filter
    let mut builder = GitignoreBuilder::new(&root);
    for line in tree_filter.lines() {
        let line = line.trim();
        if !line.is_empty() && !line.starts_with('#') {
            let _: Option<&mut GitignoreBuilder> = builder.add_line(None, line).ok();
        }
    }
    let gitignore = builder.build().ok();

    // Build set of open folders for quick lookup
    let open_set: HashSet<_> = tree_open_folders.iter().cloned().collect();

    // Build map of descriptions for quick lookup
    let desc_map: std::collections::HashMap<_, _> = tree_descriptions.iter().map(|d| (d.path.clone(), d)).collect();

    let mut output = String::new();

    // Show pwd at the top
    if let Ok(cwd) = std::env::current_dir() {
        let _r = writeln!(output, "pwd: {}", cwd.display());
    }

    // Build tree recursively - directly show contents without root folder line
    let ctx = TreeContext { gitignore: gitignore.as_ref(), open_set: &open_set, desc_map: &desc_map };
    build_tree_new(&TreeNode { dir: &root, path_str: ".", prefix: "" }, &ctx, &mut output);

    output
}

/// Normalize a path to a consistent format
pub(crate) fn normalize_path(path: &Path) -> String {
    let path_str = path.to_string_lossy();
    let normalized = path_str.trim_start_matches("./").trim_end_matches('/');

    if normalized.is_empty() || normalized == "." { ".".to_owned() } else { normalized.to_owned() }
}

/// Context passed through tree recursion to avoid excessive parameters.
struct TreeContext<'tree> {
    /// Optional gitignore matcher for filtering entries.
    gitignore: Option<&'tree ignore::gitignore::Gitignore>,
    /// Set of folder paths currently expanded in the tree.
    open_set: &'tree HashSet<String>,
    /// Map from path to file/folder description annotation.
    desc_map: &'tree std::collections::HashMap<String, &'tree TreeFileDescription>,
}

/// Recursive traversal state for a single tree node.
struct TreeNode<'tree> {
    /// Filesystem directory path for this node.
    dir: &'tree Path,
    /// Normalized string representation of the path.
    path_str: &'tree str,
    /// Indentation prefix for tree drawing characters.
    prefix: &'tree str,
}

/// Per-entry drawing parameters for one tree row.
struct Row<'row> {
    /// Indentation prefix inherited from the parent node.
    prefix: &'row str,
    /// Branch connector (`├── ` or `└── `).
    connector: &'row str,
    /// Indentation added for this row's children.
    child_prefix: &'row str,
    /// Display name of the entry.
    name_str: &'row str,
    /// Normalized path string for description/open lookups.
    entry_path: &'row str,
}

/// Render a directory entry: the folder line plus (when expanded) its children.
fn render_dir(entry: &fs::DirEntry, row: &Row<'_>, ctx: &TreeContext<'_>, output: &mut String) {
    let is_open = ctx.open_set.contains(row.entry_path);
    let folder_desc = ctx.desc_map.get(row.entry_path).map(|d| &d.description);
    let triangle = if is_open { "▼ " } else { "▶ " };

    if is_open {
        if let Some(desc) = folder_desc {
            let _r = writeln!(output, "{}{}{triangle}{}/  - {desc}", row.prefix, row.connector, row.name_str);
        } else {
            let _r = writeln!(output, "{}{}{triangle}{}/", row.prefix, row.connector, row.name_str);
        }
        let child_node = TreeNode {
            dir: &entry.path(),
            path_str: row.entry_path,
            prefix: &format!("{}{}", row.prefix, row.child_prefix),
        };
        build_tree_new(&child_node, ctx, output);
    } else if let Some(desc) = folder_desc {
        let _r = writeln!(output, "{}{}{triangle}{}/ - {desc}", row.prefix, row.connector, row.name_str);
    } else {
        let _r = writeln!(output, "{}{}{triangle}{}/ ", row.prefix, row.connector, row.name_str);
    }
}

/// Render a file entry: description line (with `[!]` stale marker) or plain name.
fn render_file(entry: &fs::DirEntry, row: &Row<'_>, ctx: &TreeContext<'_>, output: &mut String) {
    if let Some(desc) = ctx.desc_map.get(row.entry_path) {
        let current_hash = compute_file_hash(&entry.path()).unwrap_or_default();
        let is_stale = !desc.file_hash.is_empty() && desc.file_hash != current_hash;
        let stale_marker = if is_stale { " [!]" } else { "" };
        let _r =
            writeln!(output, "{}{}{}{} - {}", row.prefix, row.connector, row.name_str, stale_marker, desc.description);
    } else {
        let _r = writeln!(output, "{}{}{}", row.prefix, row.connector, row.name_str);
    }
}

/// Recursively build the tree output string for a single directory node.
fn build_tree_new(node: &TreeNode<'_>, ctx: &TreeContext<'_>, output: &mut String) {
    let Ok(entries) = fs::read_dir(node.dir) else { return };

    let mut items: Vec<_> = entries
        .filter_map(Result::ok)
        .filter(|e| {
            let path = e.path();
            let is_dir = path.is_dir();
            // .context-pilot/ is internal rigging — hide unless explicitly opted in
            if is_dir && e.file_name() == ".context-pilot" && !*SHOW_CONTEXT_PILOT {
                return false;
            }
            ctx.gitignore.as_ref().is_none_or(|gi| !gi.matched(&path, is_dir).is_ignore())
        })
        .collect();

    // Sort: directories first, then alphabetically
    items.sort_by(|a, b| {
        let a_dir = a.path().is_dir();
        let b_dir = b.path().is_dir();
        match (a_dir, b_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.file_name().cmp(&b.file_name()),
        }
    });

    let total = items.len();
    for (i, entry) in items.iter().enumerate() {
        let is_last = i == total.saturating_sub(1);
        let connector = if is_last { "└── " } else { "├── " };
        let child_prefix = if is_last { "    " } else { "│   " };

        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let entry_path =
            if node.path_str == "." { name_str.to_string() } else { format!("{}/{name_str}", node.path_str) };

        let row = Row { prefix: node.prefix, connector, child_prefix, name_str: &name_str, entry_path: &entry_path };

        if entry.path().is_dir() {
            render_dir(entry, &row, ctx, output);
        } else {
            render_file(entry, &row, ctx, output);
        }
    }
}
