//! MCP setup overlay: builder + renderer.
//!
//! The overlay lets users list, add, and remove MCP servers from the TUI.
//! Toggled with a keybinding (wired in actions). State lives in
//! [`McpSetupState`](cp_mod_mcp::bridge::setup::McpSetupState).

use ratatui::{
    prelude::{Frame, Line, Rect, Span, Style},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use cp_render::mcp_overlay_ir::{
    McpFormIR, McpMode, McpSetupOverlay, McpServerType,
};
use cp_render::Semantic;

use crate::infra::constants::{chars, theme};

/// State → IR builder.
pub(crate) mod builder;
/// Keyboard input handler.
pub(crate) mod input;

/// Render the MCP setup overlay centered on the given area.
///
/// Consumes the pre-built IR [`McpSetupOverlay`] snapshot — no direct state access.
pub(crate) fn render_mcp_setup_overlay(
    frame: &mut Frame<'_>,
    overlay: &McpSetupOverlay,
    area: Rect,
) {
    let overlay_width = 72u16.min(area.width.saturating_sub(4));
    let content_height = estimate_content_height(overlay);
    let overlay_height = content_height.saturating_add(4).min(area.height.saturating_sub(2));
    let x = area.x.saturating_add(area.width.saturating_sub(overlay_width) >> 1);
    let y = area.y.saturating_add(area.height.saturating_sub(overlay_height) >> 1);
    let overlay_area = Rect::new(x, y, overlay_width, overlay_height);

    let mut lines: Vec<Line<'_>> = Vec::new();

    match overlay.mode {
        McpMode::List | McpMode::ConfirmDelete => {
            render_server_list(&mut lines, overlay, overlay_width);
        }
        McpMode::AddForm => {
            if let Some(form) = &overlay.form {
                render_add_form(&mut lines, form, overlay_width);
            }
        }
        McpMode::OAuthPending => {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled("⏳ ", Style::default().fg(theme::warning())),
                Span::styled(
                    "Waiting for browser OAuth flow…",
                    Style::default().fg(theme::text()),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::styled(
                    "     Check your browser to complete authentication.",
                    Style::default().fg(theme::text_muted()),
                ),
            ]));
            lines.push(Line::from(""));
        }
    }

    // Error / success messages
    if let Some(err) = &overlay.error {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("  ✗ ", Style::default().fg(theme::error())),
            Span::styled(err.clone(), Style::default().fg(theme::error())),
        ]));
    }
    if let Some(ok) = &overlay.success {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("  ✓ ", Style::default().fg(theme::success())),
            Span::styled(ok.clone(), Style::default().fg(theme::success())),
        ]));
    }

    // Footer
    add_separator(&mut lines, overlay_width);
    lines.push(Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled(&overlay.footer, Style::default().fg(theme::text_muted())),
    ]));

    let title = match overlay.mode {
        McpMode::AddForm => " Add MCP Server ",
        McpMode::List | McpMode::ConfirmDelete | McpMode::OAuthPending => " MCP Server Setup ",
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::accent()))
        .style(Style::default().bg(theme::bg_surface()))
        .title(Span::styled(title, Style::default().fg(theme::accent()).bold()));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(Clear, overlay_area);
    frame.render_widget(paragraph, overlay_area);
}

// ── Server list mode ────────────────────────────────────────────────────────

/// Render the server list table.
fn render_server_list(lines: &mut Vec<Line<'_>>, overlay: &McpSetupOverlay, _width: u16) {
    if overlay.servers.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(
                "  No MCP servers configured.",
                Style::default().fg(theme::text_muted()),
            ),
        ]));
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(
                "  Press 'a' to add a server, or configure via mcp.json",
                Style::default().fg(theme::text_muted()),
            ),
        ]));
        lines.push(Line::from(""));
        return;
    }

    // Header row
    lines.push(Line::from(vec![
        Span::styled(
            format!("  {:<16} {:<6} {:<24} {:<7} {:<10}", "Name", "Type", "Status", "Auth", "Scope"),
            Style::default().fg(theme::text_secondary()).bold(),
        ),
    ]));
    add_separator(lines, 70);

    // Data rows
    for server in &overlay.servers {
        let indicator = if server.selected { "▸ " } else { "  " };
        let status_color = semantic_color(server.status_semantic);

        let row_style = if server.selected {
            Style::default().fg(theme::accent())
        } else {
            Style::default().fg(theme::text())
        };

        // Scope gets a distinct color: project = accent, global = muted
        let (scope_icon, scope_color) = match server.scope.as_str() {
            "project" => ("📁 ", theme::accent()),
            "global" => ("🌐 ", theme::text_muted()),
            _ => ("   ", theme::text_muted()),
        };

        lines.push(Line::from(vec![
            Span::styled(indicator, Style::default().fg(theme::accent())),
            Span::styled(format!("{:<16} ", server.name), row_style.bold()),
            Span::styled(format!("{:<6} ", server.server_type), row_style),
            Span::styled(format!("{:<24} ", server.status_label), Style::default().fg(status_color)),
            Span::styled(format!("{:<7} ", server.auth_label), Style::default().fg(theme::text_muted())),
            Span::styled(scope_icon, Style::default().fg(scope_color)),
            Span::styled(format!("{:<7}", server.scope), Style::default().fg(scope_color)),
        ]));
    }

    // Confirm delete prompt
    if overlay.mode == McpMode::ConfirmDelete {
        lines.push(Line::from(""));
        let target = overlay
            .servers
            .iter()
            .find(|s| s.selected)
            .map_or("?", |s| &s.name);
        lines.push(Line::from(vec![
            Span::styled("  ⚠ ", Style::default().fg(theme::warning())),
            Span::styled(
                format!("Delete '{target}'?  "),
                Style::default().fg(theme::warning()).bold(),
            ),
            Span::styled("y", Style::default().fg(theme::success()).bold()),
            Span::styled(" confirm  ", Style::default().fg(theme::text_muted())),
            Span::styled("n", Style::default().fg(theme::error()).bold()),
            Span::styled(" cancel", Style::default().fg(theme::text_muted())),
        ]));
    }
}

// ── Add form mode ───────────────────────────────────────────────────────────

/// Render the add-server form.
fn render_add_form<'line>(lines: &mut Vec<Line<'line>>, form: &'line McpFormIR, _width: u16) {
    lines.push(Line::from(""));

    // Name field
    render_text_field(lines, &form.name);

    // Server type selector
    render_selector(
        lines,
        "Type",
        form.server_type.label(),
        form.focused_field == 1,
    );

    // Type-specific fields
    match form.server_type {
        McpServerType::Stdio => {
            render_text_field(lines, &form.command);
            render_text_field(lines, &form.args);
        }
        McpServerType::Http => {
            render_text_field(lines, &form.url);
            render_selector(
                lines,
                "Auth Mode",
                form.auth_mode.label(),
                // auth_mode is field index 3 for http
                form.focused_field == 3,
            );
            if form.bearer_token.visible {
                render_text_field(lines, &form.bearer_token);
            }
        }
    }

    // Scope selector
    render_selector(
        lines,
        "Scope",
        form.scope.label(),
        form.focused_field == form.field_count.saturating_sub(1),
    );

    lines.push(Line::from(""));
}

/// Render a text input field.
fn render_text_field<'line>(lines: &mut Vec<Line<'line>>, field: &'line cp_render::mcp_overlay_ir::McpFormField) {
    if !field.visible {
        return;
    }
    let label_style = Style::default().fg(theme::text_secondary()).bold();
    let (bracket_l, bracket_r, value_style) = if field.focused {
        (
            Style::default().fg(theme::accent()),
            Style::default().fg(theme::accent()),
            Style::default().fg(theme::text()),
        )
    } else {
        (
            Style::default().fg(theme::border()),
            Style::default().fg(theme::border()),
            Style::default().fg(theme::text()),
        )
    };

    let display_value = if field.value.is_empty() {
        Span::styled(&field.placeholder, Style::default().fg(theme::text_muted()))
    } else {
        Span::styled(&field.value, value_style)
    };

    let cursor = if field.focused { "▏" } else { "" };

    lines.push(Line::from(vec![
        Span::styled(format!("  {:<14}", format!("{}:", field.label)), label_style),
        Span::styled("[", bracket_l),
        display_value,
        Span::styled(cursor, Style::default().fg(theme::accent())),
        Span::styled("]", bracket_r),
    ]));
}

/// Render a selector field (cycle with Space).
fn render_selector<'line>(lines: &mut Vec<Line<'line>>, label: &str, value: &'line str, focused: bool) {
    let label_style = Style::default().fg(theme::text_secondary()).bold();
    let (arrow_style, value_style) = if focused {
        (
            Style::default().fg(theme::accent()),
            Style::default().fg(theme::accent()).bold(),
        )
    } else {
        (
            Style::default().fg(theme::text_muted()),
            Style::default().fg(theme::text()),
        )
    };

    lines.push(Line::from(vec![
        Span::styled(format!("  {:<14}", format!("{label}:")), label_style),
        Span::styled("◄ ", arrow_style),
        Span::styled(value, value_style),
        Span::styled(" ►", arrow_style),
    ]));
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Append a horizontal separator line.
fn add_separator(lines: &mut Vec<Line<'_>>, width: u16) {
    let repeat = (width as usize).saturating_sub(6);
    lines.push(Line::from(vec![Span::styled(
        format!("  {}", chars::HORIZONTAL.repeat(repeat)),
        Style::default().fg(theme::border()),
    )]));
}

/// Map a [`Semantic`] colour variant to a ratatui [`Color`](ratatui::prelude::Color).
fn semantic_color(semantic: Semantic) -> ratatui::prelude::Color {
    match semantic {
        Semantic::Success => theme::success(),
        Semantic::Error => theme::error(),
        Semantic::Warning => theme::warning(),
        Semantic::Info | Semantic::Accent | Semantic::AccentDim => theme::accent(),
        Semantic::Muted => theme::text_muted(),
        // Default, Active, KeyHint, Code, DiffAdd/Remove, Header, Border, Bold,
        // plus any future variants (Semantic is #[non_exhaustive]).
        Semantic::Default
        | Semantic::Active
        | Semantic::KeyHint
        | Semantic::Code
        | Semantic::DiffAdd
        | Semantic::DiffRemove
        | Semantic::Header
        | Semantic::Border
        | Semantic::Bold
        | _ => theme::text(),
    }
}

/// Estimate content lines for height calculation.
fn estimate_content_height(overlay: &McpSetupOverlay) -> u16 {
    let base: u16 = match overlay.mode {
        McpMode::List | McpMode::ConfirmDelete => {
            if overlay.servers.is_empty() {
                5 // empty state message
            } else {
                let rows = u16::try_from(overlay.servers.len()).unwrap_or(u16::MAX);
                let confirm_extra: u16 = if overlay.mode == McpMode::ConfirmDelete { 2 } else { 0 };
                2u16.saturating_add(rows).saturating_add(confirm_extra)
            }
        }
        McpMode::AddForm => {
            8 // form fields estimate
        }
        McpMode::OAuthPending => {
            4
        }
    };

    let messages = overlay.error.as_ref().map_or(0u16, |_| 2u16)
        .saturating_add(overlay.success.as_ref().map_or(0u16, |_| 2u16));

    base.saturating_add(messages).saturating_add(3) // +3 for separator + footer + padding
}
