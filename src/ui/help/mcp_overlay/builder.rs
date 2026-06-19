//! Builder: `State` → [`McpSetupOverlay`] IR.
//!
//! Reads [`McpState`] (live server connections) and [`McpSetupState`] (overlay
//! form state) to produce the rendering IR consumed by the overlay renderer.

use cp_mod_mcp::bridge::config;
use cp_mod_mcp::bridge::servers::{ConnStatus, McpState};
use cp_mod_mcp::bridge::setup::{McpSetupState, Mode};
use cp_render::mcp_overlay_ir::{
    McpFormField, McpAuthMode, McpFormIR, McpMode, McpSetupOverlay,
    McpScope, McpSetupServer, McpServerType,
};
use cp_render::Semantic;

use crate::state::State;

/// Build the [`McpSetupOverlay`] IR from current application state.
pub(crate) fn build_mcp_setup_overlay(state: &State) -> McpSetupOverlay {
    let mcp = McpState::get(state);
    let setup = McpSetupState::get(state);

    // Load both config layers to determine scope per server.
    let global_keys = config::load_global()
        .map(|m| m.servers.keys().cloned().collect::<std::collections::HashSet<_>>())
        .unwrap_or_default();
    let project_keys = config::load_project()
        .map(|m| m.servers.keys().cloned().collect::<std::collections::HashSet<_>>())
        .unwrap_or_default();

    // Build server list.
    let sorted_names = mcp.sorted_names();
    let servers: Vec<McpSetupServer> = sorted_names
        .iter()
        .enumerate()
        .filter_map(|(i, name)| {
            let entry = mcp.servers.get(name)?;

            let server_type = if entry.spec.as_ref().is_some_and(|s| s.url.is_some()) {
                "http"
            } else {
                "stdio"
            };

            let (status_label, status_semantic) = match &entry.status {
                ConnStatus::Connected { tool_count } => {
                    (format!("Connected ({tool_count} tools)"), Semantic::Success)
                }
                ConnStatus::Failed(msg) => {
                    let short = if msg.len() > 40 {
                        let truncated: String = msg.chars().take(40).collect();
                        format!("{truncated}…")
                    } else {
                        msg.clone()
                    };
                    (format!("Failed: {short}"), Semantic::Error)
                }
                ConnStatus::Unsupported(msg) => {
                    (format!("Unsupported: {msg}"), Semantic::Warning)
                }
            };

            let auth_label = entry
                .spec
                .as_ref()
                .and_then(|s| s.auth.as_deref())
                .unwrap_or_else(|| if entry.spec.as_ref().is_some_and(|s| s.bearer_token.is_some()) {
                    "bearer"
                } else if entry.spec.as_ref().is_some_and(|s| s.url.is_some()) {
                    "auto"
                } else {
                    "n/a"
                })
                .to_string();

            let scope = if project_keys.contains(name) {
                "project"
            } else if global_keys.contains(name) {
                "global"
            } else {
                "runtime"
            };

            Some(McpSetupServer {
                name: name.clone(),
                server_type: server_type.to_string(),
                status_label,
                status_semantic,
                auth_label,
                scope: scope.to_string(),
                tool_count: entry.tools.len(),
                selected: i == setup.selected_index,
            })
        })
        .collect();

    // Map mode.
    let mode = match setup.mode {
        Mode::List => McpMode::List,
        Mode::AddForm => McpMode::AddForm,
        Mode::ConfirmDelete => McpMode::ConfirmDelete,
        Mode::OAuthPending => McpMode::OAuthPending,
    };

    // Build form IR if in add mode.
    let form = setup.form.as_ref().map(build_form_ir);

    // Footer text based on mode.
    let footer = match setup.mode {
        Mode::List => {
            if servers.is_empty() {
                "a add server · Esc close".to_string()
            } else {
                "↑↓ navigate · a add · d delete · r reconnect · Esc close".to_string()
            }
        }
        Mode::AddForm => {
            "Tab/↑↓ navigate · Space toggle · Enter save · Esc cancel".to_string()
        }
        Mode::ConfirmDelete => {
            let target = setup.delete_target.as_deref().unwrap_or("?");
            format!("Delete '{target}'? y confirm · n cancel")
        }
        Mode::OAuthPending => "Waiting for browser OAuth flow… Esc cancel".to_string(),
    };

    McpSetupOverlay {
        title: "MCP Server Setup".to_string(),
        servers,
        selected_index: setup.selected_index,
        mode,
        form,
        error: setup.error.clone(),
        success: setup.success.clone(),
        footer,
    }
}

/// Map mutable form state → form IR snapshot.
fn build_form_ir(
    form: &cp_mod_mcp::bridge::setup::Form,
) -> McpFormIR {
    use cp_mod_mcp::bridge::setup::{FormField, AuthMode, Scope, ServerType};

    let visible_fields = form.visible_fields();
    let field_count = visible_fields.len();

    // Build IR for the "name" field.
    let name = build_field_ir(form, &visible_fields, FormField::Name);
    let command = build_field_ir(form, &visible_fields, FormField::Command);
    let args = build_field_ir(form, &visible_fields, FormField::Args);
    let url = build_field_ir(form, &visible_fields, FormField::Url);
    let bearer_token = build_field_ir(form, &visible_fields, FormField::BearerToken);

    let server_type = match form.server_type {
        ServerType::Stdio => McpServerType::Stdio,
        ServerType::Http => McpServerType::Http,
    };

    let auth_mode = match form.auth_mode {
        AuthMode::None => McpAuthMode::None,
        AuthMode::Bearer => McpAuthMode::Bearer,
        AuthMode::OAuth => McpAuthMode::OAuth,
    };

    let scope = match form.scope {
        Scope::Global => McpScope::Global,
        Scope::Project => McpScope::Project,
    };

    McpFormIR {
        title: "Add Server".to_string(),
        name,
        server_type,
        command,
        args,
        url,
        bearer_token,
        auth_mode,
        scope,
        focused_field: form.focused_field,
        field_count,
    }
}

/// Build a single `McpFormField` IR from the form state.
fn build_field_ir(
    form: &cp_mod_mcp::bridge::setup::Form,
    visible: &[cp_mod_mcp::bridge::setup::FormField],
    field: cp_mod_mcp::bridge::setup::FormField,
) -> McpFormField {
    let is_visible = visible.contains(&field);
    let focused_index = visible.iter().position(|f| *f == field);
    let is_focused = focused_index.is_some_and(|idx| idx == form.focused_field);

    let value = match field {
        cp_mod_mcp::bridge::setup::FormField::Name => form.name.clone(),
        cp_mod_mcp::bridge::setup::FormField::Command => form.command.clone(),
        cp_mod_mcp::bridge::setup::FormField::Args => form.args.clone(),
        cp_mod_mcp::bridge::setup::FormField::Url => form.url.clone(),
        cp_mod_mcp::bridge::setup::FormField::BearerToken => form.bearer_token.clone(),
        cp_mod_mcp::bridge::setup::FormField::ServerType
        | cp_mod_mcp::bridge::setup::FormField::AuthMode
        | cp_mod_mcp::bridge::setup::FormField::Scope => String::new(),
    };

    McpFormField {
        label: field.label().to_string(),
        value,
        placeholder: field.placeholder().unwrap_or("").to_string(),
        focused: is_focused,
        visible: is_visible,
    }
}
