//! Conversation IR builder — assembles [`Conversation`] from application state.
//!
//! Extracts the data logic from `modules::conversation::panel` into a pure
//! function returning IR types. No ratatui, no Frame, no caching — just
//! state → data transformation. Caching lives in the adapter layer (Phase 5).

use cp_render::conversation::PerfShareBar;
use cp_render::conversation::{
    Autocomplete, AutocompleteEntry, Conversation, HistorySection, InputArea, Message as IrMessage, Overlay,
    PerfBudgetBar, PerfMeiliStats, PerfOp, PerfOverlay, StreamingTool, ToolResultPreview, ToolUsePreview,
};
use cp_render::{Block, Semantic};

use crate::state::{Kind, MsgKind, MsgStatus, State, ToolResultRecord, ToolUseRecord};
use cp_base::cast::Safe as _;
use cp_base::cast::float_math;

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
        state.thread().context.iter().filter(|c| c.context_type.as_str() == Kind::CONVERSATION_HISTORY).collect();
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
    let last_msg_id = state.thread().messages.last().map(|m| m.id.clone());

    state
        .thread()
        .messages
        .iter()
        .filter(|msg| {
            if msg.status == MsgStatus::Deleted {
                return false;
            }
            // Skip empty text messages (unless currently streaming)
            let is_last = last_msg_id.as_ref() == Some(&msg.id);
            let is_streaming = state.thread().stream.phase.is_streaming() && is_last && msg.role == "assistant";
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
    if msg.msg_type == MsgKind::TextMessage {
        if msg.content.is_empty() {
            Vec::new()
        } else {
            // Each line becomes a Block::Line. Markdown rendering
            // is deferred to the adapter layer (Phase 5).
            msg.content.lines().map(|line| Block::text(line.to_owned())).collect()
        }
    } else if msg.content.is_empty() {
        // Tool calls / results carry their payload in tool_uses / tool_results;
        // content is usually empty.
        Vec::new()
    } else {
        vec![Block::text(msg.content.clone())]
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
        source.to_owned()
    };

    ToolResultPreview { tool_name: tr.tool_name.clone(), summary, success: !tr.is_error }
}

// ── Streaming tools ──────────────────────────────────────────────────

/// Build streaming tool previews from state.
fn build_streaming_tools(state: &State) -> Vec<StreamingTool> {
    state
        .thread()
        .streaming_tool
        .as_ref()
        .map(|st| vec![StreamingTool { tool_name: st.name.clone(), partial_input: st.input_so_far.clone() }])
        .unwrap_or_default()
}

// ── Input area ───────────────────────────────────────────────────────

/// Build the input area from state.
fn build_input(state: &State) -> InputArea {
    InputArea {
        text: state.thread().composer.text.clone(),
        cursor: state.thread().composer.cursor,
        placeholder: "Type a message\u{2026}".into(),
        focused: !state.thread().stream.phase.is_streaming(),
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
            icon: if e.is_dir { "\u{1f4c1}".into() } else { "\u{1f4c4}".into() },
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
    let pct = float_math::percent(f64::from(open), f64::from(u32::try_from(limit).unwrap_or(u32::MAX)));
    if pct < 50.0 {
        Semantic::Success
    } else if pct < 80.0 {
        Semantic::Warning
    } else {
        Semantic::Error
    }
}

/// Build the per-operation perf rows (top 10 by total time) from a snapshot.
fn build_perf_ops(snapshot: &crate::ui::perf::PerfSnapshot) -> Vec<PerfOp> {
    let total_time: f64 = snapshot.ops.iter().map(|o| o.total_ms).sum();

    snapshot
        .ops
        .iter()
        .take(10)
        .map(|op| {
            let pct = if total_time > 0.0f64 { float_math::percent(op.total_ms, total_time) } else { 0.0f64 };
            let is_hotspot = pct > 30.0f64;

            let name = if op.name.len() <= 24 {
                op.name.to_owned()
            } else {
                let tail_start = op.name.len().saturating_sub(22);
                format!("..{}", op.name.get(tail_start..).unwrap_or(""))
            };

            let total_display = if op.total_ms >= 1_000.0f64 {
                format!("{:.1}s", float_math::div(op.total_ms, 1_000.0f64))
            } else {
                format!("{:.0}ms", op.total_ms)
            };

            let std_semantic = if op.std_ms < 1.0f64 {
                Semantic::Success
            } else if op.std_ms < 5.0f64 {
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
        .collect()
}

/// Build the optional Meilisearch stats row for the perf overlay (None when no
/// meili process is running or it reports no CPU/memory).
fn build_perf_meili(state: &State) -> Option<PerfMeiliStats> {
    let (cpu_pct, memory_bytes) = cp_mod_search::meili_process_stats(state)?;
    if memory_bytes == 0 && cpu_pct <= 0.0 {
        return None;
    }
    let mb = float_math::div_u64(memory_bytes, 1_048_576.0f64);
    Some(PerfMeiliStats { cpu_pct: f64::from(cpu_pct), cpu_semantic: cpu_semantic(f64::from(cpu_pct)), memory_mb: mb })
}

/// Build the two frame-budget bars (60fps / 30fps) from the average frame time.
fn build_perf_budget_bars(frame_avg_ms: f64) -> Vec<PerfBudgetBar> {
    let build_bar = |label: &str, budget_ms: f64| -> PerfBudgetBar {
        let pct = float_math::percent(frame_avg_ms, budget_ms).min(150.0);
        let semantic = if pct <= 80.0f64 {
            Semantic::Success
        } else if pct <= 100.0f64 {
            Semantic::Warning
        } else {
            Semantic::Error
        };
        PerfBudgetBar { label: label.into(), percent: pct, semantic }
    };
    vec![build_bar("60fps", FRAME_BUDGET_60FPS), build_bar("30fps", FRAME_BUDGET_30FPS)]
}

/// Minimum age before the F12 overlay rebuilds its perf snapshot.
const SNAPSHOT_TTL: std::time::Duration =
    std::time::Duration::from_millis(crate::infra::constants::PERF_OVERLAY_FRAME_MS);

/// A perf snapshot plus the instant it was taken.
type TimedSnapshot = (std::time::Instant, std::sync::Arc<crate::ui::perf::PerfSnapshot>);

/// Last perf snapshot and when it was taken. `PERF.snapshot()` locks and sorts
/// every op (~730); rebuilding it every frame made `ir_overlays` spike to 55 ms.
static SNAPSHOT_CACHE: std::sync::Mutex<Option<TimedSnapshot>> = std::sync::Mutex::new(None);

/// The perf snapshot, rebuilt at most every [`SNAPSHOT_TTL`] (30 per second).
fn cached_snapshot() -> std::sync::Arc<crate::ui::perf::PerfSnapshot> {
    let mut slot = SNAPSHOT_CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(entry) = slot.as_ref()
        && entry.0.elapsed() < SNAPSHOT_TTL
    {
        return std::sync::Arc::clone(&entry.1);
    }
    let snap = std::sync::Arc::new(crate::ui::perf::PERF.snapshot());
    *slot = Some((std::time::Instant::now(), std::sync::Arc::clone(&snap)));
    snap
}

/// Build the perf overlay IR data from the perf metrics snapshot.
fn build_perf_overlay(state: &State) -> PerfOverlay {
    let snapshot = cached_snapshot();

    let fps = if snapshot.frame_avg_ms > 0.0f64 { float_math::div(1_000.0f64, snapshot.frame_avg_ms) } else { 0.0f64 };

    let operations = build_perf_ops(&snapshot);

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
        meili: build_perf_meili(state),
        budget_bars: build_perf_budget_bars(snapshot.frame_avg_ms),
        share_names: LOOP_SHARE_STEPS.iter().map(|name| name.trim_start_matches("loop.").to_owned()).collect(),
        share_bars: build_perf_share_bars(&snapshot),
        loop_iterations: loop_iterations(&snapshot),
        sparkline: snapshot.frame_times_ms.clone(),
        operations,
    }
}

/// Level-1 main-loop steps shown in the share-bars, in loop execution order.
///
/// Fixed on purpose: segment colour = position in this list, so colours never
/// move between frames (sorting by the live snapshot reshuffled them on every
/// refresh). `loop.idle` is excluded: the input-poll park would dwarf every
/// real step. Keep in sync with `watchdog::Step::perf_name`.
const LOOP_SHARE_STEPS: [&str; 12] = [
    "loop.input",
    "loop.bridge",
    "loop.threads_emit",
    "loop.stream",
    "loop.cache",
    "loop.watchers",
    "loop.tools",
    "loop.spine",
    "loop.reverie",
    "loop.panel_refresh",
    "loop.render",
    "loop.save",
];

/// One snapshot op per [`LOOP_SHARE_STEPS`] entry (`None` = not recorded yet),
/// aligned index-for-index with the step list.
fn loop_substeps(snapshot: &crate::ui::perf::PerfSnapshot) -> Vec<Option<&crate::ui::perf::OpSnapshot>> {
    LOOP_SHARE_STEPS.iter().map(|&name| snapshot.ops.iter().find(|op| op.name == name)).collect()
}

/// Extracts one lifetime metric (µs or µs²) from an op snapshot.
type Metric = fn(&crate::ui::perf::OpSnapshot) -> f64;

/// Main-loop iterations recorded since F12 was enabled: `loop.idle` is marked
/// exactly once per iteration, and resets with the overlay (unlike
/// `PERF.loop_count`, which only ticks under `--measure`).
fn loop_iterations(snapshot: &crate::ui::perf::PerfSnapshot) -> u64 {
    snapshot.ops.iter().find(|op| op.name == "loop.idle").map_or(0, |op| op.count)
}

/// Four stacked share-bars (total / mean / std / max) over the loop substeps:
/// each segment is that substep's percentage of the metric's sum. `total`
/// (lifetime time ÷ loop iterations) is the average cost per iteration — where
/// wall time goes; the others show per-run cost of each substep.
/// Std (not variance) so the per-run bars share one unit (µs).
fn build_perf_share_bars(snapshot: &crate::ui::perf::PerfSnapshot) -> Vec<PerfShareBar> {
    let steps = loop_substeps(snapshot);
    let iterations = loop_iterations(snapshot).max(1).to_f64();
    let metrics: [(&str, &str, Metric); 4] = [
        ("total", "\u{b5}s/it", |op| op.total_ms),
        ("mean", "\u{b5}s", |op| op.mean_us),
        ("std", "\u{b5}s", |op| op.variance_us2.sqrt()),
        ("max", "\u{b5}s", |op| op.max_us),
    ];
    metrics
        .iter()
        .map(|&(label, unit, metric)| {
            let values: Vec<f64> = steps.iter().map(|op| op.map_or(0.0f64, metric)).collect();
            let total: f64 = values.iter().sum();
            let shares =
                values.iter().map(|&v| if total > 0.0f64 { float_math::percent(v, total) } else { 0.0f64 }).collect();
            // Total is summed in ms over the whole run: ×1000 → µs, ÷ iterations.
            let shown =
                if label == "total" { float_math::div(float_math::mul(total, 1_000.0f64), iterations) } else { total };
            PerfShareBar { label: label.to_owned(), shares, total_display: format!("{shown:.0}{unit}") }
        })
        .collect()
}
