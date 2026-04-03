//! Demo frame — builds a representative [`Frame`] for visual testing.
//!
//! Used by `App` when no live `State` is wired yet. Populates all
//! regions (sidebar, status bar, panel, conversation) with sample data
//! so the layout and renderers can be verified visually.

use cp_render::conversation::{Conversation, InputArea, Message, StreamingTool, ToolResultPreview, ToolUsePreview};
use cp_render::frame::{
    AgentCard, Badge, Frame, GitChanges, HelpHint, PanelContent, QueueCard, Sidebar, SidebarEntry, SidebarMode,
    StatusBar, TokenBar,
};
use cp_render::{Align, Block, Cell, ProgressSegment, Semantic, Span, TreeNode};

/// Build a static demo [`Frame`] with sample data for all regions.
#[must_use]
pub fn build_demo_frame() -> Frame {
    Frame {
        sidebar: demo_sidebar(),
        status_bar: demo_status_bar(),
        active_panel: demo_panel(),
        conversation: demo_conversation(),
        overlays: vec![],
    }
}

// ── Sidebar ──────────────────────────────────────────────────────────

/// Build a demo sidebar with sample entries and token bar.
fn demo_sidebar() -> Sidebar {
    Sidebar {
        mode: SidebarMode::Normal,
        entries: vec![
            SidebarEntry {
                id: "P1".into(),
                icon: "📋".into(),
                label: "Todo List".into(),
                tokens: 1240,
                active: false,
                frozen: false,
                fixed: true,
                badge: Some("3/7".into()),
            },
            SidebarEntry {
                id: "P2".into(),
                icon: "📚".into(),
                label: "Library".into(),
                tokens: 392,
                active: false,
                frozen: false,
                fixed: true,
                badge: None,
            },
            SidebarEntry {
                id: "P5".into(),
                icon: "🌲".into(),
                label: "Directory Tree".into(),
                tokens: 5174,
                active: true,
                frozen: false,
                fixed: true,
                badge: None,
            },
            SidebarEntry {
                id: "P6".into(),
                icon: "🧠".into(),
                label: "Memories".into(),
                tokens: 2454,
                active: false,
                frozen: false,
                fixed: true,
                badge: Some("22".into()),
            },
            SidebarEntry {
                id: "P8".into(),
                icon: "📜".into(),
                label: "Logs".into(),
                tokens: 9738,
                active: false,
                frozen: true,
                fixed: true,
                badge: None,
            },
            SidebarEntry {
                id: "P14".into(),
                icon: "📄".into(),
                label: "app.rs".into(),
                tokens: 501,
                active: false,
                frozen: false,
                fixed: false,
                badge: None,
            },
        ],
        token_bar: Some(TokenBar {
            used: 53800,
            budget: 200_000,
            threshold: 140_000,
            segments: vec![
                ProgressSegment { percent: 7, semantic: Semantic::Info, label: Some("system".into()) },
                ProgressSegment { percent: 6, semantic: Semantic::Muted, label: Some("tools".into()) },
                ProgressSegment { percent: 14, semantic: Semantic::Accent, label: Some("panels".into()) },
            ],
        }),
        token_stats: None,
        pr_card: None,
        help_hints: vec![
            HelpHint { key: "Tab".into(), description: "Cycle panels".into() },
            HelpHint { key: "Ctrl+B".into(), description: "Toggle sidebar".into() },
            HelpHint { key: "Enter".into(), description: "Send message".into() },
        ],
    }
}

// ── Status bar ───────────────────────────────────────────────────────

/// Build a demo status bar with provider, model, and git info.
fn demo_status_bar() -> StatusBar {
    StatusBar {
        badge: Badge { label: "READY".into(), semantic: Semantic::Success },
        provider: Some("Anthropic".into()),
        model: Some("claude-sonnet-4-20250514".into()),
        agent: Some(AgentCard { name: "Pirate Coder".into() }),
        skills: vec![],
        git: Some(GitChanges { branch: "egui-frontend".into(), files_changed: 3, additions: 648, deletions: 12 }),
        auto_continue: None,
        reveries: vec![],
        queue: Some(QueueCard { active: false, count: 0 }),
        stop_reason: None,
        retry_count: 0,
        max_retries: 3,
        loading_count: 0,
        input_char_count: 0,
    }
}

// ── Panel content ────────────────────────────────────────────────────

/// Build a demo panel with tree, table, and progress bar blocks.
fn demo_panel() -> PanelContent {
    PanelContent {
        title: "🌲 Directory Tree".into(),
        refreshed_ago: Some("2s ago".into()),
        blocks: vec![
            Block::header("Project Structure".into()),
            Block::Tree(vec![
                TreeNode {
                    label: vec![Span::accent("crates/".into())],
                    expanded: true,
                    children: vec![
                        TreeNode {
                            label: vec![Span::accent("cp-egui/".into())],
                            expanded: true,
                            children: vec![
                                TreeNode {
                                    label: vec![Span::new("app.rs".into()), Span::muted("  — App struct".into())],
                                    expanded: false,
                                    children: vec![],
                                },
                                TreeNode {
                                    label: vec![Span::new("layout.rs".into()), Span::muted("  — Frame layout".into())],
                                    expanded: false,
                                    children: vec![],
                                },
                                TreeNode {
                                    label: vec![
                                        Span::new("renderers.rs".into()),
                                        Span::muted("  — Block renderers".into()),
                                    ],
                                    expanded: false,
                                    children: vec![],
                                },
                                TreeNode {
                                    label: vec![
                                        Span::new("theme.rs".into()),
                                        Span::muted("  — Semantic styles".into()),
                                    ],
                                    expanded: false,
                                    children: vec![],
                                },
                            ],
                        },
                        TreeNode {
                            label: vec![Span::accent("cp-render/".into()), Span::muted("  — IR types".into())],
                            expanded: false,
                            children: vec![TreeNode {
                                label: vec![Span::new("lib.rs".into())],
                                expanded: false,
                                children: vec![],
                            }],
                        },
                    ],
                },
                TreeNode {
                    label: vec![Span::accent("src/".into()), Span::muted("  — Main binary".into())],
                    expanded: false,
                    children: vec![],
                },
            ]),
            Block::separator(),
            Block::header("Build Status".into()),
            Block::table(
                vec![("Check", Align::Left), ("Status", Align::Center), ("Time", Align::Right)],
                vec![
                    vec![
                        Cell::text("cargo check".into()),
                        Cell::styled("✓ pass".into(), Semantic::Success),
                        Cell::right(Span::muted("0.8s".into())),
                    ],
                    vec![
                        Cell::text("cargo clippy".into()),
                        Cell::styled("✓ pass".into(), Semantic::Success),
                        Cell::right(Span::muted("1.2s".into())),
                    ],
                    vec![
                        Cell::text("cargo test".into()),
                        Cell::styled("✓ 50/50".into(), Semantic::Success),
                        Cell::right(Span::muted("3.4s".into())),
                    ],
                    vec![
                        Cell::text("cargo fmt".into()),
                        Cell::styled("✓ clean".into(), Semantic::Success),
                        Cell::right(Span::muted("0.3s".into())),
                    ],
                ],
            ),
            Block::empty(),
            Block::header("Token Usage".into()),
            Block::ProgressBar {
                segments: vec![
                    ProgressSegment { percent: 27, semantic: Semantic::Accent, label: Some("27%".into()) },
                    ProgressSegment { percent: 5, semantic: Semantic::Warning, label: None },
                ],
                label: Some("53.8K / 200K tokens".into()),
            },
        ],
    }
}

// ── Conversation ─────────────────────────────────────────────────────

/// Build a demo conversation with user/assistant messages and tool previews.
fn demo_conversation() -> Conversation {
    Conversation {
        history_sections: vec![],
        messages: vec![
            Message {
                role: "user".into(),
                content: vec![Block::text("Can you create the egui frontend scaffolding with a dark theme?".into())],
                tool_uses: vec![],
                tool_results: vec![],
            },
            Message {
                role: "assistant".into(),
                content: vec![
                    Block::text("Arr captain! Setting sail on the egui voyage! 🏴\u{200d}☠️".into()),
                    Block::empty(),
                    Block::text("I'll create the crate scaffold with:".into()),
                    Block::Line(vec![
                        Span::muted("  • ".into()),
                        Span::accent("app.rs".into()),
                        Span::new(" — App struct + update loop".into()),
                    ]),
                    Block::Line(vec![
                        Span::muted("  • ".into()),
                        Span::accent("theme.rs".into()),
                        Span::new(" — Semantic → Color32 mapping".into()),
                    ]),
                    Block::Line(vec![
                        Span::muted("  • ".into()),
                        Span::accent("renderers.rs".into()),
                        Span::new(" — Block variant renderers".into()),
                    ]),
                    Block::Line(vec![
                        Span::muted("  • ".into()),
                        Span::accent("layout.rs".into()),
                        Span::new(" — Frame → egui regions".into()),
                    ]),
                ],
                tool_uses: vec![
                    ToolUsePreview {
                        tool_name: "Write".into(),
                        summary: "crates/cp-egui/src/app.rs (62 lines)".into(),
                        semantic: Semantic::Warning,
                    },
                    ToolUsePreview {
                        tool_name: "Write".into(),
                        summary: "crates/cp-egui/src/theme.rs (96 lines)".into(),
                        semantic: Semantic::Warning,
                    },
                ],
                tool_results: vec![
                    ToolResultPreview { tool_name: "Write".into(), summary: "Created app.rs".into(), success: true },
                    ToolResultPreview { tool_name: "Write".into(), summary: "Created theme.rs".into(), success: true },
                ],
            },
            Message {
                role: "user".into(),
                content: vec![Block::text("Looks good! Can you run the checks?".into())],
                tool_uses: vec![],
                tool_results: vec![],
            },
            Message {
                role: "assistant".into(),
                content: vec![Block::Line(vec![
                    Span::success("All checks pass! ".into()),
                    Span::new("The winds of compilation be favorable! ⛵".into()),
                ])],
                tool_uses: vec![ToolUsePreview {
                    tool_name: "console_easy_bash".into(),
                    summary: "cargo check -p cp-egui".into(),
                    semantic: Semantic::Success,
                }],
                tool_results: vec![ToolResultPreview {
                    tool_name: "console_easy_bash".into(),
                    summary: "exit 0 (1.2s)".into(),
                    success: true,
                }],
            },
        ],
        streaming_tools: vec![StreamingTool {
            tool_name: "Edit".into(),
            partial_input: "{\"file_path\": \"crates/cp-egui/src/...".into(),
        }],
        input: InputArea {
            text: String::new(),
            cursor: 0,
            placeholder: "Ask me anything, captain...".into(),
            focused: true,
        },
    }
}
