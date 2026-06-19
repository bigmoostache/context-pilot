//! Conversation IR builder — assembles [`Conversation`] from application state.
//!
//! Extracts the data logic from `modules::conversation::panel` into a pure
//! function returning IR types. No ratatui, no Frame, no caching — just
//! state → data transformation. Caching lives in the adapter layer (Phase 5).

use cp_render::conversation::{
    Autocomplete, AutocompleteEntry, Conversation, HistorySection, InputArea, Message as IrMessage, Overlay,
    PerfBudgetBar, PerfMeiliStats, PerfOp, PerfOverlay, StreamingTool, ToolResultPreview, ToolUsePreview,
};
use cp_render::{Block, Semantic};

use crate::state::{Kind, MsgKind, MsgStatus, State, ToolResultRecord, ToolUseRecord};
use cp_base::cast::Safe as _;

/// Build the conversation region from application state.
#[must_use]
pub(crate) fn build_conversation(state: &State) -> Conversation {
    let history_sections = build_history_sections(state);
    let messages = build_messages(state);
    let streaming_tools = build_streaming_tools(state);
    let input = build_input(state);

    Conversation { history_sections, messages, streaming_tools, input }
}

// ── History sections ─────────────────────────────────────────────────

/// Build history sections from `ConversationHistory` context elements.
fn build_history_sections(state: &State) -> Vec<HistorySection> {
    let mut history_panels: Vec<_> =
        state.context.iter().filter(|c| c.context_type.as_str() == Kind::CONVERSATION_HISTORY).collect();
    history_panels.sort_by_key(|c| c.last_refresh_ms);

    history_panels
        .iter()
        .map(|ctx| {
            let messages = ctx
                .history_messages
                .as_ref()
                .map(|msgs| msgs.iter().filter(|m| m.status != MsgStatus::Deleted).map(msg_to_ir).collect())
                .unwrap_or_default();

            HistorySection { label: ctx.name.clone(), expanded: true, messages }
        })
        .collect()
}

// ── Messages ─────────────────────────────────────────────────────────

/// Build the visible message list from current conversation.
fn build_messages(state: &State) -> Vec<IrMessage> {
    let last_msg_id = state.messages.last().map(|m| m.id.clone());

    state
        .messages
        .iter()
        .filter(|msg| {
            if msg.status == MsgStatus::Deleted {
                return false;
            }
            // Skip empty text messages (unless currently streaming)
            let is_last = last_msg_id.as_ref() == Some(&msg.id);
            let is_streaming = state.flags.stream.phase.is_streaming() && is_last && msg.role == "assistant";
            if msg.msg_type == MsgKind::TextMessage && msg.content.trim().is_empty() && !is_streaming {
                return false;
            }
            true
        })
        .map(msg_to_ir)
        .collect()
}

/// Convert a single application Message to an IR Message.
fn msg_to_ir(msg: &crate::state::Message) -> IrMessage {
    let content = build_message_content(msg);
    let tool_uses = msg.tool_uses.iter().map(tool_use_to_ir).collect();
    let tool_results = msg.tool_results.iter().map(tool_result_to_ir).collect();

    IrMessage { role: msg.role.clone(), content, tool_uses, tool_results }
}

/// Build content blocks for a message based on its type.
fn build_message_content(msg: &crate::state::Message) -> Vec<Block> {
    match msg.msg_type {
        MsgKind::TextMessage => {
            if msg.content.is_empty() {
                Vec::new()
            } else {
                // Each line becomes a Block::Line. Markdown rendering
                // is deferred to the adapter layer (Phase 5).
                msg.content.lines().map(|line| Block::text(line.to_owned())).collect()
            }
        }
        MsgKind::ToolCall => {
            // Tool calls are represented via tool_uses, content is usually empty
            if msg.content.is_empty() { Vec::new() } else { vec![Block::text(msg.content.clone())] }
        }
        MsgKind::ToolResult => {
            // Tool results are represented via tool_results, content is usually empty
            if msg.content.is_empty() { Vec::new() } else { vec![Block::text(msg.content.clone())] }
        }
    }
}

/// Convert a [`ToolUseRecord`] to an IR [`ToolUsePreview`].
fn tool_use_to_ir(tu: &ToolUseRecord) -> ToolUsePreview {
    // Build a short summary from input parameters
    let summary: String =
        tu.input.as_object().map(|obj| obj.keys().take(3).cloned().collect::<Vec<_>>().join(", ")).unwrap_or_default();

    ToolUsePreview { tool_name: tu.name.clone(), summary, semantic: Semantic::Success }
}

/// Convert a [`ToolResultRecord`] to an IR [`ToolResultPreview`].
fn tool_result_to_ir(tr: &ToolResultRecord) -> ToolResultPreview {
    // Prefer display (user-facing) over content (LLM-facing) for the UI
    let source = tr.display.as_deref().unwrap_or(&tr.content);

    // Truncate content for summary
    let summary = if source.len() > 80 {
        let boundary = source.floor_char_boundary(77);
        format!("{}...", source.get(..boundary).unwrap_or(""))
    } else {
        source.to_string()
    };

    ToolResultPreview { tool_name: tr.tool_name.clone(), summary, success: !tr.is_error }
}

// ── Streaming tools ──────────────────────────────────────────────────

/// Build streaming tool previews from state.
fn build_streaming_tools(state: &State) -> Vec<StreamingTool> {
    state
        .streaming_tool
        .as_ref()
        .map(|st| vec![StreamingTool { tool_name: st.name.clone(), partial_input: st.input_so_far.clone() }])
        .unwrap_or_default()
}

// ── Input area ───────────────────────────────────────────────────────

/// Build the input area from state.
fn build_input(state: &State) -> InputArea {
    InputArea {
        text: state.input.clone(),
        cursor: state.input_cursor,
        placeholder: "Type a message…".into(),
        focused: !state.flags.stream.phase.is_streaming(),
    }
}

// ── Overlays ─────────────────────────────────────────────────────────

/// Build overlay stack from state (question form, autocomplete).
#[must_use]
pub(crate) fn build_overlays(state: &State) -> Vec<Overlay> {
    let mut overlays = Vec::new();

    // Autocomplete overlay
    if let Some(ac) = state.get_ext::<cp_base::state::autocomplete::Suggestions>()
        && ac.active
    {
        overlays.push(Overlay::Autocomplete(build_autocomplete(ac)));
    }

    // Config overlay
    if state.flags.config.config_view {
        overlays.push(Overlay::Config(crate::ui::help::config_overlay::build_config_overlay(state)));
    }

    // Perf overlay
    if state.flags.ui.perf_enabled {
        overlays.push(Overlay::Perf(build_perf_overlay(state)));
    }

    // Search index overlay
    if state.flags.overlays.index_status {
        overlays.push(Overlay::SearchIndex(Box::new(crate::ui::search_overlay::build_search_index_overlay(state))));
    }

    // MCP setup overlay
    if cp_mod_mcp::bridge::setup::McpSetupState::get(state).visible {
        overlays.push(Overlay::McpSetup(Box::new(
            crate::ui::help::mcp_overlay::builder::build_mcp_setup_overlay(state),
        )));
    }

    overlays
}

/// Build autocomplete from suggestions state.
fn build_autocomplete(ac: &cp_base::state::autocomplete::Suggestions) -> Autocomplete {
    let visible = ac.visible_matches();
    let selected_relative = ac.selected.saturating_sub(ac.scroll_offset);
    let entries = visible
        .iter()
        .map(|e| AutocompleteEntry {
            label: e.name.clone(),
            is_dir: e.is_dir,
            icon: if e.is_dir { "📁".into() } else { "📄".into() },
        })
        .collect();

    Autocomplete {
        query: ac.query.clone(),
        entries,
        selected_index: selected_relative,
        dir_prefix: ac.dir_prefix.clone(),
        total_matches: ac.matches.len(),
        input_visual_lines: ac.input_visual_lines,
    }
}

// ── Perf overlay ─────────────────────────────────────────────────────

/// Frame budget for 60fps (milliseconds).
const FRAME_BUDGET_60FPS: f64 = 16.67;
/// Frame budget for 30fps (milliseconds).
const FRAME_BUDGET_30FPS: f64 = 33.33;

/// Map frame time to a Semantic (green < 60fps budget, yellow < 30fps, red otherwise).
fn frame_time_semantic(ms: f64) -> Semantic {
    if ms < FRAME_BUDGET_60FPS {
        Semantic::Success
    } else if ms < FRAME_BUDGET_30FPS {
        Semantic::Warning
    } else {
        Semantic::Error
    }
}

/// Map a percentage to a Semantic (green < 25%, yellow < 50%, red otherwise).
fn cpu_semantic(pct: f64) -> Semantic {
    if pct < 25.0 {
        Semantic::Success
    } else if pct < 50.0 {
        Semantic::Warning
    } else {
        Semantic::Error
    }
}

/// Map FD usage ratio to a Semantic (green < 50%, yellow < 80%, red otherwise).
fn fd_semantic(open: u32, limit: u64) -> Semantic {
    if limit == 0 {
        return Semantic::Muted;
    }
    let pct = f64::from(open) / f64::from(u32::try_from(limit).unwrap_or(u32::MAX)) * 100.0;
    if pct < 50.0 {
        Semantic::Success
    } else if pct < 80.0 {
        Semantic::Warning
    } else {
        Semantic::Error
    }
}

/// Build the perf overlay IR data from the perf metrics snapshot.
fn build_perf_overlay(state: &State) -> PerfOverlay {
    use crate::ui::perf::PERF;

    let snapshot = PERF.snapshot();

    let fps = if snapshot.frame_avg_ms > 0.0 { 1000.0 / snapshot.frame_avg_ms } else { 0.0 };

    // Meilisearch stats
    let meili = cp_mod_search::overlay_info(state).and_then(|info| {
        if info.meili_memory_bytes == 0 && info.meili_cpu_pct <= 0.0 {
            return None;
        }
        let mb = info.meili_memory_bytes.to_f64() / (1024.0 * 1024.0);
        Some(PerfMeiliStats {
            cpu_pct: f64::from(info.meili_cpu_pct),
            cpu_semantic: cpu_semantic(f64::from(info.meili_cpu_pct)),
            memory_mb: mb,
        })
    });

    // Budget bars
    let build_bar = |label: &str, budget_ms: f64| -> PerfBudgetBar {
        let pct = (snapshot.frame_avg_ms / budget_ms * 100.0).min(150.0);
        let semantic = if pct <= 80.0 {
            Semantic::Success
        } else if pct <= 100.0 {
            Semantic::Warning
        } else {
            Semantic::Error
        };
        PerfBudgetBar { label: label.into(), percent: pct, semantic }
    };

    let budget_bars = vec![build_bar("60fps", FRAME_BUDGET_60FPS), build_bar("30fps", FRAME_BUDGET_30FPS)];

    // Operations
    let total_time: f64 = snapshot.ops.iter().map(|o| o.total_ms).sum();

    let operations = snapshot
        .ops
        .iter()
        .take(10)
        .map(|op| {
            let pct = if total_time > 0.0 { op.total_ms / total_time * 100.0 } else { 0.0 };
            let is_hotspot = pct > 30.0;

            let name = if op.name.len() <= 24 {
                op.name.to_string()
            } else {
                let tail_start = op.name.len().saturating_sub(22);
                format!("..{}", op.name.get(tail_start..).unwrap_or(""))
            };

            let total_display = if op.total_ms >= 1000.0 {
                format!("{:.1}s", op.total_ms / 1000.0)
            } else {
                format!("{:.0}ms", op.total_ms)
            };

            let std_semantic = if op.std_ms < 1.0 {
                Semantic::Success
            } else if op.std_ms < 5.0 {
                Semantic::Warning
            } else {
                Semantic::Error
            };

            PerfOp {
                name,
                mean_ms: op.mean_ms,
                mean_semantic: frame_time_semantic(op.mean_ms),
                std_ms: op.std_ms,
                std_semantic,
                total_display,
                is_hotspot,
            }
        })
        .collect();

    PerfOverlay {
        fps,
        frame_avg_ms: snapshot.frame_avg_ms,
        frame_max_ms: snapshot.frame_max_ms,
        frame_semantic: frame_time_semantic(snapshot.frame_avg_ms),
        cpu_usage: snapshot.cpu_usage,
        cpu_semantic: cpu_semantic(f64::from(snapshot.cpu_usage)),
        memory_mb: snapshot.memory_mb,
        open_fds: snapshot.open_fds,
        fd_limit_soft: snapshot.fd_limit_soft,
        fd_semantic: fd_semantic(snapshot.open_fds, snapshot.fd_limit_soft),
        meili,
        budget_bars,
        sparkline: snapshot.frame_times_ms,
        operations,
    }
}
