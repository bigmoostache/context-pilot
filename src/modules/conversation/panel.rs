use std::rc::Rc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use cp_render::Block;

use crate::app::actions::Action;
use crate::app::panels::{ContextItem, Panel};
use crate::state::{FullCache, InputCache, Kind, MessageCache, MsgKind, MsgStatus, State, hash_values};
use cp_base::panels::scroll_key_action;

use super::list::{self, ListAction};
use super::render_blocks::{self, MessageBlockOpts};
use super::render_input_blocks::{self, InputBlockCtx};
use cp_base::cast::Safe as _;

/// Panel for displaying the conversation messages and user input.
pub(super) struct ConversationPanel;

impl ConversationPanel {
    /// Compute hash for message cache invalidation
    fn compute_message_hash(msg: &crate::state::Message, viewport_width: u16, dev_mode: bool) -> u64 {
        // Include all fields that affect rendering
        let status_num = match msg.status {
            MsgStatus::Full => 0u8,
            MsgStatus::Deleted => 2,
            MsgStatus::Detached => 3,
        };
        let tool_uses_len = msg.tool_uses.len();
        let tool_results_len = msg.tool_results.len();

        hash_values(&[
            msg.content.as_str(),
            &format!(
                "{}{}{}{}{}{}",
                status_num,
                viewport_width,
                u8::from(dev_mode),
                tool_uses_len,
                tool_results_len,
                msg.input_tokens
            ),
        ])
    }

    /// Compute hash for input cache invalidation
    fn compute_input_hash(input: &str, cursor: usize, anchor: Option<usize>, viewport_width: u16) -> u64 {
        let anchor_str = anchor.map_or_else(String::new, |a| a.to_string());
        hash_values(&[input, &format!("{cursor}{anchor_str}{viewport_width}")])
    }

    /// Compute a hash of all content that affects rendering
    fn compute_full_content_hash(state: &State, viewport_width: u16) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();

        // Hash viewport width
        std::hash::Hash::hash(&viewport_width, &mut hasher);
        std::hash::Hash::hash(&state.flags.ui.dev_mode, &mut hasher);
        std::hash::Hash::hash(&state.flags.stream.phase.is_streaming(), &mut hasher);

        // Hash conversation history panel count (invalidate when panels added/removed)
        let history_count =
            state.context.iter().filter(|c| c.context_type.as_str() == Kind::CONVERSATION_HISTORY).count();
        std::hash::Hash::hash(&history_count, &mut hasher);

        // Hash all message content that affects rendering
        for msg in &state.messages {
            std::hash::Hash::hash(&msg.id, &mut hasher);
            std::hash::Hash::hash(&msg.content, &mut hasher);
            std::hash::Hash::hash(&msg.role, &mut hasher);
            std::hash::Hash::hash(&msg.status, &mut hasher);
            std::hash::Hash::hash(&msg.tool_uses.len(), &mut hasher);
            std::hash::Hash::hash(&msg.tool_results.len(), &mut hasher);
            std::hash::Hash::hash(&msg.input_tokens, &mut hasher);
        }

        // Hash streaming tool state (invalidate when tool preview changes)
        if let Some(st) = &(state.streaming_tool) {
            std::hash::Hash::hash(&st.name, &mut hasher);
            std::hash::Hash::hash(&st.input_so_far, &mut hasher);
        }

        // Hash input
        std::hash::Hash::hash(&state.input, &mut hasher);
        std::hash::Hash::hash(&state.input_cursor, &mut hasher);
        std::hash::Hash::hash(&state.input_selection_anchor, &mut hasher);

        std::hash::Hasher::finish(&hasher)
    }

    /// Prepend frozen `ConversationHistory` panels (oldest first) as separator-
    /// wrapped message blocks.
    fn push_history_panels(state: &State, blocks: &mut Vec<Block>, viewport_width: u16) {
        let mut history_panels: Vec<_> =
            state.context.iter().filter(|c| c.context_type.as_str() == Kind::CONVERSATION_HISTORY).collect();
        history_panels.sort_by_key(|c| c.last_refresh_ms);

        for ctx in &history_panels {
            let Some(msgs) = &ctx.history_messages else { continue };
            // Separator header
            blocks.push(Block::line(vec![
                cp_render::Span::styled(format!("── {} ──", ctx.name), cp_render::Semantic::Muted).bold(),
            ]));
            for msg in msgs {
                let rendered = render_blocks::render_message_blocks(
                    msg,
                    &MessageBlockOpts { viewport_width, is_streaming: false, dev_mode: state.flags.ui.dev_mode },
                );
                blocks.extend(rendered);
            }
            // Separator footer
            blocks.push(Block::line(vec![cp_render::Span::styled(
                "\u{2500}\u{2500} \u{2500}\u{2500} \u{2500}\u{2500} \u{2500}\u{2500}".to_owned(),
                cp_render::Semantic::Muted,
            )]));
            blocks.push(Block::empty());
        }
    }

    /// Render the live conversation messages (with per-message caching), skipping
    /// deleted + empty-non-streaming messages.
    fn push_message_blocks(state: &mut State, blocks: &mut Vec<Block>, viewport_width: u16) {
        let last_msg_id = state.messages.last().map(|m| m.id.clone());
        for msg in &state.messages {
            if msg.status == MsgStatus::Deleted {
                continue;
            }
            let is_last = last_msg_id.as_ref() == Some(&msg.id);
            let is_streaming_this = state.flags.stream.phase.is_streaming() && is_last && msg.role == "assistant";

            // Skip empty text messages (unless streaming)
            if msg.msg_type == MsgKind::TextMessage && msg.content.trim().is_empty() && !is_streaming_this {
                continue;
            }

            let hash = Self::compute_message_hash(msg, viewport_width, state.flags.ui.dev_mode);
            if let Some(cached) = state.message_cache.get(&msg.id)
                && cached.content_hash == hash
                && cached.viewport_width == viewport_width
            {
                blocks.extend(cached.blocks.iter().cloned());
                continue;
            }

            let rendered = render_blocks::render_message_blocks(
                msg,
                &MessageBlockOpts {
                    viewport_width,
                    is_streaming: is_streaming_this,
                    dev_mode: state.flags.ui.dev_mode,
                },
            );
            if !is_streaming_this {
                let _r = state
                    .message_cache
                    .insert(msg.id.clone(), MessageCache::new(Rc::from(rendered.as_slice()), hash, viewport_width));
            }
            blocks.extend(rendered);
        }
    }

    /// Render the input area fresh and store it in the input cache (miss path).
    fn render_and_cache_input(state: &mut State, input_hash: u64, viewport_width: u16) -> Vec<Block> {
        let command_ids: Vec<String> =
            cp_mod_prompt::storage::load_prompts_for(cp_mod_prompt::types::PromptType::Command)
                .iter()
                .map(|c| c.id.clone())
                .collect();
        let input_blocks = render_input_blocks::render_input_blocks(
            &state.input,
            state.input_cursor,
            state.input_selection_anchor,
            &InputBlockCtx {
                command_ids: &command_ids,
                paste_buffers: &state.paste_buffers,
                paste_buffer_labels: &state.paste_buffer_labels,
                viewport_width,
            },
        );
        state.input_cache = Some(InputCache::new(Rc::from(input_blocks.as_slice()), input_hash, viewport_width));
        input_blocks
    }

    /// Render the input area (cached by input hash), updating the autocomplete
    /// popup's visual-line count. Renders fresh + stores on cache miss.
    fn push_input_area(state: &mut State, blocks: &mut Vec<Block>, viewport_width: u16) {
        let input_hash =
            Self::compute_input_hash(&state.input, state.input_cursor, state.input_selection_anchor, viewport_width);

        let cache_hit = state
            .input_cache
            .as_ref()
            .is_some_and(|c| c.input_hash == input_hash && c.viewport_width == viewport_width);

        let out_blocks: Vec<Block> = if cache_hit {
            state.input_cache.as_ref().map_or_else(Vec::new, |c| c.blocks.to_vec())
        } else {
            Self::render_and_cache_input(state, input_hash, viewport_width)
        };

        let block_count = out_blocks.len();
        blocks.extend(out_blocks);
        if let Some(ac) = state.get_ext_mut::<cp_base::state::autocomplete::Suggestions>() {
            ac.input_visual_lines = block_count.to_u16();
        }
    }

    /// Assemble the conversation body: history panels, messages (or an empty
    /// placeholder), the streaming-tool preview, and the input area.
    fn assemble_body(state: &mut State, blocks: &mut Vec<Block>, viewport_width: u16) {
        Self::push_history_panels(state, blocks, viewport_width);

        if state.messages.is_empty() {
            blocks.push(Block::empty());
            blocks.push(Block::empty());
            blocks.push(Block::line(vec![
                cp_render::Span::styled(
                    "  Start a conversation by typing below".to_owned(),
                    cp_render::Semantic::Muted,
                )
                .italic(),
            ]));
        } else {
            Self::push_message_blocks(state, blocks, viewport_width);
        }

        // Streaming tool preview (between messages and input)
        if let Some(streaming_tool) = &(state.streaming_tool) {
            blocks.extend(render_blocks::render_streaming_tool_blocks(
                &streaming_tool.name,
                &streaming_tool.input_so_far,
                viewport_width,
            ));
        }

        Self::push_input_area(state, blocks, viewport_width);
    }

    /// Build content with caching - called from `render()` which has &mut State
    fn build_content_cached_inner(state: &mut State) -> Vec<Block> {
        let _guard = crate::profile!("panel::conversation::content");
        let viewport_width = state.last_viewport_width;

        // Compute full content hash for top-level cache check
        let full_hash = Self::compute_full_content_hash(state, viewport_width);

        // Check full content cache first - if valid, return immediately
        if let Some(cached) = &state.full_content_cache
            && cached.content_hash == full_hash
        {
            return cached.blocks.to_vec();
        }

        // Cache miss — viewport width change invalidates per-message caches
        let width_changed = state.message_cache.values().next().is_some_and(|c| c.viewport_width != viewport_width);
        if width_changed {
            state.message_cache.clear();
            state.input_cache = None;
        }

        let mut blocks: Vec<Block> = Vec::new();
        Self::assemble_body(state, &mut blocks, viewport_width);

        // Padding at end for scroll
        for _ in 0i32..3i32 {
            blocks.push(Block::empty());
        }

        // Store in full content cache
        state.full_content_cache = Some(FullCache::new(Rc::from(blocks.as_slice()), full_hash));

        blocks
    }
}

impl Panel for ConversationPanel {
    // Conversations are sent to the API as messages, not as context items
    fn max_freezes(&self) -> u8 {
        0
    }

    fn context(&self, _state: &State) -> Vec<ContextItem> {
        Vec::new()
    }

    fn blocks(&self, _state: &State) -> Vec<Block> {
        Vec::new()
    }
    fn title(&self, state: &State) -> String {
        if state.flags.stream.phase.is_streaming() { "Conversation *".to_owned() } else { "Conversation".to_owned() }
    }

    fn handle_key(&self, key: &KeyEvent, state: &State) -> Option<Action> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let alt = key.modifiers.contains(KeyModifiers::ALT);

        // Modifier-combo shortcuts (Ctrl+Backspace/A, Ctrl/Alt+Arrow word jump)
        if let Some(action) = handle_modifier_combo(key.code, &Mods { ctrl, shift, alt }) {
            return Some(action);
        }

        // Regular typing and editing
        match key.code {
            KeyCode::Char(c) => Some(Action::InputChar(c)),
            KeyCode::Backspace => Some(Action::InputBackspace),
            KeyCode::Delete => Some(Action::InputDelete),
            KeyCode::Left if shift => Some(Action::CursorLeftSelect),
            KeyCode::Left => Some(Action::CursorLeft),
            KeyCode::Right if shift => Some(Action::CursorRightSelect),
            KeyCode::Right => Some(Action::CursorRight),
            KeyCode::Enter => Some(handle_enter_key(state)),
            KeyCode::Home if shift => Some(Action::CursorHomeSelect),
            KeyCode::Home => Some(Action::CursorHome),
            KeyCode::End if shift => Some(Action::CursorEndSelect),
            KeyCode::End => Some(Action::CursorEnd),
            // Remaining variants: delegate scroll keys, ignore everything else
            KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown => scroll_key_action(key),
            KeyCode::Tab
            | KeyCode::BackTab
            | KeyCode::Insert
            | KeyCode::F(_)
            | KeyCode::Null
            | KeyCode::Esc
            | KeyCode::CapsLock
            | KeyCode::ScrollLock
            | KeyCode::NumLock
            | KeyCode::PrintScreen
            | KeyCode::Pause
            | KeyCode::Menu
            | KeyCode::KeypadBegin
            | KeyCode::Media(_)
            | KeyCode::Modifier(_) => None,
        }
    }

    fn refresh(&self, _state: &mut State) {}

    fn needs_cache(&self) -> bool {
        false
    }

    fn refresh_cache(&self, _request: cp_base::panels::CacheRequest) -> Option<cp_base::panels::CacheUpdate> {
        None
    }

    fn build_cache_request(&self, _ctx: &crate::state::Entry, _state: &State) -> Option<cp_base::panels::CacheRequest> {
        None
    }

    fn apply_cache_update(
        &self,
        _update: cp_base::panels::CacheUpdate,
        _ctx: &mut crate::state::Entry,
        _state: &mut State,
    ) -> bool {
        false
    }

    fn cache_refresh_interval_ms(&self) -> Option<u64> {
        None
    }

    fn suicide(&self, _ctx: &crate::state::Entry, _state: &State) -> bool {
        false
    }
}

/// Word-jump action for a `Ctrl`/`Alt`+arrow, selecting when `shift` is held.
const fn word_arrow_action(is_left: bool, shift: bool) -> Action {
    match (is_left, shift) {
        (true, true) => Action::CursorWordLeftSelect,
        (true, false) => Action::CursorWordLeft,
        (false, true) => Action::CursorWordRightSelect,
        (false, false) => Action::CursorWordRight,
    }
}

/// Active keyboard modifier flags for a key event.
struct Mods {
    /// Control held.
    ctrl: bool,
    /// Shift held.
    shift: bool,
    /// Alt held.
    alt: bool,
}

/// Resolve modifier-combo shortcuts (`Ctrl+Backspace`/`Ctrl+A`, `Ctrl`/`Alt`+
/// arrow word jump with optional `Shift` selection). `None` falls through to the
/// plain key match.
const fn handle_modifier_combo(code: KeyCode, mods: &Mods) -> Option<Action> {
    let word_mod = mods.ctrl || mods.alt;
    match code {
        KeyCode::Backspace if mods.ctrl => Some(Action::DeleteWordLeft),
        KeyCode::Char('a') if mods.ctrl => Some(Action::SelectAll),
        KeyCode::Left if word_mod => Some(word_arrow_action(true, mods.shift)),
        KeyCode::Right if word_mod => Some(word_arrow_action(false, mods.shift)),
        KeyCode::Backspace
        | KeyCode::Char(_)
        | KeyCode::Enter
        | KeyCode::Left
        | KeyCode::Right
        | KeyCode::Up
        | KeyCode::Down
        | KeyCode::Home
        | KeyCode::End
        | KeyCode::PageUp
        | KeyCode::PageDown
        | KeyCode::Tab
        | KeyCode::BackTab
        | KeyCode::Delete
        | KeyCode::Insert
        | KeyCode::F(_)
        | KeyCode::Null
        | KeyCode::Esc
        | KeyCode::CapsLock
        | KeyCode::ScrollLock
        | KeyCode::NumLock
        | KeyCode::PrintScreen
        | KeyCode::Pause
        | KeyCode::Menu
        | KeyCode::KeypadBegin
        | KeyCode::Media(_)
        | KeyCode::Modifier(_) => None,
    }
}

/// Resolve the `Enter` key: send on an empty trailing line at end-of-input,
/// else continue/close a markdown list, else insert a newline.
fn handle_enter_key(state: &State) -> Action {
    // Send if: cursor at end AND (input empty OR ends with empty line)
    let at_end = state.input_cursor >= state.input.len();
    let ends_with_empty_line =
        state.input.ends_with('\n') || state.input.lines().last().is_none_or(|l| l.trim().is_empty());

    if at_end && ends_with_empty_line {
        return Action::InputSubmit;
    }
    match list::detect_list_action(&state.input) {
        Some(ListAction::Continue(text)) => Action::InsertText(text),
        Some(ListAction::RemoveItem) => Action::RemoveListItem,
        None => Action::InputChar('\n'),
    }
}

/// Public entry point for the cached conversation content builder.
///
/// Delegates to [`ConversationPanel::build_content_cached_inner`], which
/// has the full multi-level cache (full → per-message → input).
/// Returns IR blocks — the TUI adapter converts to ratatui lines.
pub(crate) fn build_content_cached(state: &mut State) -> Vec<Block> {
    ConversationPanel::build_content_cached_inner(state)
}
