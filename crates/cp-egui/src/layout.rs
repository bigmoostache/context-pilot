//! Frame layout — maps [`cp_render::frame::Frame`] to egui regions.
//!
//! The top-level [`render_frame`] function divides the window into four
//! regions: left sidebar, bottom status bar, right panel content, and
//! central conversation area.

use cp_render::Semantic;
use cp_render::frame::{Frame, HelpHint, PanelContent, Sidebar, SidebarEntry, SidebarMode, StatusBar, TokenBar};
use eframe::egui::{self, Color32, RichText, ScrollArea, Ui};

use crate::renderers::render_blocks;
use crate::theme::{BODY_FONT_SIZE, HEADER_FONT_SIZE, semantic_color};

/// Sidebar width in normal mode.
const SIDEBAR_WIDTH: f32 = 260.0;
/// Sidebar width in collapsed (icon-only) mode.
const SIDEBAR_COLLAPSED_WIDTH: f32 = 40.0;
/// Status bar height.
const STATUS_BAR_HEIGHT: f32 = 24.0;

// ── Top-level layout ─────────────────────────────────────────────────

/// Render a full [`Frame`] into the egui context.
///
/// Creates: left sidebar, bottom status bar, central panel + conversation.
pub fn render_frame(ctx: &egui::Context, frame: &Frame) {
    // Sidebar (left).
    render_sidebar(ctx, &frame.sidebar);

    // Status bar (bottom).
    render_status_bar(ctx, &frame.status_bar);

    // Central area: panel content + conversation.
    drop(egui::CentralPanel::default().show(ctx, |ui| {
        render_central_area(ui, &frame.active_panel, &frame.conversation);
    }));
}

// ── Sidebar ──────────────────────────────────────────────────────────

/// Render the left sidebar panel.
fn render_sidebar(ctx: &egui::Context, sidebar: &Sidebar) {
    match sidebar.mode {
        SidebarMode::Hidden => {}
        SidebarMode::Collapsed => {
            drop(egui::SidePanel::left("sidebar").exact_width(SIDEBAR_COLLAPSED_WIDTH).resizable(false).show(
                ctx,
                |ui| {
                    render_sidebar_collapsed(ui, sidebar);
                },
            ));
        }
        SidebarMode::Normal => {
            drop(egui::SidePanel::left("sidebar").exact_width(SIDEBAR_WIDTH).resizable(false).show(ctx, |ui| {
                render_sidebar_normal(ui, sidebar);
            }));
        }
    }
}

/// Render full sidebar: entries, token bar, help hints.
fn render_sidebar_normal(ui: &mut Ui, sidebar: &Sidebar) {
    // Token bar at the top.
    if let Some(ref token_bar) = sidebar.token_bar {
        render_token_bar(ui, token_bar);
        ui.add_space(4.0);
        drop(ui.separator());
    }

    // Scrollable entry list.
    let _ = ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        for entry in &sidebar.entries {
            render_sidebar_entry(ui, entry);
        }
    });

    // Help hints at the bottom.
    if !sidebar.help_hints.is_empty() {
        drop(ui.separator());
        render_help_hints(ui, &sidebar.help_hints);
    }
}

/// Render collapsed sidebar: icon strip only.
fn render_sidebar_collapsed(ui: &mut Ui, sidebar: &Sidebar) {
    let _ = ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        for entry in &sidebar.entries {
            let color = if entry.active { semantic_color(Semantic::Accent) } else { semantic_color(Semantic::Muted) };
            drop(ui.label(RichText::new(&entry.icon).color(color).size(BODY_FONT_SIZE)));
        }
    });
}

/// Render a single sidebar entry row.
fn render_sidebar_entry(ui: &mut Ui, entry: &SidebarEntry) {
    let bg = entry.active.then(|| Color32::from_rgba_premultiplied(0, 215, 255, 20));

    let frame = egui::Frame::NONE.inner_margin(egui::Margin::symmetric(4, 2));
    let frame = bg.map_or(frame, |bg_color| frame.fill(bg_color));

    drop(frame.show(ui, |ui| {
        drop(ui.horizontal(|ui| {
            // Icon.
            let icon_color =
                if entry.active { semantic_color(Semantic::Accent) } else { semantic_color(Semantic::Muted) };
            drop(ui.label(RichText::new(&entry.icon).color(icon_color)));

            // ID badge.
            drop(ui.label(RichText::new(&entry.id).color(semantic_color(Semantic::Muted)).size(11.0)));

            // Label (truncated).
            let label_color =
                if entry.active { semantic_color(Semantic::Default) } else { semantic_color(Semantic::Muted) };
            drop(ui.label(RichText::new(&entry.label).color(label_color)));

            // Right-aligned token count + badge.
            drop(ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Badge (if any).
                if let Some(ref badge) = entry.badge {
                    drop(ui.label(RichText::new(badge).color(semantic_color(Semantic::Warning)).size(10.0)));
                }
                // Token count.
                drop(ui.label(
                    RichText::new(format_tokens(entry.tokens)).color(semantic_color(Semantic::Muted)).size(10.0),
                ));
                // Frozen indicator.
                if entry.frozen {
                    drop(ui.label(RichText::new("❄").color(semantic_color(Semantic::Info)).size(10.0)));
                }
            }));
        }));
    }));
}

/// Render the token usage gauge bar.
fn render_token_bar(ui: &mut Ui, token: &TokenBar) {
    // Label: "42K / 200K".
    let used_k = f64::from(token.used) / 1000.0;
    let budget_k = f64::from(token.budget) / 1000.0;
    let label = format!("{used_k:.0}K / {budget_k:.0}K");
    drop(ui.label(RichText::new(label).color(semantic_color(Semantic::Muted)).size(11.0)));

    // Reuse progress bar renderer from renderers module.
    let segments: Vec<cp_render::ProgressSegment> = token
        .segments
        .iter()
        .map(|s| cp_render::ProgressSegment { percent: s.percent, semantic: s.semantic, label: s.label.clone() })
        .collect();
    crate::renderers::render_progress_bar_raw(ui, &segments, 12.0);
}

/// Render keyboard help hints.
fn render_help_hints(ui: &mut Ui, hints: &[HelpHint]) {
    for hint in hints {
        drop(ui.horizontal(|ui| {
            drop(ui.label(RichText::new(&hint.key).color(semantic_color(Semantic::KeyHint)).size(11.0).strong()));
            drop(ui.label(RichText::new(&hint.description).color(semantic_color(Semantic::Muted)).size(11.0)));
        }));
    }
}

// ── Status bar ───────────────────────────────────────────────────────

/// Render the bottom status bar.
fn render_status_bar(ctx: &egui::Context, status: &StatusBar) {
    drop(egui::TopBottomPanel::bottom("status_bar").exact_height(STATUS_BAR_HEIGHT).show(ctx, |ui| {
        drop(ui.horizontal_centered(|ui| {
            // Primary badge.
            drop(ui.label(
                RichText::new(&status.badge.label).color(semantic_color(status.badge.semantic)).strong().size(12.0),
            ));

            drop(ui.separator());

            // Provider + model.
            if let Some(ref provider) = status.provider {
                drop(ui.label(RichText::new(provider).color(semantic_color(Semantic::Muted)).size(12.0)));
            }
            if let Some(ref model) = status.model {
                drop(ui.label(RichText::new(model).color(semantic_color(Semantic::Default)).size(12.0)));
            }

            // Agent card.
            if let Some(ref agent) = status.agent {
                drop(ui.separator());
                drop(ui.label(
                    RichText::new(format!("⚓ {}", agent.name)).color(semantic_color(Semantic::Accent)).size(12.0),
                ));
            }

            // Git changes.
            if let Some(ref git) = status.git {
                drop(ui.separator());
                drop(ui.label(RichText::new(&git.branch).color(semantic_color(Semantic::Info)).size(12.0)));
                if git.files_changed > 0 {
                    drop(
                        ui.label(
                            RichText::new(format!("+{} -{}", git.additions, git.deletions))
                                .color(semantic_color(Semantic::Muted))
                                .size(11.0),
                        ),
                    );
                }
            }

            // Reverie cards.
            for rev in &status.reveries {
                drop(ui.separator());
                drop(
                    ui.label(
                        RichText::new(format!("🔮 {} ({})", rev.agent, rev.tool_count))
                            .color(semantic_color(Semantic::AccentDim))
                            .size(12.0),
                    ),
                );
            }

            // Queue card.
            if let Some(ref queue) = status.queue {
                drop(ui.separator());
                let q_text = if queue.active {
                    format!("📋 Queue: {}", queue.count)
                } else {
                    format!("📋 Queue: {} (paused)", queue.count)
                };
                drop(ui.label(RichText::new(q_text).color(semantic_color(Semantic::Warning)).size(12.0)));
            }

            // Right-aligned: stop reason + loading + input chars.
            drop(ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if status.input_char_count > 0 {
                    drop(
                        ui.label(
                            RichText::new(format!("{}ch", status.input_char_count))
                                .color(semantic_color(Semantic::Muted))
                                .size(11.0),
                        ),
                    );
                }
                if status.loading_count > 0 {
                    drop(
                        ui.label(
                            RichText::new(format!("⟳{}", status.loading_count))
                                .color(semantic_color(Semantic::Warning))
                                .size(11.0),
                        ),
                    );
                }
                if let Some(ref sr) = status.stop_reason {
                    drop(ui.label(RichText::new(&sr.reason).color(semantic_color(sr.semantic)).size(11.0)));
                }
            }));
        }));
    }));
}

// ── Central area ─────────────────────────────────────────────────────

/// Render the central area: panel content + conversation.
fn render_central_area(ui: &mut Ui, panel: &PanelContent, conversation: &cp_render::conversation::Conversation) {
    // Split: conversation on top, panel on bottom.
    let available = ui.available_height();
    let conversation_height = available * 0.55;

    // Conversation region.
    drop(ui.allocate_ui(egui::Vec2::new(ui.available_width(), conversation_height), |ui| {
        render_conversation(ui, conversation);
    }));

    drop(ui.separator());

    // Panel content region.
    render_panel_content(ui, panel);
}

/// Render the active panel content.
fn render_panel_content(ui: &mut Ui, panel: &PanelContent) {
    // Panel title bar.
    drop(ui.horizontal(|ui| {
        drop(ui.label(
            RichText::new(&panel.title).color(semantic_color(Semantic::Accent)).size(HEADER_FONT_SIZE).strong(),
        ));
        if let Some(ref ago) = panel.refreshed_ago {
            drop(ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                drop(ui.label(RichText::new(ago).color(semantic_color(Semantic::Muted)).size(11.0)));
            }));
        }
    }));

    drop(ui.separator());

    // Scrollable block content.
    let _ = ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
        render_blocks(ui, &panel.blocks);
    });
}

/// Render the conversation region (messages + input).
fn render_conversation(ui: &mut Ui, conversation: &cp_render::conversation::Conversation) {
    let _ = ScrollArea::vertical().auto_shrink(false).stick_to_bottom(true).show(ui, |ui| {
        // Messages.
        for msg in &conversation.messages {
            render_message(ui, msg);
            ui.add_space(4.0);
        }

        // Streaming tool calls.
        for tool in &conversation.streaming_tools {
            drop(ui.horizontal(|ui| {
                drop(ui.label(
                    RichText::new(format!("⚙ {}", tool.tool_name)).color(semantic_color(Semantic::Warning)).strong(),
                ));
                drop(ui.label(RichText::new(&tool.partial_input).color(semantic_color(Semantic::Muted)).size(12.0)));
            }));
        }
    });

    // Input area.
    render_input_area(ui, &conversation.input);
}

/// Render a single conversation message.
fn render_message(ui: &mut Ui, msg: &cp_render::conversation::Message) {
    let (role_label, role_color) = match msg.role.as_str() {
        "user" => ("You", semantic_color(Semantic::Success)),
        "assistant" => ("Assistant", semantic_color(Semantic::Accent)),
        _ => ("System", semantic_color(Semantic::Muted)),
    };

    // Role header.
    drop(ui.label(RichText::new(role_label).color(role_color).strong().size(13.0)));

    // Content blocks.
    render_blocks(ui, &msg.content);

    // Tool use previews.
    for tool in &msg.tool_uses {
        drop(ui.horizontal(|ui| {
            drop(
                ui.label(
                    RichText::new(format!("⚙ {}", tool.tool_name)).color(semantic_color(tool.semantic)).size(12.0),
                ),
            );
            drop(ui.label(RichText::new(&tool.summary).color(semantic_color(Semantic::Muted)).size(12.0)));
        }));
    }

    // Tool result previews.
    for result in &msg.tool_results {
        let icon = if result.success { "✓" } else { "✗" };
        let color = if result.success { semantic_color(Semantic::Success) } else { semantic_color(Semantic::Error) };
        drop(ui.horizontal(|ui| {
            drop(ui.label(RichText::new(format!("{icon} {}", result.tool_name)).color(color).size(12.0)));
            drop(ui.label(RichText::new(&result.summary).color(semantic_color(Semantic::Muted)).size(12.0)));
        }));
    }
}

/// Render the input area.
fn render_input_area(ui: &mut Ui, input: &cp_render::conversation::InputArea) {
    drop(ui.separator());
    drop(ui.horizontal(|ui| {
        drop(ui.label(RichText::new("›").color(semantic_color(Semantic::Accent)).strong()));
        // Read-only display of current input text (actual editing is Phase 5).
        if input.text.is_empty() {
            drop(ui.label(RichText::new(&input.placeholder).color(semantic_color(Semantic::Muted)).italics()));
        } else {
            drop(ui.label(RichText::new(&input.text).color(semantic_color(Semantic::Default))));
        }
    }));
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Format a token count: `1234` → `"1.2K"`, `0` → `"0"`.
fn format_tokens(tokens: u32) -> String {
    if tokens >= 1000 {
        // Float division for display rounding — no integer truncation concerns.
        let k = f64::from(tokens) / 1000.0;
        format!("{k:.1}K")
    } else {
        tokens.to_string()
    }
}
