//! CLI subcommands for Typst compilation.
//!
//! These run as one-shot processes (never in TUI mode). Both functions return
//! `Result<String, (String, i32)>` — the caller in `main.rs` handles stdout
//! printing and process exit.

/// Run the typst-compile subcommand: compile a .typ file to PDF in the same directory.
/// Used by the typst-compile callback via `$CP_CHANGED_FILES`.
///
/// # Errors
///
/// Returns `Err((message, exit_code))` on missing args or compilation failure.
pub(crate) fn run_typst_compile(args: &[String]) -> Result<String, (String, i32)> {
    let Some(source_path) = args.first() else {
        return Err(("Usage: cpilot typst-compile <source.typ>".to_string(), 1));
    };

    // Output: same directory, same name, .pdf extension
    let stem = std::path::Path::new(source_path).file_stem().and_then(|s| s.to_str()).unwrap_or("output");
    let parent =
        std::path::Path::new(source_path).parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    let out = if parent.is_empty() { format!("{stem}.pdf") } else { format!("{parent}/{stem}.pdf") };

    cp_mod_typst::compiler::compile_and_write(source_path, &out).map_err(|err| (err, 1))
}

/// Recompile all watched .typ documents whose dependencies include any of the changed files.
/// Used by the typst-watchlist callback via `$CP_CHANGED_FILES`.
///
/// # Errors
///
/// Returns `Err((message, 7))` when no documents are affected (silent success for callback),
/// or `Err((message, 1))` on compilation failure.
pub(crate) fn run_typst_recompile_watched(args: &[String]) -> Result<String, (String, i32)> {
    if args.is_empty() {
        // Exit 7 = "nothing to do" — callback system treats this as silent success
        return Err((String::new(), 7));
    }

    let watchlist = cp_mod_typst::watchlist::Watchlist::load();
    if watchlist.entries.is_empty() {
        return Err((String::new(), 7));
    }

    // Find all watched documents affected by the changed files
    let mut affected: Vec<(String, String)> = Vec::new();
    for changed_file in args {
        if changed_file.is_empty() {
            continue;
        }
        for (source, output) in watchlist.find_affected(changed_file) {
            if !affected.iter().any(|(s, _)| s == &source) {
                affected.push((source, output));
            }
        }
    }

    if affected.is_empty() {
        // Exit 7 = "nothing to do" — callback system treats this as silent success
        return Err((String::new(), 7));
    }

    // Recompile each affected document (and update deps)
    let mut output_lines = Vec::new();
    let mut error_lines = Vec::new();
    for (source, output) in &affected {
        match cp_mod_typst::watchlist::compile_and_update_deps(source, output) {
            Ok(msg) => output_lines.push(msg),
            Err(err) => error_lines.push(format!("Error compiling {source}: {err}")),
        }
    }

    if error_lines.is_empty() {
        Ok(output_lines.join("\n"))
    } else {
        // Include any successful compilations in the error output too
        let mut combined = output_lines;
        combined.extend(error_lines);
        Err((combined.join("\n"), 1))
    }
}
