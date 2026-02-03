use std::io;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use crossterm::event;
use ratatui::prelude::*;

use crate::actions::{apply_action, clean_llm_id_prefix, Action, ActionResult};
use crate::api::{start_cleaning, start_streaming, StreamEvent};
use crate::background::{generate_tldr, TlDrResult};
use crate::cache::{process_cache_request, CacheRequest, CacheUpdate};
use crate::constants::{EVENT_POLL_MS, MAX_CLEANING_ITERATIONS, MAX_API_RETRIES, GLOB_DEPRECATION_MS, GREP_DEPRECATION_MS, TMUX_DEPRECATION_MS, GIT_STATUS_REFRESH_MS, RENDER_THROTTLE_MS};
use crate::context_cleaner;
use crate::events::handle_event;
use crate::panels::now_ms;
use crate::persistence::{check_ownership, save_message, save_state};
use crate::state::{ContextType, Message, MessageStatus, MessageType, State, ToolResultRecord, ToolUseRecord};
use crate::tools::{execute_tool, ToolResult, ToolUse};
use crate::typewriter::TypewriterBuffer;
use crate::ui;
use crate::watcher::{FileWatcher, WatchEvent};

use super::context::prepare_stream_context;

pub struct App {
    pub state: State,
    typewriter: TypewriterBuffer,
    pending_done: Option<(usize, usize)>,
    pending_tools: Vec<ToolUse>,
    cleaning_pending_done: Option<(usize, usize)>,
    cleaning_pending_tools: Vec<ToolUse>,
    cleaning_iterations: u32,
    cache_tx: Sender<CacheUpdate>,
    file_watcher: Option<FileWatcher>,
    /// Tracks which file paths are being watched
    watched_file_paths: std::collections::HashSet<String>,
    /// Tracks which directory paths are being watched (for tree)
    watched_dir_paths: std::collections::HashSet<String>,
    /// Last time we checked timer-based caches
    last_timer_check_ms: u64,
    /// Last time we checked ownership
    last_ownership_check_ms: u64,
    /// Pending retry error (will retry on next loop iteration)
    pending_retry_error: Option<String>,
    /// Last render time for throttling
    last_render_ms: u64,
    /// Channel for API check results
    api_check_rx: Option<Receiver<crate::llms::ApiCheckResult>>,
    /// Whether to auto-start streaming on first loop iteration
    resume_stream: bool,
}

impl App {
    pub fn new(state: State, cache_tx: Sender<CacheUpdate>, resume_stream: bool) -> Self {
        let file_watcher = FileWatcher::new().ok();

        Self {
            state,
            typewriter: TypewriterBuffer::new(),
            pending_done: None,
            pending_tools: Vec::new(),
            cleaning_pending_done: None,
            cleaning_pending_tools: Vec::new(),
            cleaning_iterations: 0,
            cache_tx,
            file_watcher,
            watched_file_paths: std::collections::HashSet::new(),
            watched_dir_paths: std::collections::HashSet::new(),
            last_timer_check_ms: now_ms(),
            last_ownership_check_ms: now_ms(),
            pending_retry_error: None,
            last_render_ms: 0,
            api_check_rx: None,
            resume_stream,
        }
    }

    pub fn run(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        tx: Sender<StreamEvent>,
        rx: Receiver<StreamEvent>,
        tldr_tx: Sender<TlDrResult>,
        tldr_rx: Receiver<TlDrResult>,
        clean_tx: Sender<StreamEvent>,
        clean_rx: Receiver<StreamEvent>,
        cache_rx: Receiver<CacheUpdate>,
    ) -> io::Result<()> {
        // Initial cache setup - watch files and schedule initial refreshes
        self.setup_file_watchers();
        self.schedule_initial_cache_refreshes();

        // Claim ownership immediately
        save_state(&self.state);

        // Auto-resume streaming if flag was set (e.g., after reload_tui)
        if self.resume_stream {
            self.resume_stream = false;
            // Set input and trigger submit to start streaming
            self.state.input = "/* automatic post-reload message */".to_string();
            self.state.input_cursor = self.state.input.len();
            self.handle_action(Action::InputSubmit, &tx, &tldr_tx, &clean_tx);
        }

        loop {
            let current_ms = now_ms();

            // === INPUT FIRST: Process user input with minimal latency ===
            // Non-blocking check for input - handle immediately for responsive feel
            if event::poll(Duration::ZERO)? {
                let evt = event::read()?;

                let Some(action) = handle_event(&evt, &self.state) else {
                    save_state(&self.state);
                    break;
                };

                self.handle_action(action, &tx, &tldr_tx, &clean_tx);

                // Render immediately after input for instant feedback
                if self.state.dirty {
                    terminal.draw(|frame| ui::render(frame, &mut self.state))?;
                    self.state.dirty = false;
                    self.last_render_ms = current_ms;
                }
            }

            // === BACKGROUND PROCESSING ===
            self.process_stream_events(&rx);
            self.handle_retry(&tx);
            self.process_typewriter();
            self.process_cleaning_events(&clean_rx, &clean_tx);
            self.process_tldr_results(&tldr_rx);
            self.process_cache_updates(&cache_rx);
            self.process_watcher_events();
            self.check_timer_based_deprecation();
            self.handle_tool_execution(&tx, &tldr_tx, &clean_tx);
            self.finalize_stream(&tldr_tx, &clean_tx);
            self.process_api_check_results();

            // Check ownership periodically (every 1 second)
            if current_ms.saturating_sub(self.last_ownership_check_ms) >= 1000 {
                self.last_ownership_check_ms = current_ms;
                if !check_ownership() {
                    // Another instance took over - exit gracefully
                    break;
                }
            }

            // Update spinner animation if there's active loading/streaming
            self.update_spinner_animation();

            // Render if dirty and enough time has passed (capped at ~28fps)
            if self.state.dirty && current_ms.saturating_sub(self.last_render_ms) >= RENDER_THROTTLE_MS {
                terminal.draw(|frame| ui::render(frame, &mut self.state))?;
                self.state.dirty = false;
                self.last_render_ms = current_ms;
            }

            // Wait for next event (with timeout to keep checking background channels)
            let _ = event::poll(Duration::from_millis(EVENT_POLL_MS))?;
        }

        Ok(())
    }

    fn process_stream_events(&mut self, rx: &Receiver<StreamEvent>) {
        let _guard = crate::profile!("app::stream_events");
        while let Ok(evt) = rx.try_recv() {
            if !self.state.is_streaming {
                continue;
            }
            self.state.dirty = true;
            match evt {
                StreamEvent::Chunk(text) => {
                    self.typewriter.add_chunk(&text);
                }
                StreamEvent::ToolUse(tool) => {
                    self.pending_tools.push(tool);
                }
                StreamEvent::Done { input_tokens, output_tokens } => {
                    self.typewriter.mark_done();
                    self.pending_done = Some((input_tokens, output_tokens));
                }
                StreamEvent::Error(e) => {
                    self.typewriter.reset();
                    // Check if we should retry
                    if self.state.api_retry_count < MAX_API_RETRIES {
                        self.state.api_retry_count += 1;
                        self.pending_retry_error = Some(e);
                    } else {
                        // Max retries reached, show error
                        self.state.api_retry_count = 0;
                        apply_action(&mut self.state, Action::StreamError(e));
                    }
                }
            }
        }
    }

    fn handle_retry(&mut self, tx: &Sender<StreamEvent>) {
        if let Some(_error) = self.pending_retry_error.take() {
            // Still streaming, retry the request
            if self.state.is_streaming {
                // Clear any partial assistant message content before retrying
                if let Some(msg) = self.state.messages.last_mut() {
                    if msg.role == "assistant" {
                        msg.content.clear();
                    }
                }
                let ctx = prepare_stream_context(&mut self.state, true);
                self.typewriter.reset();
                self.pending_done = None;
                start_streaming(
                    self.state.llm_provider,
                    self.state.current_model(),
                    ctx.messages, ctx.context_items, ctx.tools, None, tx.clone(),
                );
                self.state.dirty = true;
            }
        }
    }

    fn process_typewriter(&mut self) {
        let _guard = crate::profile!("app::typewriter");
        if self.state.is_streaming {
            if let Some(chars) = self.typewriter.take_chars() {
                apply_action(&mut self.state, Action::AppendChars(chars));
                self.state.dirty = true;
            }
        }
    }

    fn process_cleaning_events(&mut self, clean_rx: &Receiver<StreamEvent>, clean_tx: &Sender<StreamEvent>) {
        while let Ok(evt) = clean_rx.try_recv() {
            if !self.state.is_cleaning_context {
                continue;
            }
            match evt {
                StreamEvent::Chunk(_text) => {
                    // Ignore text output from cleaner
                }
                StreamEvent::ToolUse(tool) => {
                    self.cleaning_pending_tools.push(tool);
                }
                StreamEvent::Done { input_tokens, output_tokens } => {
                    self.cleaning_pending_done = Some((input_tokens, output_tokens));
                }
                StreamEvent::Error(_e) => {
                    self.state.is_cleaning_context = false;
                    self.cleaning_pending_tools.clear();
                    self.cleaning_pending_done = None;
                    self.state.dirty = true;
                }
            }
        }

        // Execute cleaning tools
        if self.state.is_cleaning_context && self.cleaning_pending_done.is_some() && !self.cleaning_pending_tools.is_empty() {
            let tools = std::mem::take(&mut self.cleaning_pending_tools);
            self.cleaning_iterations += 1;

            for tool in &tools {
                let _result = execute_tool(tool, &mut self.state);
            }

            self.state.dirty = true;
            save_state(&self.state);

            let (_, usage_pct) = context_cleaner::calculate_context_usage(&self.state);
            if usage_pct < self.state.cleaning_target() || self.cleaning_iterations >= MAX_CLEANING_ITERATIONS {
                self.state.is_cleaning_context = false;
                self.cleaning_pending_done = None;
                self.cleaning_iterations = 0;
            } else {
                let ctx = prepare_stream_context(&mut self.state, true);
                let cleaner_tools = context_cleaner::get_cleaner_tools();
                self.cleaning_pending_done = None;
                start_cleaning(
                    self.state.llm_provider,
                    self.state.current_model(),
                    ctx.messages, ctx.context_items, cleaner_tools, &self.state, clean_tx.clone(),
                );
            }
        }

        // Finalize cleaning
        if self.state.is_cleaning_context && self.cleaning_pending_done.is_some() && self.cleaning_pending_tools.is_empty() {
            self.state.is_cleaning_context = false;
            self.cleaning_pending_done = None;
            self.cleaning_iterations = 0;
            self.state.dirty = true;
            save_state(&self.state);
        }
    }

    fn process_tldr_results(&mut self, tldr_rx: &Receiver<TlDrResult>) {
        while let Ok(tldr) = tldr_rx.try_recv() {
            self.state.pending_tldrs = self.state.pending_tldrs.saturating_sub(1);
            self.state.dirty = true;
            if let Some(msg) = self.state.messages.iter_mut().find(|m| m.id == tldr.message_id) {
                msg.tl_dr = Some(tldr.tl_dr);
                msg.tl_dr_token_count = tldr.token_count;
                save_message(msg);
            }
        }
    }

    fn process_api_check_results(&mut self) {
        if let Some(rx) = &self.api_check_rx {
            if let Ok(result) = rx.try_recv() {
                self.state.api_check_in_progress = false;
                self.state.api_check_result = Some(result);
                self.state.dirty = true;
                self.api_check_rx = None;
                save_state(&self.state);
            }
        }
    }

    fn handle_tool_execution(&mut self, tx: &Sender<StreamEvent>, tldr_tx: &Sender<TlDrResult>, clean_tx: &Sender<StreamEvent>) {
        if !self.state.is_streaming || self.pending_done.is_none() || !self.typewriter.pending_chars.is_empty() || self.pending_tools.is_empty() {
            return;
        }

        self.state.dirty = true;
        let tools = std::mem::take(&mut self.pending_tools);
        let mut tool_results: Vec<ToolResult> = Vec::new();

        // Finalize current assistant message
        if let Some(msg) = self.state.messages.last_mut() {
            if msg.role == "assistant" {
                // Clean any LLM ID prefixes before saving
                msg.content = clean_llm_id_prefix(&msg.content);
                save_message(msg);
                if !msg.content.trim().is_empty() && msg.tl_dr.is_none() {
                    self.state.pending_tldrs += 1;
                    generate_tldr(msg.id.clone(), msg.content.clone(), tldr_tx.clone());
                }
            }
        }

        // Create tool call messages
        for tool in &tools {
            let tool_id = format!("T{}", self.state.next_tool_id);
            self.state.next_tool_id += 1;

            let tool_msg = Message {
                id: tool_id,
                role: "assistant".to_string(),
                message_type: MessageType::ToolCall,
                content: String::new(),
                content_token_count: 0,
                tl_dr: None,
                tl_dr_token_count: 0,
                status: MessageStatus::Full,
                tool_uses: vec![ToolUseRecord {
                    id: tool.id.clone(),
                    name: tool.name.clone(),
                    input: tool.input.clone(),
                }],
                tool_results: Vec::new(),
                input_tokens: 0,
            };
            save_message(&tool_msg);
            self.state.messages.push(tool_msg);

            let result = execute_tool(tool, &mut self.state);
            tool_results.push(result);
        }

        // Create tool result message
        let result_id = format!("R{}", self.state.next_result_id);
        self.state.next_result_id += 1;
        let tool_result_records: Vec<ToolResultRecord> = tool_results.iter()
            .map(|r| ToolResultRecord {
                tool_use_id: r.tool_use_id.clone(),
                content: r.content.clone(),
                is_error: r.is_error,
            })
            .collect();
        let result_msg = Message {
            id: result_id,
            role: "user".to_string(),
            message_type: MessageType::ToolResult,
            content: String::new(),
            content_token_count: 0,
            tl_dr: None,
            tl_dr_token_count: 0,
            status: MessageStatus::Full,
            tool_uses: Vec::new(),
            tool_results: tool_result_records,
            input_tokens: 0,
        };
        save_message(&result_msg);
        self.state.messages.push(result_msg);

        // Create new assistant message
        let assistant_id = format!("A{}", self.state.next_assistant_id);
        self.state.next_assistant_id += 1;
        let new_assistant_msg = Message {
            id: assistant_id,
            role: "assistant".to_string(),
            message_type: MessageType::TextMessage,
            content: String::new(),
            content_token_count: 0,
            tl_dr: None,
            tl_dr_token_count: 0,
            status: MessageStatus::Full,
            tool_uses: Vec::new(),
            tool_results: Vec::new(),
            input_tokens: 0,
        };
        self.state.messages.push(new_assistant_msg);

        self.state.streaming_estimated_tokens = 0;
        save_state(&self.state);

        // Check if automatic cleaning should start (before continuing streaming)
        if context_cleaner::should_clean_context(&self.state) {
            self.state.is_cleaning_context = true;
            self.cleaning_pending_tools.clear();
            self.cleaning_pending_done = None;
            self.cleaning_iterations = 0;
            let ctx = prepare_stream_context(&mut self.state, true);
            let cleaner_tools = context_cleaner::get_cleaner_tools();
            start_cleaning(
                self.state.llm_provider,
                self.state.current_model(),
                ctx.messages, ctx.context_items, cleaner_tools, &self.state, clean_tx.clone(),
            );
        }

        // Continue streaming
        let ctx = prepare_stream_context(&mut self.state, true);
        self.typewriter.reset();
        self.pending_done = None;
        start_streaming(
            self.state.llm_provider,
            self.state.current_model(),
            ctx.messages, ctx.context_items, ctx.tools, None, tx.clone(),
        );
    }

    fn finalize_stream(&mut self, tldr_tx: &Sender<TlDrResult>, clean_tx: &Sender<StreamEvent>) {
        if !self.state.is_streaming {
            return;
        }

        if let Some((input_tokens, output_tokens)) = self.pending_done {
            if self.typewriter.pending_chars.is_empty() && self.pending_tools.is_empty() {
                self.state.dirty = true;
                match apply_action(&mut self.state, Action::StreamDone { _input_tokens: input_tokens, output_tokens }) {
                    ActionResult::SaveMessage(id) => {
                        let tldr_info = self.state.messages.iter().find(|m| m.id == id).and_then(|msg| {
                            save_message(msg);
                            if msg.role == "assistant" && msg.tl_dr.is_none() && !msg.content.is_empty() {
                                Some((msg.id.clone(), msg.content.clone()))
                            } else {
                                None
                            }
                        });
                        if let Some((msg_id, content)) = tldr_info {
                            self.state.pending_tldrs += 1;
                            generate_tldr(msg_id, content, tldr_tx.clone());
                        }
                        save_state(&self.state);
                    }
                    ActionResult::Save => save_state(&self.state),
                    _ => {}
                }
                // Reset retry count on successful completion
                self.state.api_retry_count = 0;
                self.typewriter.reset();
                self.pending_done = None;

                // Check if automatic cleaning should start
                if context_cleaner::should_clean_context(&self.state) {
                    self.state.is_cleaning_context = true;
                    self.cleaning_pending_tools.clear();
                    self.cleaning_pending_done = None;
                    self.cleaning_iterations = 0;
                    let ctx = prepare_stream_context(&mut self.state, true);
                    let cleaner_tools = context_cleaner::get_cleaner_tools();
                    start_cleaning(
                        self.state.llm_provider,
                        self.state.current_model(),
                        ctx.messages, ctx.context_items, cleaner_tools, &self.state, clean_tx.clone(),
                    );
                }
            }
        }
    }

    fn handle_action(
        &mut self,
        action: Action,
        tx: &Sender<StreamEvent>,
        tldr_tx: &Sender<TlDrResult>,
        clean_tx: &Sender<StreamEvent>,
    ) {
        // Any action triggers a re-render
        self.state.dirty = true;
        match apply_action(&mut self.state, action) {
            ActionResult::StartStream => {
                self.typewriter.reset();
                self.pending_tools.clear();
                // Generate TL;DR for user message
                if self.state.messages.len() >= 2 {
                    let user_msg = &self.state.messages[self.state.messages.len() - 2];
                    if user_msg.role == "user" && user_msg.tl_dr.is_none() {
                        self.state.pending_tldrs += 1;
                        generate_tldr(user_msg.id.clone(), user_msg.content.clone(), tldr_tx.clone());
                    }
                }
                let ctx = prepare_stream_context(&mut self.state, false);
                start_streaming(
                    self.state.llm_provider,
                    self.state.current_model(),
                    ctx.messages, ctx.context_items, ctx.tools, None, tx.clone(),
                );
                save_state(&self.state);
            }
            ActionResult::StopStream => {
                self.typewriter.reset();
                self.pending_done = None;
                self.pending_tools.clear();
                if let Some(msg) = self.state.messages.last() {
                    if msg.role == "assistant" {
                        save_message(msg);
                    }
                }
                save_state(&self.state);
            }
            ActionResult::Save => {
                save_state(&self.state);
            }
            ActionResult::SaveMessage(id) => {
                let tldr_info = self.state.messages.iter().find(|m| m.id == id).and_then(|msg| {
                    save_message(msg);
                    if msg.role == "assistant" && msg.tl_dr.is_none() && !msg.content.is_empty() {
                        Some((msg.id.clone(), msg.content.clone()))
                    } else {
                        None
                    }
                });
                if let Some((msg_id, content)) = tldr_info {
                    self.state.pending_tldrs += 1;
                    generate_tldr(msg_id, content, tldr_tx.clone());
                }
                save_state(&self.state);
            }
            ActionResult::StartCleaning => {
                self.cleaning_pending_tools.clear();
                self.cleaning_pending_done = None;
                self.cleaning_iterations = 0;
                let ctx = prepare_stream_context(&mut self.state, true);
                let cleaner_tools = context_cleaner::get_cleaner_tools();
                start_cleaning(
                    self.state.llm_provider,
                    self.state.current_model(),
                    ctx.messages, ctx.context_items, cleaner_tools, &self.state, clean_tx.clone(),
                );
                save_state(&self.state);
            }
            ActionResult::StartApiCheck => {
                let (api_tx, api_rx) = std::sync::mpsc::channel();
                self.api_check_rx = Some(api_rx);
                crate::llms::start_api_check(
                    self.state.llm_provider,
                    self.state.current_model(),
                    api_tx,
                );
                save_state(&self.state);
            }
            ActionResult::Nothing => {}
        }
    }

    /// Set up file watchers for all current file contexts and tree open folders
    fn setup_file_watchers(&mut self) {
        let Some(watcher) = &mut self.file_watcher else { return };

        // Watch files in File contexts
        for ctx in &self.state.context {
            if ctx.context_type == ContextType::File {
                if let Some(path) = &ctx.file_path {
                    if !self.watched_file_paths.contains(path) {
                        if watcher.watch_file(path).is_ok() {
                            self.watched_file_paths.insert(path.clone());
                        }
                    }
                }
            }
        }

        // Watch directories for Tree panel (only open folders)
        for folder in &self.state.tree_open_folders {
            if !self.watched_dir_paths.contains(folder) {
                if watcher.watch_dir(folder).is_ok() {
                    self.watched_dir_paths.insert(folder.clone());
                }
            }
        }
    }

    /// Schedule initial cache refreshes for all context elements
    fn schedule_initial_cache_refreshes(&self) {
        let current_ms = now_ms();

        for ctx in &self.state.context {
            match ctx.context_type {
                ContextType::File => {
                    if let Some(path) = &ctx.file_path {
                        process_cache_request(
                            CacheRequest::RefreshFile {
                                context_id: ctx.id.clone(),
                                file_path: path.clone(),
                                current_hash: ctx.file_hash.clone(),
                            },
                            self.cache_tx.clone(),
                        );
                    }
                }
                ContextType::Tree => {
                    process_cache_request(
                        CacheRequest::RefreshTree {
                            context_id: ctx.id.clone(),
                            tree_filter: self.state.tree_filter.clone(),
                            tree_open_folders: self.state.tree_open_folders.clone(),
                            tree_descriptions: self.state.tree_descriptions.clone(),
                        },
                        self.cache_tx.clone(),
                    );
                }
                ContextType::Glob => {
                    if let Some(pattern) = &ctx.glob_pattern {
                        process_cache_request(
                            CacheRequest::RefreshGlob {
                                context_id: ctx.id.clone(),
                                pattern: pattern.clone(),
                                base_path: ctx.glob_path.clone(),
                            },
                            self.cache_tx.clone(),
                        );
                    }
                }
                ContextType::Grep => {
                    if let Some(pattern) = &ctx.grep_pattern {
                        process_cache_request(
                            CacheRequest::RefreshGrep {
                                context_id: ctx.id.clone(),
                                pattern: pattern.clone(),
                                path: ctx.grep_path.clone(),
                                file_pattern: ctx.grep_file_pattern.clone(),
                            },
                            self.cache_tx.clone(),
                        );
                    }
                }
                ContextType::Tmux => {
                    if let Some(pane_id) = &ctx.tmux_pane_id {
                        process_cache_request(
                            CacheRequest::RefreshTmux {
                                context_id: ctx.id.clone(),
                                pane_id: pane_id.clone(),
                                current_last_lines_hash: ctx.tmux_last_lines_hash.clone(),
                            },
                            self.cache_tx.clone(),
                        );
                    }
                }
                // Conversation, Memory, Todo, Overview - internal state triggers, no initial refresh needed
                _ => {}
            }
        }

        // Schedule initial git status refresh
        process_cache_request(
            CacheRequest::RefreshGitStatus {
                show_diffs: self.state.git_show_diffs,
                current_hash: None, // Force full refresh on startup
            },
            self.cache_tx.clone(),
        );

        // Update last timer check
        // (This is handled in the mutable version - we just set it in new())
        let _ = current_ms;
    }

    /// Process incoming cache updates from background threads
    fn process_cache_updates(&mut self, cache_rx: &Receiver<CacheUpdate>) {
        while let Ok(update) = cache_rx.try_recv() {
            self.state.dirty = true;

            match update {
                CacheUpdate::FileContent { context_id, content, hash, token_count } => {
                    if let Some(ctx) = self.state.context.iter_mut().find(|c| c.id == context_id) {
                        ctx.cached_content = Some(content);
                        ctx.file_hash = Some(hash);
                        ctx.token_count = token_count;
                        ctx.cache_deprecated = false;
                        ctx.last_refresh_ms = now_ms();
                    }
                }
                CacheUpdate::TreeContent { context_id, content, token_count } => {
                    if let Some(ctx) = self.state.context.iter_mut().find(|c| c.id == context_id) {
                        ctx.cached_content = Some(content);
                        ctx.token_count = token_count;
                        ctx.cache_deprecated = false;
                        ctx.last_refresh_ms = now_ms();
                    }
                }
                CacheUpdate::GlobContent { context_id, content, token_count } => {
                    if let Some(ctx) = self.state.context.iter_mut().find(|c| c.id == context_id) {
                        ctx.cached_content = Some(content);
                        ctx.token_count = token_count;
                        ctx.cache_deprecated = false;
                        ctx.last_refresh_ms = now_ms();
                    }
                }
                CacheUpdate::GrepContent { context_id, content, token_count } => {
                    if let Some(ctx) = self.state.context.iter_mut().find(|c| c.id == context_id) {
                        ctx.cached_content = Some(content);
                        ctx.token_count = token_count;
                        ctx.cache_deprecated = false;
                        ctx.last_refresh_ms = now_ms();
                    }
                }
                CacheUpdate::TmuxContent { context_id, content, last_lines_hash, token_count } => {
                    if let Some(ctx) = self.state.context.iter_mut().find(|c| c.id == context_id) {
                        ctx.cached_content = Some(content);
                        ctx.tmux_last_lines_hash = Some(last_lines_hash);
                        ctx.token_count = token_count;
                        ctx.cache_deprecated = false;
                        ctx.last_refresh_ms = now_ms();
                    }
                }
                CacheUpdate::GitStatus {
                    branch,
                    is_repo,
                    file_changes,
                    branches,
                    formatted_content,
                    token_count,
                    status_hash,
                } => {
                    use crate::state::{GitFileChange, ContextType};
                    self.state.git_branch = branch;
                    self.state.git_branches = branches;
                    self.state.git_is_repo = is_repo;
                    self.state.git_file_changes = file_changes.into_iter()
                        .map(|(path, additions, deletions, change_type, diff_content)| GitFileChange {
                            path,
                            additions,
                            deletions,
                            change_type,
                            diff_content,
                        })
                        .collect();
                    self.state.git_last_refresh_ms = now_ms();
                    self.state.git_status_hash = Some(status_hash);

                    // Update cached content and token count for Git panel
                    for ctx in &mut self.state.context {
                        if ctx.context_type == ContextType::Git {
                            ctx.cached_content = Some(formatted_content);
                            ctx.token_count = token_count;
                            ctx.cache_deprecated = false;
                            ctx.last_refresh_ms = now_ms();
                            break;
                        }
                    }
                }
                CacheUpdate::GitStatusUnchanged => {
                    // Just update the refresh time, no other changes needed
                    self.state.git_last_refresh_ms = now_ms();
                    self.state.dirty = false; // No actual change, don't trigger re-render
                }
            }
        }
    }

    /// Process file watcher events
    fn process_watcher_events(&mut self) {
        let Some(watcher) = &self.file_watcher else { return };

        let events = watcher.poll_events();
        for event in events {
            match event {
                WatchEvent::FileChanged(path) => {
                    // Find and mark file context as deprecated, then schedule refresh
                    for ctx in &mut self.state.context {
                        if ctx.context_type == ContextType::File && ctx.file_path.as_deref() == Some(&path) {
                            ctx.cache_deprecated = true;
                            self.state.dirty = true;

                            // Schedule background refresh
                            process_cache_request(
                                CacheRequest::RefreshFile {
                                    context_id: ctx.id.clone(),
                                    file_path: path.clone(),
                                    current_hash: ctx.file_hash.clone(),
                                },
                                self.cache_tx.clone(),
                            );
                        }
                    }
                }
                WatchEvent::DirChanged(_path) => {
                    // Mark tree context as deprecated and schedule refresh
                    for ctx in &mut self.state.context {
                        if ctx.context_type == ContextType::Tree {
                            ctx.cache_deprecated = true;
                            self.state.dirty = true;

                            // Schedule background refresh
                            process_cache_request(
                                CacheRequest::RefreshTree {
                                    context_id: ctx.id.clone(),
                                    tree_filter: self.state.tree_filter.clone(),
                                    tree_open_folders: self.state.tree_open_folders.clone(),
                                    tree_descriptions: self.state.tree_descriptions.clone(),
                                },
                                self.cache_tx.clone(),
                            );
                        }
                    }
                }
            }
        }
    }

    /// Check timer-based deprecation for glob, grep, tmux
    /// Also handles initial population for newly created context elements
    fn check_timer_based_deprecation(&mut self) {
        let current_ms = now_ms();

        // Only check every 100ms to avoid excessive work
        if current_ms.saturating_sub(self.last_timer_check_ms) < 100 {
            return;
        }
        self.last_timer_check_ms = current_ms;

        for ctx in &self.state.context {
            // Check if this element needs initial population (newly created via tool)
            let needs_initial_population = ctx.cached_content.is_none();

            // Check if cache was explicitly marked as deprecated (e.g., after sending keys to tmux)
            let explicitly_deprecated = ctx.cache_deprecated;

            // For timer-based types, also check if refresh timer has elapsed
            let timer_refresh_needed = match ctx.context_type {
                ContextType::Glob => {
                    let elapsed = current_ms.saturating_sub(ctx.last_refresh_ms);
                    elapsed >= GLOB_DEPRECATION_MS
                }
                ContextType::Grep => {
                    let elapsed = current_ms.saturating_sub(ctx.last_refresh_ms);
                    elapsed >= GREP_DEPRECATION_MS
                }
                ContextType::Tmux => {
                    let elapsed = current_ms.saturating_sub(ctx.last_refresh_ms);
                    elapsed >= TMUX_DEPRECATION_MS
                }
                _ => false,
            };

            if needs_initial_population || explicitly_deprecated || timer_refresh_needed {
                // Schedule refresh in background based on context type
                match ctx.context_type {
                    ContextType::File => {
                        if let Some(path) = &ctx.file_path {
                            // Set up file watcher for new file
                            if needs_initial_population {
                                if let Some(watcher) = &mut self.file_watcher {
                                    if !self.watched_file_paths.contains(path) {
                                        if watcher.watch_file(path).is_ok() {
                                            self.watched_file_paths.insert(path.clone());
                                        }
                                    }
                                }
                            }
                            process_cache_request(
                                CacheRequest::RefreshFile {
                                    context_id: ctx.id.clone(),
                                    file_path: path.clone(),
                                    current_hash: ctx.file_hash.clone(),
                                },
                                self.cache_tx.clone(),
                            );
                        }
                    }
                    ContextType::Glob => {
                        if let Some(pattern) = &ctx.glob_pattern {
                            process_cache_request(
                                CacheRequest::RefreshGlob {
                                    context_id: ctx.id.clone(),
                                    pattern: pattern.clone(),
                                    base_path: ctx.glob_path.clone(),
                                },
                                self.cache_tx.clone(),
                            );
                        }
                    }
                    ContextType::Grep => {
                        if let Some(pattern) = &ctx.grep_pattern {
                            process_cache_request(
                                CacheRequest::RefreshGrep {
                                    context_id: ctx.id.clone(),
                                    pattern: pattern.clone(),
                                    path: ctx.grep_path.clone(),
                                    file_pattern: ctx.grep_file_pattern.clone(),
                                },
                                self.cache_tx.clone(),
                            );
                        }
                    }
                    ContextType::Tmux => {
                        if let Some(pane_id) = &ctx.tmux_pane_id {
                            process_cache_request(
                                CacheRequest::RefreshTmux {
                                    context_id: ctx.id.clone(),
                                    pane_id: pane_id.clone(),
                                    current_last_lines_hash: ctx.tmux_last_lines_hash.clone(),
                                },
                                self.cache_tx.clone(),
                            );
                        }
                    }
                    // Tree, Conversation, Todo, Memory, Overview - handled by state changes
                    _ => {}
                }
            }
        }

        // Check if git status needs refresh (timer-based or explicitly deprecated)
        let git_elapsed = current_ms.saturating_sub(self.state.git_last_refresh_ms);
        let git_deprecated = self.state.context.iter()
            .any(|c| c.context_type == ContextType::Git && c.cache_deprecated);
        
        if git_elapsed >= GIT_STATUS_REFRESH_MS || git_deprecated {
            // Clear the deprecated flag before requesting refresh
            for ctx in &mut self.state.context {
                if ctx.context_type == ContextType::Git {
                    ctx.cache_deprecated = false;
                }
            }
            process_cache_request(
                CacheRequest::RefreshGitStatus {
                    show_diffs: self.state.git_show_diffs,
                    current_hash: if git_deprecated { None } else { self.state.git_status_hash.clone() },
                },
                self.cache_tx.clone(),
            );
        }
    }

    /// Update spinner animation frame if there's active loading/streaming
    fn update_spinner_animation(&mut self) {
        // Check if there's any active operation that needs spinner animation
        let has_active_spinner = self.state.is_streaming
            || self.state.is_cleaning_context
            || self.state.pending_tldrs > 0
            || self.state.api_check_in_progress
            || self.state.context.iter().any(|c| {
                c.cached_content.is_none() && c.context_type.needs_cache()
            });

        if has_active_spinner {
            // Increment spinner frame (wraps around automatically with u64)
            self.state.spinner_frame = self.state.spinner_frame.wrapping_add(1);
            // Mark dirty to trigger re-render with new spinner frame
            self.state.dirty = true;
        }
    }
}
