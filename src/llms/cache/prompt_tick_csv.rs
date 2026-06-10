//! Prompt tick CSV dumper for debugging cache behavior.
//!
//! Writes every message in the assembled prompt to a CSV file at each tick.
//! Rolling deletion keeps only the 20 most recent files.

use crate::llms::{ApiMessage, ContentBlock};

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

    // Rolling cleanup: keep only 20 most recent CSVs
    if let Ok(mut entries) = std::fs::read_dir(&dir) {
        let mut files: Vec<std::path::PathBuf> = entries
            .by_ref()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("csv"))
            .collect();
        if files.len() >= 20 {
            files.sort();
            for old in files.iter().take(files.len().saturating_sub(19)) {
                let _del = std::fs::remove_file(old);
            }
        }
    }

    // Filename: datetime with second precision
    let ts = cp_mod_utilities::time::now_local_ymd_hms_file();
    let path = dir.join(format!("{ts}.csv"));

    for msg in api_messages {
        for block in &msg.content {
            let (block_type, context, raw_text) = match block {
                ContentBlock::Text { text } => {
                    let ctx = classify_text_context(text, &msg.role);
                    ("text", ctx, text.as_str())
                }
                ContentBlock::ToolUse { id, name, .. } => {
                    let ctx =
                        if name == "dynamic_panel" { format!("panel_call:{id}") } else { format!("tool_use:{name}") };
                    ("tool_use", ctx, name.as_str())
                }
                ContentBlock::ToolResult { tool_use_id, content } => {
                    let ctx = if tool_use_id.starts_with("panel_") {
                        let panel_info = content
                            .lines()
                            .next()
                            .unwrap_or("")
                            .trim_start_matches("======= [")
                            .split(']')
                            .next()
                            .unwrap_or(tool_use_id);
                        format!("panel_result:{panel_info}")
                    } else {
                        format!("tool_result:{tool_use_id}")
                    };
                    ("tool_result", ctx, content.as_str())
                }
            };

            let full_hash = crate::state::cache::hash_content(raw_text);
            let short_hash = full_hash.get(..16).unwrap_or(&full_hash).to_string();
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
    let mut rows: Vec<String> = vec!["hash,role,type,context,tokens,acc_tokens,rev_acc_tokens,preview".to_string()];

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
        return "panel_header".to_string();
    }
    // Panel timestamp lines
    if text.starts_with("Panel automatically generated at") {
        return "panel_timestamp".to_string();
    }
    // Panel footer
    if text.contains("End of dynamic panel display") {
        return "panel_footer".to_string();
    }
    // Seed re-injection header
    if text.contains("System instructions") {
        return "seed_reinjection".to_string();
    }
    // Seed re-injection ack
    if role == "assistant" && text.contains("Understood") && text.len() < 100 {
        return "seed_ack".to_string();
    }
    // Footer ack
    if role == "user" && text.contains("Proceeding with conversation") {
        return "footer_ack".to_string();
    }
    // Conversation messages
    if role == "user" { "conversation:user".to_string() } else { "conversation:assistant".to_string() }
}
