//! Prompt tick CSV dumper for debugging cache behavior.
//!
//! Writes every message in the assembled prompt to a CSV file at each tick.
//! Rolling deletion keeps only the 20 most recent files.

use crate::llms::{ApiMessage, ContentBlock};

/// Rolling cleanup: keep only the 20 most recent CSVs in `dir`.
fn rolling_cleanup_csvs(dir: &std::path::Path) {
    let Ok(mut entries) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<std::path::PathBuf> = entries
        .by_ref()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("csv"))
        .collect();
    if files.len() < 20 {
        return;
    }
    files.sort();
    for old in files.iter().take(files.len().saturating_sub(19)) {
        let _del = std::fs::remove_file(old);
    }
}

/// Classify one content block into `(block_type, context, raw_text)` for the CSV.
fn block_to_row<'blk>(block: &'blk ContentBlock, role: &str) -> (&'static str, String, &'blk str) {
    match block {
        ContentBlock::Text { text } => ("text", classify_text_context(text, role), text.as_str()),
        ContentBlock::ToolUse { id, name, .. } => {
            let ctx = if name == "dynamic_panel" { format!("panel_call:{id}") } else { format!("tool_use:{name}") };
            ("tool_use", ctx, name.as_str())
        }
        ContentBlock::ToolResult { tool_use_id, content } => {
            ("tool_result", tool_result_context(tool_use_id, content), content.as_str())
        }
    }
}

/// Build the context label for a `ToolResult` block (panel-aware).
fn tool_result_context(tool_use_id: &str, content: &str) -> String {
    if !tool_use_id.starts_with("panel_") {
        return format!("tool_result:{tool_use_id}");
    }
    let panel_info =
        content.lines().next().unwrap_or("").trim_start_matches("======= [").split(']').next().unwrap_or(tool_use_id);
    format!("panel_result:{panel_info}")
}

/// Dump every message in the assembled prompt to a CSV file for debugging.
///
/// Each tick writes a new file to `.context-pilot/prompt_ticks/` named by
/// datetime (second precision). Rolling deletion keeps only the 20 most recent.
pub(crate) fn dump_prompt_tick_csv(api_messages: &[ApiMessage]) {
    struct CsvRow {
        hash: String,
        role: String,
        block_type: &'static str,
        context: String,
        preview: String,
        tokens: usize,
    }

    let mut row_data: Vec<CsvRow> = Vec::new();

    let dir = std::path::Path::new(".context-pilot").join("prompt_ticks");
    let _mkdir = std::fs::create_dir_all(&dir);

    rolling_cleanup_csvs(&dir);

    // Filename: datetime with second precision
    let ts = cp_mod_utilities::time::now_local_ymd_hms_file();
    let path = dir.join(format!("{ts}.csv"));

    for msg in api_messages {
        for block in &msg.content {
            let (block_type, context, raw_text) = block_to_row(block, &msg.role);

            let full_hash = crate::state::cache::hash_content(raw_text);
            let short_hash = full_hash.get(..16).unwrap_or(&full_hash).to_owned();
            let tokens = cp_base::state::context::estimate_tokens(raw_text);

            let preview: String = raw_text
                .chars()
                .take(60)
                .map(|c| if c == ',' || c == '\n' || c == '\r' || c == '"' { ' ' } else { c })
                .collect();

            row_data.push(CsvRow { hash: short_hash, role: msg.role.clone(), block_type, context, preview, tokens });
        }
    }

    // Second pass: compute accumulated and reverse-accumulated token counts
    let total_tokens: usize = row_data.iter().map(|r| r.tokens).sum();
    let mut acc: usize = 0;
    let mut rows: Vec<String> = vec!["hash,role,type,context,tokens,acc_tokens,rev_acc_tokens,preview".to_owned()];

    for row in &row_data {
        acc = acc.saturating_add(row.tokens);
        let rev_acc = total_tokens.saturating_sub(acc);
        rows.push(format!(
            "{},{},{},{},{},{},{},{}",
            row.hash, row.role, row.block_type, row.context, row.tokens, acc, rev_acc, row.preview
        ));
    }

    let csv_content = rows.join("\n");
    let _write = std::fs::write(&path, csv_content.as_bytes());
}

/// Classify a text block's context based on content and role.
fn classify_text_context(text: &str, role: &str) -> String {
    // Panel header (first text in the panel injection sequence)
    if text.contains("Beginning of dynamic panel display") {
        return "panel_header".to_owned();
    }
    // Panel timestamp lines
    if text.starts_with("Panel automatically generated at") {
        return "panel_timestamp".to_owned();
    }
    // Panel footer
    if text.contains("End of dynamic panel display") {
        return "panel_footer".to_owned();
    }
    // Seed re-injection header
    if text.contains("System instructions") {
        return "seed_reinjection".to_owned();
    }
    // Seed re-injection ack
    if role == "assistant" && text.contains("Understood") && text.len() < 100 {
        return "seed_ack".to_owned();
    }
    // Footer ack
    if role == "user" && text.contains("Proceeding with conversation") {
        return "footer_ack".to_owned();
    }
    // Conversation messages
    if role == "user" { "conversation:user".to_owned() } else { "conversation:assistant".to_owned() }
}
