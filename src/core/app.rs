use std::io;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use crossterm::event;
use ratatui::prelude::*;

use crate::actions::{apply_action, clean_llm_id_prefix, Action, ActionResult};
use crate::api::{start_streaming, StreamEvent};
use crate::background::{generate_tldr, TlDrResult};
use crate::cache::{process_cache_request, CacheRequest, CacheUpdate};
use crate::constants::{DEFAULT_WORKER_ID, EVENT_POLL_MS, MAX_API_RETRIES, RENDER_THROTTLE_MS};
use crate::events::handle_event;
use crate::help::CommandPalette;
use crate::core::panels::now_ms;
use crate::persistence::{check_ownership, save_message, save_state};
use crate::state::{ContextType, Message, MessageStatus, MessageType, State, ToolResultRecord, ToolUseRecord};
use crate::tools::{execute_tool, perform_reload, ToolResult, ToolUse};
use crate::typewriter::TypewriterBuffer;
use crate::ui;
use crate::watcher::{FileWatcher, WatchEvent};
use crate::gh_watcher::GhWatcher;

use super::context::prepare_stream_context;
use super::init::get_active_seed_content;

pub struct App {
    pub state: State,
    typewriter: TypewriterBuffer,
    pending_done: Option<(usize, usize, usize, usize, Option<String>)>,
    pending_tools: Vec<ToolUse>,
    cache_tx: Sender<CacheUpdate>,
    file_watcher: Option<FileWatcher>,
    gh_watcher: GhWatcher,
    /// Tracks which file paths are being watched
    watched_file_paths: std::collections::HashSet<String>,
    /// Tracks which directory paths are being watched (for tree)
    watched_dir_paths: std::collections::HashSet<String>,
    /// Tracks .git/ paths being watched (for GitResult panel deprecation)
    watched_git_paths: std::collections::HashSet<String>,
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
    /// Command palette state
    pub command_palette: CommandPalette,
}

impl App {
    pub fn new(state: State, cache_tx: Sender<CacheUpdate>, resume_stream: bool) -> Self {
        let file_watcher = FileWatcher::new().ok();
        let gh_watcher = GhWatcher::new(cache_tx.clone());

        Self {
            state,
            typewriter: TypewriterBuffer::new(),
            pending_done: None,
            pending_tools: Vec::new(),
            cache_tx,
            file_watcher,
            gh_watcher,
            watched_file_paths: std::collections::HashSet::new(),
            watched_dir_paths: std::collections::HashSet::new(),
            watched_git_paths: std::collections::HashSet::new(),
            last_timer_check_ms: now_ms(),
            last_ownership_check_ms: now_ms(),
            pending_retry_error: None,
            last_render_ms: 0,
            api_check_rx: None,
            resume_stream,
            command_palette: CommandPalette::new(),
        }
    }

    pub fn run(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
        tx: Sender<StreamEvent>,
        rx: Receiver<StreamEvent>,
        tldr_tx: Sender<TlDrResult>,
        tldr_rx: Receiver<TlDrResult>,
        cache_rx: Receiver<CacheUpdate>,
    ) -> io::Result<()> {
        // Initial cache setup - watch files and schedule initial refreshes
        self.setup_file_watchers();
        self.sync_gh_watches();
        self.schedule_initial_cache_refreshes();

        // Claim ownership immediately
        save_state(&self.state);

        // Auto-resume streaming if flag was set (e.g., after reload_tui)
        if self.resume_stream {
            self.resume_stream = false;
            // Set input and trigger submit to start streaming
            self.state.input = "/* automatic post-reload message */".to_string();
            self.state.input_cursor = self.state.input.len();
            self.handle_action(Action::InputSubmit, &tx, &tldr_tx);
        }

        loop {
            let current_ms = now_ms();

            // === INPUT FIRST: Process user input with minimal latency ===
            // Non-blocking check for input - handle immediately for responsive feel
            if event::poll(Duration::ZERO)? {
                let evt = event::read()?;

                // Handle command palette events first if it's open
                if self.command_palette.is_open {
                    if let Some(action) = self.handle_palette_event(&evt) {
                        self.handle_action(action, &tx, &tldr_tx);
                    }
                    self.state.dirty = true;

                    // Render immediately after input for instant feedback
                    if self.state.dirty {
                        terminal.draw(|frame| {
                            ui::render(frame, &mut self.state);
                            self.command_palette.render(frame, &self.state);
                        })?;
                        self.state.dirty = false;
                        self.last_render_ms = current_ms;
                    }
                    continue;
                }

                let Some(action) = handle_event(&evt, &self.state) else {
                    save_state(&self.state);
                    break;
                };

                // Check for Ctrl+P to open palette
                if let Action::OpenCommandPalette = action {
                    self.command_palette.open(&self.state);
                    self.state.dirty = true;
                } else {
                    self.handle_action(action, &tx, &tldr_tx);
                }

                // Render immediately after input for instant feedback
                if self.state.dirty {
                    terminal.draw(|frame| {
                        ui::render(frame, &mut self.state);
                        self.command_palette.render(frame, &self.state);
                    })?;
                    self.state.dirty = false;
                    self.last_render_ms = current_ms;
                }
            }

            // === BACKGROUND PROCESSING ===
            self.process_stream_events(&rx);
            self.handle_retry(&tx);
            self.process_typewriter();
            self.process_tldr_results(&tldr_rx);
            self.process_cache_updates(&cache_rx);
            self.process_watcher_events();
            self.sync_gh_watches();
            self.check_timer_based_deprecation();
            self.handle_tool_execution(&tx, &tldr_tx, &cache_rx, terminal);
            self.finalize_stream(&tldr_tx);
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
                terminal.draw(|frame| {
                    ui::render(frame, &mut self.state);
                    self.command_palette.render(frame, &self.state);
                })?;
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
                StreamEvent::Done { input_tokens, output_tokens, cache_hit_tokens, cache_miss_tokens, stop_reason } => {
                    self.typewriter.mark_done();
                    self.pending_done = Some((input_tokens, output_tokens, cache_hit_tokens, cache_miss_tokens, stop_reason));
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
                let system_prompt = get_active_seed_content(&self.state);
                self.typewriter.reset();
                self.pending_done = None;
                start_streaming(
                    self.state.llm_provider,
                    self.state.current_model(),
                    ctx.messages, ctx.context_items, ctx.tools, None, system_prompt.clone(), Some(system_prompt), DEFAULT_WORKER_ID.to_string(), tx.clone(),
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

    fn handle_tool_execution(&mut self, tx: &Sender<StreamEvent>, tldr_tx: &Sender<TlDrResult>, cache_rx: &Receiver<CacheUpdate>, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) {
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
            let tool_uid = format!("UID_{}_T", self.state.global_next_uid);
            self.state.next_tool_id += 1;
            self.state.global_next_uid += 1;

            let tool_msg = Message {
                id: tool_id,
                uid: Some(tool_uid),
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
                timestamp_ms: crate::core::panels::now_ms(),
            };
            save_message(&tool_msg);
            self.state.messages.push(tool_msg);

            let result = execute_tool(tool, &mut self.state);
            tool_results.push(result);
        }

        // Create tool result message
        let result_id = format!("R{}", self.state.next_result_id);
        let result_uid = format!("UID_{}_R", self.state.global_next_uid);
        self.state.next_result_id += 1;
        self.state.global_next_uid += 1;
        let tool_result_records: Vec<ToolResultRecord> = tool_results.iter()
            .map(|r| ToolResultRecord {
                tool_use_id: r.tool_use_id.clone(),
                content: r.content.clone(),
                is_error: r.is_error,
            })
            .collect();
        let result_msg = Message {
            id: result_id,
            uid: Some(result_uid),
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
            timestamp_ms: crate::core::panels::now_ms(),
        };
        save_message(&result_msg);
        self.state.messages.push(result_msg);

        // Check if reload was requested - perform it after tool result is saved
        if self.state.reload_pending {
            perform_reload(&mut self.state);
            // Note: perform_reload calls std::process::exit(0), so we won't reach here
        }

        // Create new assistant message
        let assistant_id = format!("A{}", self.state.next_assistant_id);
        let assistant_uid = format!("UID_{}_A", self.state.global_next_uid);
        self.state.next_assistant_id += 1;
        self.state.global_next_uid += 1;
        let new_assistant_msg = Message {
            id: assistant_id,
            uid: Some(assistant_uid),
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
            timestamp_ms: crate::core::panels::now_ms(),
        };
        self.state.messages.push(new_assistant_msg);

        self.state.streaming_estimated_tokens = 0;

        // Accumulate token stats from intermediate stream before discarding pending_done
        if let Some((_, output_tokens, cache_hit_tokens, cache_miss_tokens, _)) = self.pending_done {
            self.state.tick_cache_hit_tokens = cache_hit_tokens;
            self.state.tick_cache_miss_tokens = cache_miss_tokens;
            self.state.tick_output_tokens = output_tokens;
            self.state.stream_cache_hit_tokens += cache_hit_tokens;
            self.state.stream_cache_miss_tokens += cache_miss_tokens;
            self.state.stream_output_tokens += output_tokens;
            self.state.cache_hit_tokens += cache_hit_tokens;
            self.state.cache_miss_tokens += cache_miss_tokens;
            self.state.total_output_tokens += output_tokens;
        }

        save_state(&self.state);

        // Wait for any dirty file panels to be loaded before continuing
        super::wait::wait_for_panels(&mut self.state, cache_rx, &self.cache_tx, terminal, |state, rx| {
            Self::process_cache_updates_static(state, rx);
        });

        // Continue streaming
        let ctx = prepare_stream_context(&mut self.state, true);
        let system_prompt = get_active_seed_content(&self.state);
        self.typewriter.reset();
        self.pending_done = None;
        start_streaming(
            self.state.llm_provider,
            self.state.current_model(),
            ctx.messages, ctx.context_items, ctx.tools, None, system_prompt.clone(), Some(system_prompt), DEFAULT_WORKER_ID.to_string(), tx.clone(),
        );
    }

    fn finalize_stream(&mut self, tldr_tx: &Sender<TlDrResult>) {
        if !self.state.is_streaming {
            return;
        }

        if let Some((input_tokens, output_tokens, cache_hit_tokens, cache_miss_tokens, ref stop_reason)) = self.pending_done {
            if self.typewriter.pending_chars.is_empty() && self.pending_tools.is_empty() {
                self.state.dirty = true;
                let stop_reason = stop_reason.clone();
                match apply_action(&mut self.state, Action::StreamDone { _input_tokens: input_tokens, output_tokens, cache_hit_tokens, cache_miss_tokens, stop_reason }) {
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
            }
        }
    }

    fn handle_action(
        &mut self,
        action: Action,
        tx: &Sender<StreamEvent>,
        tldr_tx: &Sender<TlDrResult>,
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
                let system_prompt = get_active_seed_content(&self.state);
                start_streaming(
                    self.state.llm_provider,
                    self.state.current_model(),
                    ctx.messages, ctx.context_items, ctx.tools, None, system_prompt.clone(), Some(system_prompt), DEFAULT_WORKER_ID.to_string(), tx.clone(),
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

        // Watch .git/ paths for GitResult panel deprecation
        if self.watched_git_paths.is_empty() {
            for path in &[".git/HEAD", ".git/index", ".git/MERGE_HEAD", ".git/REBASE_HEAD", ".git/CHERRY_PICK_HEAD"] {
                if watcher.watch_file(path).is_ok() {
                    self.watched_git_paths.insert(path.to_string());
                }
            }
            for path in &[".git/refs/heads", ".git/refs/tags", ".git/refs/remotes"] {
                if watcher.watch_dir_recursive(path).is_ok() {
                    self.watched_git_paths.insert(path.to_string());
                }
            }
        }
    }

    /// Sync GhWatcher with current GithubResult panels
    fn sync_gh_watches(&self) {
        let token = match &self.state.github_token {
            Some(t) => t.clone(),
            None => return,
        };
        let panels: Vec<(String, String, String)> = self.state.context.iter()
            .filter(|c| c.context_type == ContextType::GithubResult)
            .filter_map(|c| c.result_command.as_ref().map(|cmd| (c.id.clone(), cmd.clone(), token.clone())))
            .collect();
        self.gh_watcher.sync_watches(&panels);
    }

    /// Schedule initial cache refreshes for all context elements
    fn schedule_initial_cache_refreshes(&self) {
        for ctx in &self.state.context {
            let panel = crate::core::panels::get_panel(ctx.context_type);
            if let Some(request) = panel.build_cache_request(ctx, &self.state) {
                process_cache_request(request, self.cache_tx.clone());
            }
        }
    }

    /// Process incoming cache updates from background threads
    fn process_cache_updates(&mut self, cache_rx: &Receiver<CacheUpdate>) {
        Self::process_cache_updates_static(&mut self.state, cache_rx);
    }

    /// Static version of process_cache_updates for use in wait module
    fn process_cache_updates_static(state: &mut State, cache_rx: &Receiver<CacheUpdate>) {
        while let Ok(update) = cache_rx.try_recv() {
            let context_type = update.context_type();
            let panel = crate::core::panels::get_panel(context_type);

            // Find the matching context element index
            let idx = if let Some(id) = update.context_id() {
                state.context.iter().position(|c| c.id == id)
            } else {
                // Git updates: match by context_type
                state.context.iter().position(|c| c.context_type == context_type)
            };

            let Some(idx) = idx else { continue };

            // Remove ctx from vec to get &mut ContextElement and &mut State simultaneously
            let mut ctx = state.context.remove(idx);
            let changed = panel.apply_cache_update(update, &mut ctx, state);
            state.context.insert(idx, ctx);

            if changed {
                state.dirty = true;
            }
        }
    }

    /// Process file watcher events
    fn process_watcher_events(&mut self) {
        // Collect events (immutable borrow on file_watcher released after this block)
        let events = {
            let Some(watcher) = &self.file_watcher else { return };
            watcher.poll_events()
        };
        if events.is_empty() { return; }

        // First pass: mark deprecated, collect indices and paths needing re-watch
        let mut refresh_indices = Vec::new();
        let mut rewatch_paths: Vec<String> = Vec::new();
        for event in &events {
            match event {
                WatchEvent::FileChanged(path) => {
                    // Check if this is a .git/ file change (HEAD, index)
                    let is_git_event = path.starts_with(".git/") || self.watched_git_paths.contains(path.as_str());
                    if is_git_event {
                        for (i, ctx) in self.state.context.iter_mut().enumerate() {
                            if ctx.context_type == ContextType::GitResult || ctx.context_type == ContextType::Git {
                                ctx.cache_deprecated = true;
                                self.state.dirty = true;
                                refresh_indices.push(i);
                            }
                        }
                    } else {
                        for (i, ctx) in self.state.context.iter_mut().enumerate() {
                            if ctx.context_type == ContextType::File && ctx.file_path.as_deref() == Some(path.as_str()) {
                                ctx.cache_deprecated = true;
                                self.state.dirty = true;
                                refresh_indices.push(i);
                            }
                        }
                    }
                    rewatch_paths.push(path.clone());
                }
                WatchEvent::DirChanged(path) => {
                    // Check if this is a .git/ directory change
                    let is_git_event = path.starts_with(".git/") || self.watched_git_paths.contains(path.as_str());
                    if is_git_event {
                        for (i, ctx) in self.state.context.iter_mut().enumerate() {
                            if ctx.context_type == ContextType::GitResult || ctx.context_type == ContextType::Git {
                                ctx.cache_deprecated = true;
                                self.state.dirty = true;
                                refresh_indices.push(i);
                            }
                        }
                    } else {
                        for (i, ctx) in self.state.context.iter_mut().enumerate() {
                            if ctx.context_type == ContextType::Tree {
                                ctx.cache_deprecated = true;
                                self.state.dirty = true;
                                refresh_indices.push(i);
                            }
                        }
                    }
                }
            }
        }

        // Second pass: build and send requests
        for i in refresh_indices {
            let ctx = &self.state.context[i];
            let panel = crate::core::panels::get_panel(ctx.context_type);
            if let Some(request) = panel.build_cache_request(ctx, &self.state) {
                process_cache_request(request, self.cache_tx.clone());
            }
        }

        // Third pass: re-watch files to pick up new inodes after atomic rename
        // (editors like vim/vscode save via rename, which invalidates the inotify watch)
        if let Some(watcher) = &mut self.file_watcher {
            for path in rewatch_paths {
                let _ = watcher.rewatch_file(&path);
            }
        }
    }

    /// Check timer-based deprecation for glob, grep, tmux, git
    /// Also handles initial population for newly created context elements
    fn check_timer_based_deprecation(&mut self) {
        let current_ms = now_ms();

        // Only check every 100ms to avoid excessive work
        if current_ms.saturating_sub(self.last_timer_check_ms) < 100 {
            return;
        }
        self.last_timer_check_ms = current_ms;

        // Immutable pass: collect requests and file watcher setup needs
        let mut requests: Vec<(CacheRequest, Option<String>)> = Vec::new();

        for ctx in &self.state.context {
            let needs_initial = ctx.cached_content.is_none() && ctx.context_type.needs_cache();
            let explicitly_deprecated = ctx.cache_deprecated;

            let panel = crate::core::panels::get_panel(ctx.context_type);
            let timer_refresh = panel.cache_refresh_interval_ms()
                .map(|interval| current_ms.saturating_sub(ctx.last_refresh_ms) >= interval)
                .unwrap_or(false);

            if needs_initial || explicitly_deprecated || timer_refresh {
                if let Some(request) = panel.build_cache_request(ctx, &self.state) {
                    // For new File contexts, we also need to set up a watcher
                    let watcher_path = if needs_initial && ctx.context_type == ContextType::File {
                        ctx.file_path.clone()
                    } else {
                        None
                    };
                    requests.push((request, watcher_path));
                }
            }
        }

        // Mutable pass: set up watchers and send requests
        for (request, watcher_path) in requests {
            if let Some(path) = watcher_path {
                if let Some(watcher) = &mut self.file_watcher {
                    if !self.watched_file_paths.contains(&path) {
                        if watcher.watch_file(&path).is_ok() {
                            self.watched_file_paths.insert(path);
                        }
                    }
                }
            }
            process_cache_request(request, self.cache_tx.clone());
        }
    }

    /// Update spinner animation frame if there's active loading/streaming
    fn update_spinner_animation(&mut self) {
        // Check if there's any active operation that needs spinner animation
        let has_active_spinner = self.state.is_streaming
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

    /// Handle keyboard events when command palette is open
    fn handle_palette_event(&mut self, event: &event::Event) -> Option<Action> {
        use crossterm::event::{KeyCode, KeyModifiers};

        let event::Event::Key(key) = event else {
            return Some(Action::None);
        };

        match key.code {
            // Escape closes palette
            KeyCode::Esc => {
                self.command_palette.close();
                None
            }
            // Enter executes selected command
            KeyCode::Enter => {
                if let Some(cmd) = self.command_palette.get_selected() {
                    let id = cmd.id.clone();
                    self.command_palette.close();

                    // Handle different command types
                    match id.as_str() {
                        "quit" => return None, // Signal quit
                        "reload" => {
                            // Perform reload (sets reload_requested flag and exits)
                            perform_reload(&mut self.state);
                            return None; // Won't reach here, but needed for type system
                        }
                        "config" => return Some(Action::ToggleConfigView),
                        _ if id.starts_with('P') => {
                            return Some(Action::SelectContextById(id));
                        }
                        _ => {}
                    }
                }
                Some(Action::None)
            }
            // Up/Down navigate results
            KeyCode::Up => {
                self.command_palette.select_prev();
                None
            }
            KeyCode::Down => {
                self.command_palette.select_next();
                None
            }
            // Left/Right move cursor in query
            KeyCode::Left => {
                self.command_palette.cursor_left();
                None
            }
            KeyCode::Right => {
                self.command_palette.cursor_right();
                None
            }
            // Home/End for cursor
            KeyCode::Home => {
                self.command_palette.cursor = 0;
                None
            }
            KeyCode::End => {
                self.command_palette.cursor = self.command_palette.query.len();
                None
            }
            // Backspace/Delete
            KeyCode::Backspace => {
                self.command_palette.backspace(&self.state);
                None
            }
            KeyCode::Delete => {
                self.command_palette.delete(&self.state);
                None
            }
            // Character input
            KeyCode::Char(c) => {
                // Ignore Ctrl+char combinations
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    return None;
                }
                self.command_palette.insert_char(c, &self.state);
                None
            }
            // Tab could cycle through results
            KeyCode::Tab => {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    self.command_palette.select_prev();
                } else {
                    self.command_palette.select_next();
                }
                None
            }
            _ => None,
        }
    }
}
