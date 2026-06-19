//! Keyboard input handler for the MCP setup overlay.
//!
//! Dispatches key events to mode-specific handlers (list, add-form,
//! confirm-delete, oauth-pending). Returns `true` when the event was consumed.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use cp_mod_mcp::bridge::config::ServerSpec;
use cp_mod_mcp::bridge::servers::McpState;
use cp_mod_mcp::bridge::setup::{
    FormField, McpSetupState, Mode,
};
use cp_mod_mcp::bridge::form_enums::{AuthMode, Scope, ServerType};
use cp_mod_mcp::bridge::McpModule;

use crate::modules::rebuild_tools;
use crate::state::State;

/// Handle a key event when the MCP setup overlay is visible.
///
/// Returns `true` if the event was consumed, `false` to pass through.
pub(crate) fn handle_mcp_overlay_input(key: KeyEvent, state: &mut State) -> bool {
    let mode = McpSetupState::get(state).mode;
    match mode {
        Mode::List => handle_list(key, state),
        Mode::AddForm => handle_form(key, state),
        Mode::ConfirmDelete => handle_confirm_delete(key, state),
        Mode::OAuthPending => handle_oauth_pending(key, state),
    }
}

// ── List mode ───────────────────────────────────────────────────────────────

/// Navigate server list, trigger add/delete/reconnect.
fn handle_list(key: KeyEvent, state: &mut State) -> bool {
    match key.code {
        KeyCode::Esc => {
            McpSetupState::get_mut(state).toggle();
            true
        }
        KeyCode::Up | KeyCode::Char('k') => {
            let count = {
                let mcp = McpState::get(state);
                mcp.servers.len()
            };
            let setup = McpSetupState::get_mut(state);
            if count > 0 {
                setup.selected_index = if setup.selected_index == 0 {
                    count.saturating_sub(1)
                } else {
                    setup.selected_index.saturating_sub(1)
                };
            }
            setup.clear_messages();
            true
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let count = {
                let mcp = McpState::get(state);
                mcp.servers.len()
            };
            let setup = McpSetupState::get_mut(state);
            if count > 0 {
                let next = setup.selected_index.saturating_add(1);
                setup.selected_index = if next >= count { 0 } else { next };
            }
            setup.clear_messages();
            true
        }
        KeyCode::Char('a') => {
            McpSetupState::get_mut(state).start_add();
            true
        }
        KeyCode::Char('d') => {
            let server_name = selected_server_name(state);
            if let Some(name) = server_name {
                McpSetupState::get_mut(state).start_delete(name);
            }
            true
        }
        KeyCode::Char('r') => {
            let server_name = selected_server_name(state);
            if let Some(name) = server_name {
                McpSetupState::get_mut(state).clear_messages();
                McpModule::force_reconnect(&name, state);
                rebuild_tools(state);
                McpSetupState::get_mut(state).success =
                    Some(format!("Reconnected '{name}'"));
            }
            true
        }
        // Consume all keys when overlay is open
        KeyCode::Backspace
        | KeyCode::Enter
        | KeyCode::Left
        | KeyCode::Right
        | KeyCode::Home
        | KeyCode::End
        | KeyCode::PageUp
        | KeyCode::PageDown
        | KeyCode::Tab
        | KeyCode::BackTab
        | KeyCode::Delete
        | KeyCode::Insert
        | KeyCode::F(_)
        | KeyCode::Char(_)
        | KeyCode::Null
        | KeyCode::CapsLock
        | KeyCode::ScrollLock
        | KeyCode::NumLock
        | KeyCode::PrintScreen
        | KeyCode::Pause
        | KeyCode::Menu
        | KeyCode::KeypadBegin
        | KeyCode::Media(_)
        | KeyCode::Modifier(_) => true,
    }
}

// ── Add form mode ───────────────────────────────────────────────────────────

/// Navigate form fields, type text, cycle selectors, save or cancel.
fn handle_form(key: KeyEvent, state: &mut State) -> bool {
    match key.code {
        KeyCode::Esc => {
            McpSetupState::get_mut(state).cancel();
            true
        }
        KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => {
            if let Some(form) = &mut McpSetupState::get_mut(state).form {
                form.focus_prev();
            }
            true
        }
        KeyCode::Tab | KeyCode::Down => {
            if let Some(form) = &mut McpSetupState::get_mut(state).form {
                form.focus_next();
            }
            true
        }
        KeyCode::Up => {
            if let Some(form) = &mut McpSetupState::get_mut(state).form {
                form.focus_prev();
            }
            true
        }
        KeyCode::Char(' ') => {
            handle_selector_cycle(state);
            true
        }
        KeyCode::Enter => {
            handle_form_submit(state);
            true
        }
        KeyCode::Backspace => {
            if let Some(form) = &mut McpSetupState::get_mut(state).form {
                form.delete_char();
            }
            true
        }
        KeyCode::Char(c) => {
            handle_form_char(c, state);
            true
        }
        KeyCode::Left => {
            handle_form_left(state);
            true
        }
        KeyCode::Right => {
            handle_form_right(state);
            true
        }
        KeyCode::Home => {
            if let Some(form) = &mut McpSetupState::get_mut(state).form {
                form.move_cursor_home();
            }
            true
        }
        KeyCode::End => {
            if let Some(form) = &mut McpSetupState::get_mut(state).form {
                form.move_cursor_end();
            }
            true
        }
        // Consume all keys when form is open
        KeyCode::PageUp
        | KeyCode::PageDown
        | KeyCode::BackTab
        | KeyCode::Delete
        | KeyCode::Insert
        | KeyCode::F(_)
        | KeyCode::Null
        | KeyCode::CapsLock
        | KeyCode::ScrollLock
        | KeyCode::NumLock
        | KeyCode::PrintScreen
        | KeyCode::Pause
        | KeyCode::Menu
        | KeyCode::KeypadBegin
        | KeyCode::Media(_)
        | KeyCode::Modifier(_) => true,
    }
}

/// Insert a character into the focused text field, or ignore for selectors.
fn handle_form_char(c: char, state: &mut State) {
    let setup = McpSetupState::get_mut(state);
    let Some(form) = &mut setup.form else { return };
    let Some(field) = form.field_at(form.focused_field) else { return };
    if field.is_text() {
        form.insert_char(c);
    }
    // Selectors ignore character input
}

/// Cycle the focused selector field (server type, auth mode, scope).
fn handle_selector_cycle(state: &mut State) {
    handle_selector_cycle_dir(state, true);
}

/// Cycle the focused selector field in a given direction.
///
/// `forward`: `true` = next, `false` = previous.
fn handle_selector_cycle_dir(state: &mut State, forward: bool) {
    let setup = McpSetupState::get_mut(state);
    let Some(form) = &mut setup.form else { return };
    let Some(field) = form.field_at(form.focused_field) else { return };
    match field {
        FormField::ServerType => {
            form.server_type = if forward { form.server_type.next() } else { form.server_type.prev() };
            form.clamp_focus();
        }
        FormField::AuthMode => {
            form.auth_mode = if forward { form.auth_mode.next() } else { form.auth_mode.prev() };
            form.clamp_focus();
        }
        FormField::Scope => {
            form.scope = if forward { form.scope.next() } else { form.scope.prev() };
        }
        FormField::Name
        | FormField::Command
        | FormField::Args
        | FormField::Url
        | FormField::BearerToken => {} // text fields: space cycles nothing
    }
}

/// Handle Left arrow in the form: cursor left for text, cycle prev for selectors.
fn handle_form_left(state: &mut State) {
    let is_text = {
        let setup = McpSetupState::get(state);
        let Some(form) = &setup.form else { return };
        form.field_at(form.focused_field).is_some_and(FormField::is_text)
    };
    if is_text {
        if let Some(form) = &mut McpSetupState::get_mut(state).form {
            form.move_cursor_left();
        }
    } else {
        handle_selector_cycle_dir(state, false);
    }
}

/// Handle Right arrow in the form: cursor right for text, cycle next for selectors.
fn handle_form_right(state: &mut State) {
    let is_text = {
        let setup = McpSetupState::get(state);
        let Some(form) = &setup.form else { return };
        form.field_at(form.focused_field).is_some_and(FormField::is_text)
    };
    if is_text {
        if let Some(form) = &mut McpSetupState::get_mut(state).form {
            form.move_cursor_right();
        }
    } else {
        handle_selector_cycle_dir(state, true);
    }
}

/// Validate and submit the add-server form.
fn handle_form_submit(state: &mut State) {
    // Extract form data before mutable borrows
    let form_data = {
        let setup = McpSetupState::get(state);
        let Some(form) = &setup.form else { return };
        FormSnapshot {
            name: form.name.trim().to_string(),
            server_type: form.server_type,
            command: form.command.trim().to_string(),
            args: form.args.clone(),
            url: form.url.trim().to_string(),
            bearer_token: form.bearer_token.clone(),
            auth_mode: form.auth_mode,
            scope: form.scope,
        }
    };

    // Validate
    if form_data.name.is_empty() {
        McpSetupState::get_mut(state).error = Some("Server name is required".to_string());
        return;
    }

    match form_data.server_type {
        ServerType::Stdio => {
            if form_data.command.is_empty() {
                McpSetupState::get_mut(state).error =
                    Some("Command is required for stdio servers".to_string());
                return;
            }
        }
        ServerType::Http => {
            if form_data.url.is_empty() {
                McpSetupState::get_mut(state).error =
                    Some("URL is required for HTTP servers".to_string());
                return;
            }
        }
    }

    // Build ServerSpec
    let spec = match form_data.server_type {
        ServerType::Stdio => ServerSpec {
            command: Some(form_data.command),
            args: form_data
                .args
                .split_whitespace()
                .map(String::from)
                .collect(),
            url: None,
            bearer_token: None,
            auth: None,
            allow_tools: None,
            deny_tools: None,
        },
        ServerType::Http => ServerSpec {
            command: None,
            args: Vec::new(),
            url: Some(form_data.url),
            bearer_token: match form_data.auth_mode {
                AuthMode::Bearer => Some(form_data.bearer_token),
                AuthMode::None | AuthMode::OAuth => None,
            },
            auth: Some(form_data.auth_mode.label().to_string()),
            allow_tools: None,
            deny_tools: None,
        },
    };

    let to_project = form_data.scope == Scope::Project;
    let name = form_data.name;

    // Save and connect
    match McpModule::add_and_connect(&name, &spec, to_project, state) {
        Ok(()) => {
            rebuild_tools(state);
            let setup = McpSetupState::get_mut(state);
            setup.cancel(); // back to list
            setup.success = Some(format!("Added '{name}'"));
        }
        Err(e) => {
            McpSetupState::get_mut(state).error = Some(e);
        }
    }
}

/// Snapshot of form data for validation and submission.
struct FormSnapshot {
    /// Server name.
    name: String,
    /// Server type.
    server_type: ServerType,
    /// Command (stdio).
    command: String,
    /// Args string (stdio).
    args: String,
    /// URL (http).
    url: String,
    /// Bearer token (http + bearer).
    bearer_token: String,
    /// Auth mode.
    auth_mode: AuthMode,
    /// Config scope.
    scope: Scope,
}

// ── Confirm delete mode ─────────────────────────────────────────────────────

/// Handle y/n confirmation for server deletion.
///
/// Only `y`/`Y` confirms; every other key (including Esc) cancels.
fn handle_confirm_delete(key: KeyEvent, state: &mut State) -> bool {
    match key.code {
        KeyCode::Char('y' | 'Y') => {
            let target = McpSetupState::get(state)
                .delete_target
                .clone();
            if let Some(name) = target {
                match McpModule::remove_and_disconnect(&name, state) {
                    Ok(()) => {
                        rebuild_tools(state);
                        let setup_after = McpSetupState::get_mut(state);
                        setup_after.cancel();
                        setup_after.success = Some(format!("Deleted '{name}'"));
                        // Clamp selection index
                        let count = McpState::get(state).servers.len();
                        let setup_clamp = McpSetupState::get_mut(state);
                        if setup_clamp.selected_index >= count && count > 0 {
                            setup_clamp.selected_index = count.saturating_sub(1);
                        }
                    }
                    Err(e) => {
                        let setup_err = McpSetupState::get_mut(state);
                        setup_err.cancel();
                        setup_err.error = Some(e);
                    }
                }
            }
            true
        }
        // Any other key cancels the deletion (Esc, 'n', etc.)
        KeyCode::Esc
        | KeyCode::Char(_)
        | KeyCode::Backspace
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
        | KeyCode::CapsLock
        | KeyCode::ScrollLock
        | KeyCode::NumLock
        | KeyCode::PrintScreen
        | KeyCode::Pause
        | KeyCode::Menu
        | KeyCode::KeypadBegin
        | KeyCode::Media(_)
        | KeyCode::Modifier(_) => {
            McpSetupState::get_mut(state).cancel();
            true
        }
    }
}

// ── OAuth pending mode ──────────────────────────────────────────────────────

/// Only Esc to cancel during OAuth flow.
fn handle_oauth_pending(key: KeyEvent, state: &mut State) -> bool {
    if key.code == KeyCode::Esc {
        McpSetupState::get_mut(state).cancel();
    }
    true // consume all keys
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Get the name of the currently selected server in the list.
fn selected_server_name(state: &State) -> Option<String> {
    let mcp = McpState::get(state);
    let setup = McpSetupState::get(state);
    let mut names: Vec<&String> = mcp.servers.keys().collect();
    names.sort();
    names.get(setup.selected_index).map(|s| (*s).clone())
}
