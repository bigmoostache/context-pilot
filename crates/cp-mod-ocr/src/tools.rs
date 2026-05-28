//! OCR tool execution.
//!
//! Dispatches the `ocr` tool call, validates parameters, and spawns
//! an async worker thread that calls the Datalab API and writes the
//! result to the output file.

use std::path::PathBuf;

use cp_base::state::runtime::State;
use cp_base::tools::async_exec::{ToolOutput, spawn_async_tool};
use cp_base::tools::{ToolResult, ToolUse};

use crate::client::{DatalabClient, OcrMode, api_key_from_env, is_ocr_extension};

/// Async timeout for OCR API calls (seconds).
///
/// Datalab can take several minutes for large documents. The client
/// has its own 300 s poll timeout; this covers submit + poll + margin.
const ASYNC_TIMEOUT_SECS: u64 = 360;

/// Dispatch OCR tool calls.
pub(crate) fn dispatch(tool: &ToolUse, state: &mut State) -> Option<ToolResult> {
    (tool.name == "ocr").then(|| execute_ocr(tool, state))
}

/// Execute the `ocr` tool.
fn execute_ocr(tool: &ToolUse, state: &mut State) -> ToolResult {
    // --- Validate DATALAB_API_KEY ---
    let Some(api_key) = api_key_from_env() else {
        return err(tool, "DATALAB_API_KEY not set in environment.".to_string());
    };

    // --- Validate path ---
    let Some(path_str) = tool.input.get("path").and_then(|v| v.as_str()) else {
        return err(tool, "Missing required parameter 'path'.".to_string());
    };
    let path = PathBuf::from(path_str);
    if !path.exists() {
        return err(tool, format!("File not found: '{path_str}'"));
    }

    let ext = path.extension().and_then(std::ffi::OsStr::to_str).unwrap_or("");
    if !is_ocr_extension(ext) {
        return err(
            tool,
            format!("Unsupported file type '.{ext}'. Supported: pdf, png, jpg, jpeg, tiff, webp, bmp, heic, gif."),
        );
    }

    // --- Validate mode ---
    let Some(mode_str) = tool.input.get("mode").and_then(|v| v.as_str()) else {
        return err(tool, "Missing required parameter 'mode'. Use 'markdown' or 'text_boxes'.".to_string());
    };
    let mode = match OcrMode::from_str(mode_str) {
        Ok(m) => m,
        Err(e) => return err(tool, e),
    };

    // --- Validate output ---
    let Some(output_str) = tool.input.get("output").and_then(|v| v.as_str()) else {
        return err(tool, "Missing required parameter 'output'.".to_string());
    };
    let output_path = PathBuf::from(output_str);

    // Ensure parent directory exists.
    if let Some(parent) = output_path.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return err(tool, format!("Cannot create output directory '{}': {e}", parent.display()));
    }

    // --- Spawn async worker ---
    let path_display = path_str.to_string();
    let output_display = output_str.to_string();

    spawn_async_tool(state, tool, ASYNC_TIMEOUT_SECS, move || {
        let client = match DatalabClient::new(&api_key) {
            Ok(c) => c,
            Err(e) => {
                return ToolOutput {
                    content: format!("Failed to create OCR client: {e}"),
                    is_error: true,
                    create_panel: None,
                    preserves_tempo: false,
                };
            }
        };

        match client.convert(&path, mode) {
            Ok(result) => {
                // Write result to output file.
                if let Err(e) = std::fs::write(&output_path, &result.text) {
                    return ToolOutput {
                        content: format!("OCR succeeded but failed to write output to '{output_display}': {e}",),
                        is_error: true,
                        create_panel: None,
                        preserves_tempo: false,
                    };
                }

                let cached_note = if result.cached { " (from cache)" } else { "" };
                let char_count = result.text.len();

                // Build a short preview for the tool result.
                let preview = if result.text.is_empty() {
                    "(empty — no text extracted)".to_string()
                } else {
                    let truncated: String = result.text.chars().take(300).collect();
                    let ellipsis = if result.text.len() > 300 { "…" } else { "" };
                    format!("Preview:\n{truncated}{ellipsis}")
                };

                ToolOutput {
                    content: format!(
                        "✅ OCR complete{cached_note}. {char_count} chars written to '{output_display}'.\n\n{preview}",
                    ),
                    is_error: false,
                    create_panel: None,
                    preserves_tempo: false,
                }
            }
            Err(e) => ToolOutput {
                content: format!("❌ OCR failed for '{path_display}': {e}"),
                is_error: true,
                create_panel: None,
                preserves_tempo: false,
            },
        }
    })
}

/// Build an error `ToolResult`.
fn err(tool: &ToolUse, content: String) -> ToolResult {
    ToolResult {
        tool_use_id: tool.id.clone(),
        content,
        display: None,
        tldr: None,
        is_error: true,
        preserves_tempo: false,
        tool_name: tool.name.clone(),
    }
}
