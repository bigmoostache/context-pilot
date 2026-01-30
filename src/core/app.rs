use std::io;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture};
use crossterm::ExecutableCommand;
use ratatui::prelude::*;

use crate::actions::{apply_action, Action, ActionResult};
use crate::api::{start_cleaning, start_streaming, StreamEvent};
use crate::background::{generate_tldr, TlDrResult};
use crate::constants::{CLEANING_TARGET, EVENT_POLL_MS, MAX_CLEANING_ITERATIONS};
use crate::context_cleaner;
use crate::events::handle_event;
use crate::persistence::{save_message, save_state};
use crate::state::{Message, MessageStatus, MessageType, State, ToolResultRecord, ToolUseRecord};
use crate::tools::{execute_tool, ToolResult, ToolUse};
use crate::typewriter::TypewriterBuffer;
use crate::ui;

use super::context::prepare_stream_context;

pub struct App {
    pub state: State,
    typewriter: TypewriterBuffer,
    pending_done: Option<(usize, usize)>,
    pending_tools: Vec<ToolUse>,
    cleaning_pending_done: Option<(usize, usize)>,
    cleaning_pending_tools: Vec<ToolUse>,
    cleaning_iterations: u32,
    mouse_captured: bool,
}

impl App {
    pub fn new(state: State) -> Self {
        Self {
            state,
            typewriter: TypewriterBuffer::new(),
            pending_done: None,
            pending_tools: Vec::new(),
            cleaning_pending_done: None,
            cleaning_pending_tools: Vec::new(),
            cleaning_iterations: 0,
            mouse_captured: true,
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
    ) -> io::Result<()> {
        loop {
            self.process_stream_events(&rx);
            self.process_typewriter();
            self.process_cleaning_events(&clean_rx, &clean_tx);
            self.process_tldr_results(&tldr_rx);
            self.handle_tool_execution(&tx, &tldr_tx, &clean_tx);
            self.finalize_stream(&tldr_tx, &clean_tx);
            self.update_mouse_capture()?;

            // Render
            terminal.draw(|frame| ui::render(frame, &mut self.state))?;

            // Poll events
            if event::poll(Duration::from_millis(EVENT_POLL_MS))? {
                let evt = event::read()?;

                let Some(action) = handle_event(&evt, &self.state) else {
                    save_state(&self.state);
                    break;
                };

                self.handle_action(action, &tx, &tldr_tx, &clean_tx);
            }
        }

        Ok(())
    }

    fn process_stream_events(&mut self, rx: &Receiver<StreamEvent>) {
        while let Ok(evt) = rx.try_recv() {
            if !self.state.is_streaming {
                continue;
            }
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
                    apply_action(&mut self.state, Action::StreamError(e));
                }
            }
        }
    }

    fn process_typewriter(&mut self) {
        if self.state.is_streaming {
            if let Some(chars) = self.typewriter.take_chars() {
                apply_action(&mut self.state, Action::AppendChars(chars));
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

            save_state(&self.state);

            let (_, usage_pct) = context_cleaner::calculate_context_usage(&self.state);
            if usage_pct < CLEANING_TARGET || self.cleaning_iterations >= MAX_CLEANING_ITERATIONS {
                self.state.is_cleaning_context = false;
                self.cleaning_pending_done = None;
                self.cleaning_iterations = 0;
            } else {
                let ctx = prepare_stream_context(&mut self.state, true);
                let cleaner_tools = context_cleaner::get_cleaner_tools();
                self.cleaning_pending_done = None;
                start_cleaning(
                    ctx.messages, ctx.file_context, ctx.glob_context, ctx.tmux_context,
                    ctx.todo_context, ctx.memory_context, ctx.overview_context, ctx.directory_tree,
                    cleaner_tools, &self.state, clean_tx.clone(),
                );
            }
        }

        // Finalize cleaning
        if self.state.is_cleaning_context && self.cleaning_pending_done.is_some() && self.cleaning_pending_tools.is_empty() {
            self.state.is_cleaning_context = false;
            self.cleaning_pending_done = None;
            self.cleaning_iterations = 0;
            save_state(&self.state);
        }
    }

    fn process_tldr_results(&mut self, tldr_rx: &Receiver<TlDrResult>) {
        while let Ok(tldr) = tldr_rx.try_recv() {
            self.state.pending_tldrs = self.state.pending_tldrs.saturating_sub(1);
            if let Some(msg) = self.state.messages.iter_mut().find(|m| m.id == tldr.message_id) {
                msg.tl_dr = Some(tldr.tl_dr);
                msg.tl_dr_token_count = tldr.token_count;
                save_message(msg);
            }
        }
    }

    fn handle_tool_execution(&mut self, tx: &Sender<StreamEvent>, tldr_tx: &Sender<TlDrResult>, clean_tx: &Sender<StreamEvent>) {
        if !self.state.is_streaming || self.pending_done.is_none() || !self.typewriter.pending_chars.is_empty() || self.pending_tools.is_empty() {
            return;
        }

        let tools = std::mem::take(&mut self.pending_tools);
        let mut tool_results: Vec<ToolResult> = Vec::new();

        // Finalize current assistant message
        if let Some(msg) = self.state.messages.last_mut() {
            if msg.role == "assistant" {
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
                ctx.messages, ctx.file_context, ctx.glob_context, ctx.tmux_context,
                ctx.todo_context, ctx.memory_context, ctx.overview_context, ctx.directory_tree,
                cleaner_tools, &self.state, clean_tx.clone(),
            );
        }

        // Continue streaming
        let ctx = prepare_stream_context(&mut self.state, true);
        self.typewriter.reset();
        self.pending_done = None;
        start_streaming(
            ctx.messages, ctx.file_context, ctx.glob_context, ctx.tmux_context,
            ctx.todo_context, ctx.memory_context, ctx.overview_context, ctx.directory_tree,
            ctx.tools, None, tx.clone(),
        );
    }

    fn finalize_stream(&mut self, tldr_tx: &Sender<TlDrResult>, clean_tx: &Sender<StreamEvent>) {
        if !self.state.is_streaming {
            return;
        }

        if let Some((input_tokens, output_tokens)) = self.pending_done {
            if self.typewriter.pending_chars.is_empty() && self.pending_tools.is_empty() {
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
                        ctx.messages, ctx.file_context, ctx.glob_context, ctx.tmux_context,
                        ctx.todo_context, ctx.memory_context, ctx.overview_context, ctx.directory_tree,
                        cleaner_tools, &self.state, clean_tx.clone(),
                    );
                }
            }
        }
    }

    fn update_mouse_capture(&mut self) -> io::Result<()> {
        if self.state.copy_mode && self.mouse_captured {
            io::stdout().execute(DisableMouseCapture)?;
            self.mouse_captured = false;
        } else if !self.state.copy_mode && !self.mouse_captured {
            io::stdout().execute(EnableMouseCapture)?;
            self.mouse_captured = true;
        }
        Ok(())
    }

    fn handle_action(
        &mut self,
        action: Action,
        tx: &Sender<StreamEvent>,
        tldr_tx: &Sender<TlDrResult>,
        clean_tx: &Sender<StreamEvent>,
    ) {
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
                    ctx.messages, ctx.file_context, ctx.glob_context, ctx.tmux_context,
                    ctx.todo_context, ctx.memory_context, ctx.overview_context, ctx.directory_tree,
                    ctx.tools, None, tx.clone(),
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
                    ctx.messages, ctx.file_context, ctx.glob_context, ctx.tmux_context,
                    ctx.todo_context, ctx.memory_context, ctx.overview_context, ctx.directory_tree,
                    cleaner_tools, &self.state, clean_tx.clone(),
                );
                save_state(&self.state);
            }
            ActionResult::Nothing => {}
        }
    }
}
