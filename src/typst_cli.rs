//! CLI subcommands for Typst compilation.
//!
//! These run as one-shot processes (never in TUI mode), so stdout/stderr
//! printing and `process::exit` are the expected interface.
#![expect(
    clippy::print_stdout,
    clippy::exit,
    reason = "CLI subcommands — stdout output and process::exit are the normal interface"
)]

use std::io::Write;

/// Run the typst-compile subcommand: compile a .typ file to PDF in the same directory.
/// Used by the typst-compile callback via $`CP_CHANGED_FILES`.
/// Usage: cpilot typst-compile <source.typ>
pub(crate) fn run_typst_compile(args: &[String]) {
    if args.is_empty() {
        drop(writeln!(std::io::stderr(), "Usage: cpilot typst-compile <source.typ>"));
        std::process::exit(1);
    }

    let source_path = &args[0];

    // Output: same directory, same name, .pdf extension
    let stem = std::path::Path::new(source_path).file_stem().and_then(|s| s.to_str()).unwrap_or("output");
    let parent =
        std::path::Path::new(source_path).parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    let out = if parent.is_empty() { format!("{stem}.pdf") } else { format!("{parent}/{stem}.pdf") };

    match cp_mod_typst::compiler::compile_and_write(source_path, &out) {
        Ok(msg) => println!("{msg}"),
        Err(err) => {
            drop(writeln!(std::io::stderr(), "{err}"));
            std::process::exit(1);
        }
    }
}

/// Recompile all watched .typ documents whose dependencies include any of the changed files.
/// Used by the typst-watchlist callback via $`CP_CHANGED_FILES`.
/// Usage: cpilot typst-recompile-watched <`changed_file1`> [`changed_file2` ...]
pub(crate) fn run_typst_recompile_watched(args: &[String]) {
    if args.is_empty() {
        return;
    }

    let watchlist = cp_mod_typst::watchlist::Watchlist::load();
    if watchlist.entries.is_empty() {
        return;
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
        std::process::exit(7);
    }

    // Recompile each affected document (and update deps)
    let mut had_error = false;
    for (source, output) in &affected {
        match cp_mod_typst::watchlist::compile_and_update_deps(source, output) {
            Ok(msg) => println!("{msg}"),
            Err(err) => {
                drop(writeln!(std::io::stderr(), "Error compiling {source}: {err}"));
                had_error = true;
            }
        }
    }

    if had_error {
        std::process::exit(1);
    }
}
