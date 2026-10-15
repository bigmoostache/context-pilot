/// Unified diff generation for displaying edit results.
pub(crate) mod diff;
/// Edit tool: `old_string`/`new_string` replacement in files.
pub(crate) mod edit_file;
/// Open tool: read a file into the context panel.
pub(crate) mod file;
/// Locate `old_string` exactly once (literal, then whitespace-tolerant) and splice.
pub(crate) mod matcher;
/// Write tool: create or fully overwrite a file.
pub(crate) mod write;
