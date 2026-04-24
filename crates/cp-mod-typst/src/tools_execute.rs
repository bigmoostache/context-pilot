//! `typst_execute` tool — unified typst CLI gateway.
//!
//! Handles all subcommands: compile, init, fonts, query, update.

use cp_base::state::context::{Entry, Kind};
use cp_base::state::runtime::State;
use cp_base::tools::{ToolResult, ToolUse};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::cli_parser::{self, TypstCommand};
use crate::packages;
use std::fmt::Write as _;

/// Helper to build a `ToolResult` from a tool and content.
fn ok_result(tool: &ToolUse, content: String) -> ToolResult {
    ToolResult { tool_use_id: tool.id.clone(), content, display: None, is_error: false, tool_name: tool.name.clone() }
}

/// Helper to build an error `ToolResult` from a tool and content.
fn err_result(tool: &ToolUse, content: String) -> ToolResult {
    ToolResult { tool_use_id: tool.id.clone(), content, display: None, is_error: true, tool_name: tool.name.clone() }
}

/// Execute the `typst_execute` tool — parse command string and dispatch to subcommand handler.
pub(crate) fn execute_typst(tool: &ToolUse, state: &mut State) -> ToolResult {
    let command = match tool.input.get("command").and_then(|v| v.as_str()) {
        Some(c) => c.to_string(),
        None => return err_result(tool, "Error: missing required 'command' parameter".to_string()),
    };

    // Parse the command
    let parsed = match cli_parser::parse_command(&command) {
        Ok(cmd) => cmd,
        Err(e) => return err_result(tool, format!("Error: {e}")),
    };

    // Dispatch to subcommand handler
    match parsed {
        TypstCommand::Compile { input, output, .. } => exec_compile(tool, &input, output.as_deref()),
        TypstCommand::Init { template, directory } => exec_init(tool, state, &template, directory.as_deref()),
        TypstCommand::Fonts { variants } => exec_fonts(tool, state, variants),
        TypstCommand::Query { input, selector, .. } => exec_query(tool, state, &input, &selector),
        TypstCommand::Update { package } => exec_update(tool, package.as_deref()),
        TypstCommand::Watch { input, output } => exec_watch(tool, &input, output.as_deref()),
        TypstCommand::Unwatch { input } => exec_unwatch(tool, &input),
        TypstCommand::Watchlist => exec_watchlist(tool),
    }
}

/// Subcommand: compile — compile .typ to PDF via embedded compiler.
fn exec_compile(tool: &ToolUse, input: &str, output: Option<&str>) -> ToolResult {
    // Default output: same name with .pdf extension
    let output_path = output.map_or_else(
        || {
            let p = Path::new(input);
            p.with_extension("pdf").to_string_lossy().to_string()
        },
        ToString::to_string,
    );

    match crate::compiler::compile_and_write(input, &output_path) {
        Ok(msg) => ok_result(tool, msg),
        Err(e) => err_result(tool, format!("Compile error:\n{e}")),
    }
}

/// Subcommand: init — download template from Typst Universe and scaffold project.
fn exec_init(tool: &ToolUse, _state: &mut State, template_spec: &str, directory: Option<&str>) -> ToolResult {
    // Parse the template spec (e.g., @preview/graceful-genetics:0.2.0)
    let spec = match packages::PackageSpec::parse(template_spec) {
        Ok(s) => s,
        Err(e) => {
            return err_result(
                tool,
                format!("Error: {e}\nUsage: typst init @preview/template-name:version [directory]"),
            );
        }
    };

    // Download/resolve the package
    let pkg_dir = match packages::resolve_package(&spec) {
        Ok(d) => d,
        Err(e) => return err_result(tool, format!("Error downloading package: {e}")),
    };

    // Determine target directory
    let target_dir = match directory {
        Some(d) => d.to_string(),
        None => spec.name.clone(),
    };

    // Create the target directory
    if let Err(e) = fs::create_dir_all(&target_dir) {
        return err_result(tool, format!("Error creating directory '{target_dir}': {e}"));
    }

    // Look for a template entry point in the package
    // Typst templates usually have a template/main.typ or just main.typ
    let template_main = find_template_main(&pkg_dir);
    let mut files_copied = Vec::new();

    if let Some(template_dir) = template_main {
        // Copy the template directory contents to the target
        copy_template_files(&template_dir, Path::new(&target_dir), &mut files_copied);
    } else {
        // No template dir found — create a basic .typ file that imports the package
        let main_content = format!("#import \"{}\": *\n\n// Your content here\n", spec.to_spec_string());
        let main_path = format!("{target_dir}/main.typ");
        if let Err(e) = fs::write(&main_path, &main_content) {
            return err_result(tool, format!("Error writing {main_path}: {e}"));
        }
        files_copied.push("main.typ".to_string());
    }

    let mut result = format!(
        "✓ Initialized {} from {}\n  Directory: {}/\n  Package cached at: {}\n\nFiles:\n",
        spec.name,
        spec.to_spec_string(),
        target_dir,
        pkg_dir.display()
    );
    for f in &files_copied {
        let _r = writeln!(result, "  {f}");
    }

    ok_result(tool, result)
}

/// Subcommand: fonts — list available system fonts.
/// Read-only → creates a dynamic context panel.
fn exec_fonts(tool: &ToolUse, state: &mut State, variants: bool) -> ToolResult {
    // Discover fonts using the same logic as the compiler
    let font_dirs = [
        PathBuf::from("/usr/share/fonts"),
        PathBuf::from("/usr/local/share/fonts"),
        crate::compiler::dirs_home().map(|h| h.join(".fonts")).unwrap_or_default(),
        crate::compiler::dirs_home().map(|h| h.join(".local/share/fonts")).unwrap_or_default(),
    ];

    let mut font_entries: Vec<(String, String, String)> = Vec::new(); // (family, style, path)

    for dir in &font_dirs {
        if dir.is_dir() {
            collect_font_info(dir, &mut font_entries);
        }
    }

    // Sort and deduplicate
    font_entries.sort();

    // Build output
    let mut output = String::new();
    if variants {
        // Show all variants (family + style)
        let mut seen = HashSet::new();
        for (family, style, _path) in &font_entries {
            let key = format!("{family} — {style}");
            if seen.insert(key.clone()) {
                output.push_str(&key);
                output.push('\n');
            }
        }
    } else {
        // Show unique font families only
        let mut families: Vec<String> = font_entries.iter().map(|(f, _, _)| f.clone()).collect();
        families.sort();
        families.dedup();
        for family in &families {
            output.push_str(family);
            output.push('\n');
        }
    }

    let count = if variants {
        font_entries.len()
    } else {
        let mut families: Vec<String> = font_entries.iter().map(|(f, _, _)| f.clone()).collect();
        families.sort();
        families.dedup();
        families.len()
    };

    let header = format!("=== Typst Fonts ({} {}) ===\n\n", count, if variants { "variants" } else { "families" });

    // Create a dynamic context panel for the result
    let panel_content = format!("{header}{output}");
    let context_id = state.next_available_context_id();
    let uid = format!("UID_{}_P", state.global_next_uid);
    state.global_next_uid = state.global_next_uid.saturating_add(1);

    let mut elem = Entry {
        id: context_id.clone(),
        uid: Some(uid),
        context_type: Kind::new("typst_result"),
        name: "Typst Fonts".to_string(),
        token_count: 0,
        metadata: std::collections::HashMap::new(),
        cached_content: Some(panel_content),
        history_messages: None,
        cache_deprecated: false,
        cache_in_flight: false,
        last_refresh_ms: cp_base::panels::now_ms(),
        content_hash: None,
        source_hash: None,
        current_page: 0,
        total_pages: 1,
        full_token_count: 0,
        panel_cache_hit: false,
        panel_total_cost: 0.0,
        freeze_count: 0,
        total_freezes: 0,
        total_cache_misses: 0,
        last_emitted_content: None,
        last_emitted_hash: None,
        last_emitted_context: None,
    };
    elem.set_meta("dynamic_label", &"typst-fonts".to_string());
    state.context.push(elem);

    ok_result(
        tool,
        format!(
            "Found {} font {}. Results shown in panel {}.",
            count,
            if variants { "variants" } else { "families" },
            context_id
        ),
    )
}

/// Subcommand: query — query document metadata/labels.
/// Read-only → creates a dynamic context panel.
fn exec_query(tool: &ToolUse, state: &mut State, input: &str, selector: &str) -> ToolResult {
    // Compile the document to get metadata
    let abs_path = match PathBuf::from(input).canonicalize() {
        Ok(p) => p,
        Err(e) => return err_result(tool, format!("Error: cannot resolve '{input}': {e}")),
    };

    // For now, return a basic message about query support
    // Full query support requires compiling + introspecting the document
    let result = format!(
        "Query support is limited in the embedded compiler.\n\
         Input: {}\n\
         Selector: {}\n\
         \n\
         To query document metadata, compile the document first with 'typst compile' \
         and inspect the output. For label queries, use Typst's built-in #metadata() + #label() system.",
        abs_path.display(),
        selector
    );

    // Create a dynamic panel for the result
    let context_id = state.next_available_context_id();
    let uid = format!("UID_{}_P", state.global_next_uid);
    state.global_next_uid = state.global_next_uid.saturating_add(1);

    let mut elem = Entry {
        id: context_id.clone(),
        uid: Some(uid),
        context_type: Kind::new("typst_result"),
        name: format!("Typst Query: {selector}"),
        token_count: 0,
        metadata: std::collections::HashMap::new(),
        cached_content: Some(result),
        history_messages: None,
        cache_deprecated: false,
        cache_in_flight: false,
        last_refresh_ms: cp_base::panels::now_ms(),
        content_hash: None,
        source_hash: None,
        current_page: 0,
        total_pages: 1,
        full_token_count: 0,
        panel_cache_hit: false,
        panel_total_cost: 0.0,
        freeze_count: 0,
        total_freezes: 0,
        total_cache_misses: 0,
        last_emitted_content: None,
        last_emitted_hash: None,
        last_emitted_context: None,
    };
    elem.set_meta("dynamic_label", &"typst-query".to_string());
    state.context.push(elem);

    ok_result(tool, format!("Query result shown in panel {context_id}."))
}

/// Subcommand: update — re-download cached packages.
fn exec_update(tool: &ToolUse, package: Option<&str>) -> ToolResult {
    if let Some(pkg_spec) = package {
        // Update a specific package
        let spec = match packages::PackageSpec::parse(pkg_spec) {
            Ok(s) => s,
            Err(e) => return err_result(tool, format!("Error: {e}")),
        };

        // Remove cached version and re-download
        let cache_dir = spec.cache_dir();
        if cache_dir.exists() {
            let _r = fs::remove_dir_all(&cache_dir);
        }

        match packages::download_package(&spec) {
            Ok(()) => ok_result(
                tool,
                format!("✓ Updated {} (re-downloaded to {})", spec.to_spec_string(), cache_dir.display()),
            ),
            Err(e) => err_result(tool, format!("Error updating {}: {}", spec.to_spec_string(), e)),
        }
    } else {
        // List all cached packages
        let cached = packages::list_cached();
        if cached.is_empty() {
            return ok_result(
                tool,
                "No cached packages to update.\nUse 'typst init @preview/package:version' to download packages."
                    .to_string(),
            );
        }

        let mut result = format!("Cached packages ({}):\n", cached.len());
        for (ns, name, ver) in &cached {
            let _r = writeln!(result, "  @{ns}/{name}:{ver}");
        }
        result.push_str("\nUse 'typst update @preview/name:version' to re-download a specific package.");
        ok_result(tool, result)
    }
}

// ============================================================================
// Helper functions for typst_execute subcommands
// ============================================================================

/// Find the template entry point in a package directory.
fn find_template_main(pkg_dir: &Path) -> Option<PathBuf> {
    // Check template/ subdirectory first (Typst convention)
    let template_dir = pkg_dir.join("template");
    if template_dir.is_dir() {
        return Some(template_dir);
    }

    // Check for a lib.typ or main.typ at root
    if pkg_dir.join("main.typ").exists() || pkg_dir.join("lib.typ").exists() {
        return None; // Package root is the import target, not a template
    }

    None
}

/// Copy files from a template directory to a target directory.
fn copy_template_files(src: &Path, dst: &Path, files: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(src) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if path.is_dir() {
            let sub_dst = dst.join(&name);
            let _r = fs::create_dir_all(&sub_dst);
            copy_template_files(&path, &sub_dst, files);
        } else {
            let dst_file = dst.join(&name);
            if let Ok(content) = fs::read(&path)
                && fs::write(&dst_file, &content).is_ok()
            {
                files.push(name);
            }
        }
    }
}

/// Collect font info from a directory recursively.
fn collect_font_info(dir: &Path, entries: &mut Vec<(String, String, String)>) {
    use typst::foundations::Bytes;
    use typst::text::FontInfo;

    let Ok(dir_entries) = fs::read_dir(dir) else { return };
    for entry in dir_entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_font_info(&path, entries);
        } else if crate::compiler::is_font_file(&path)
            && let Ok(data) = fs::read(&path)
        {
            let bytes = Bytes::new(data);
            for info in FontInfo::iter(&bytes) {
                let family = info.family.clone();
                let style = format!("{:?}", info.variant);
                entries.push((family, style, path.to_string_lossy().to_string()));
            }
        }
    }
}

/// Subcommand: watch — add a .typ document to the auto-compile watchlist.
/// Compiles immediately and records dependency tree for future change detection.
fn exec_watch(tool: &ToolUse, input: &str, output: Option<&str>) -> ToolResult {
    // Default output: same name with .pdf extension
    let output_path = output.map_or_else(
        || {
            let p = Path::new(input);
            p.with_extension("pdf").to_string_lossy().to_string()
        },
        ToString::to_string,
    );

    // Compile + update deps in the watchlist
    match crate::watchlist::compile_and_update_deps(input, &output_path) {
        Ok(msg) => {
            let watchlist = crate::watchlist::Watchlist::load();
            let dep_count = watchlist.entries.get(input).map_or(0, |e| e.deps.len());
            ok_result(
                tool,
                format!(
                    "✓ Now watching '{input}' → '{output_path}'\n{msg}\n\nThe document and its {dep_count} dependencies will auto-recompile when any dependency is edited."
                ),
            )
        }
        Err(e) => err_result(tool, format!("Error compiling '{input}': {e}\nDocument was NOT added to watchlist.")),
    }
}

/// Subcommand: unwatch — remove a document from the auto-compile watchlist.
fn exec_unwatch(tool: &ToolUse, input: &str) -> ToolResult {
    let mut watchlist = crate::watchlist::Watchlist::load();
    if watchlist.unwatch(input) {
        ok_result(tool, format!("✓ Removed '{input}' from watchlist. It will no longer auto-recompile."))
    } else {
        ok_result(tool, format!("'{input}' was not in the watchlist.\nUse 'typst watchlist' to see current entries."))
    }
}

/// Subcommand: watchlist — list all watched documents and their dependency counts.
fn exec_watchlist(tool: &ToolUse) -> ToolResult {
    let watchlist = crate::watchlist::Watchlist::load();
    if watchlist.entries.is_empty() {
        return ok_result(tool, "No documents in watchlist.\nUse 'typst watch <file.typ>' to add one.".to_string());
    }

    let mut output = format!("Typst Watchlist ({} documents):\n\n", watchlist.entries.len());
    for (source, entry) in watchlist.list() {
        let _r = writeln!(output, "  {} → {} ({} deps)", source, entry.output, entry.deps.len());
    }
    output.push_str("\nUse 'typst unwatch <file.typ>' to remove.");
    ok_result(tool, output)
}
