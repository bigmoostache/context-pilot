use std::fs;

use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};

use crate::constants::MAX_CONTEXT_TOKENS;
use crate::highlight::highlight_file;
use crate::state::{ContextType, State, TodoStatus, MemoryImportance};
use crate::tool_defs::{ParamType, ToolCategory, ToolParam};
use crate::tools::{capture_pane_content, compute_glob_results, generate_directory_tree};
use super::{theme, chars, helpers::*};

pub fn render_file(frame: &mut Frame, state: &mut State, area: Rect) {
    let base_style = Style::default().bg(theme::BG_SURFACE);
    let selected = state.context.get(state.selected_context);

    let (title, content, file_path) = if let Some(ctx) = selected {
        let path = ctx.file_path.as_deref().unwrap_or("");
        let content = if !path.is_empty() {
            fs::read_to_string(path).unwrap_or_else(|e| format!("Error reading file: {}", e))
        } else {
            "No file path".to_string()
        };
        (ctx.name.clone(), content, path.to_string())
    } else {
        ("File".to_string(), String::new(), String::new())
    };

    let inner_area = Rect::new(
        area.x + 1,
        area.y,
        area.width.saturating_sub(2),
        area.height
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(theme::BORDER))
        .style(base_style)
        .title(Span::styled(format!(" {} ", title), Style::default().fg(theme::ACCENT).bold()));

    let content_area = block.inner(inner_area);
    frame.render_widget(block, inner_area);

    // Get syntax highlighting
    let highlighted = if !file_path.is_empty() {
        highlight_file(&file_path, &content)
    } else {
        Vec::new()
    };

    let mut text: Vec<Line> = Vec::new();

    if highlighted.is_empty() {
        for (i, line) in content.lines().enumerate() {
            let line_num = i + 1;
            text.push(Line::from(vec![
                Span::styled(format!(" {:4} ", line_num), Style::default().fg(theme::TEXT_MUTED).bg(theme::BG_BASE)),
                Span::styled(" ", base_style),
                Span::styled(line, Style::default().fg(theme::TEXT)),
            ]));
        }
    } else {
        for (i, spans) in highlighted.iter().enumerate() {
            let line_num = i + 1;
            let mut line_spans = vec![
                Span::styled(format!(" {:4} ", line_num), Style::default().fg(theme::TEXT_MUTED).bg(theme::BG_BASE)),
                Span::styled(" ", base_style),
            ];

            for (color, text) in spans {
                line_spans.push(Span::styled(text.clone(), Style::default().fg(*color)));
            }

            text.push(Line::from(line_spans));
        }
    }

    // Calculate and set max scroll
    let content_height = text.len();
    let viewport_height = content_area.height as usize;
    let max_scroll = content_height.saturating_sub(viewport_height) as f32;
    state.max_scroll = max_scroll;
    state.scroll_offset = state.scroll_offset.clamp(0.0, max_scroll);

    let paragraph = Paragraph::new(text)
        .style(base_style)
        .scroll((state.scroll_offset.round() as u16, 0));

    frame.render_widget(paragraph, content_area);
}

pub fn render_tree(frame: &mut Frame, state: &mut State, area: Rect) {
    let base_style = Style::default().bg(theme::BG_SURFACE);
    let tree_content = generate_directory_tree(state);

    let inner_area = Rect::new(
        area.x + 1,
        area.y,
        area.width.saturating_sub(2),
        area.height
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(theme::BORDER))
        .style(base_style)
        .title(Span::styled(" Directory Tree ", Style::default().fg(theme::ACCENT).bold()));

    let content_area = block.inner(inner_area);
    frame.render_widget(block, inner_area);

    let mut text: Vec<Line> = Vec::new();
    for line in tree_content.lines() {
        if let Some(size_start) = find_size_pattern(line) {
            let (main_part, size_part) = line.split_at(size_start);
            text.push(Line::from(vec![
                Span::styled(format!(" {}", main_part), Style::default().fg(theme::TEXT)),
                Span::styled(size_part.trim_end(), Style::default().fg(theme::ACCENT_DIM)),
            ]));
        } else {
            text.push(Line::from(vec![
                Span::styled(format!(" {}", line), Style::default().fg(theme::TEXT)),
            ]));
        }
    }

    // Calculate and set max scroll
    let content_height = text.len();
    let viewport_height = content_area.height as usize;
    let max_scroll = content_height.saturating_sub(viewport_height) as f32;
    state.max_scroll = max_scroll;
    state.scroll_offset = state.scroll_offset.clamp(0.0, max_scroll);

    let paragraph = Paragraph::new(text)
        .style(base_style)
        .scroll((state.scroll_offset.round() as u16, 0));

    frame.render_widget(paragraph, content_area);
}

pub fn render_glob(frame: &mut Frame, state: &mut State, area: Rect) {
    let base_style = Style::default().bg(theme::BG_SURFACE);
    let selected = state.context.get(state.selected_context);

    let (title, content) = if let Some(ctx) = selected {
        let pattern = ctx.glob_pattern.as_deref().unwrap_or("*");
        let search_path = ctx.glob_path.as_deref().unwrap_or(".");
        let (results, count) = compute_glob_results(pattern, search_path);
        (format!("{} ({} files)", ctx.name, count), results)
    } else {
        ("Glob".to_string(), String::new())
    };

    let inner_area = Rect::new(
        area.x + 1,
        area.y,
        area.width.saturating_sub(2),
        area.height
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(theme::BORDER))
        .style(base_style)
        .title(Span::styled(format!(" {} ", title), Style::default().fg(theme::ACCENT).bold()));

    let content_area = block.inner(inner_area);
    frame.render_widget(block, inner_area);

    let text: Vec<Line> = content.lines()
        .map(|line| Line::from(vec![
            Span::styled(format!("  {} ", chars::DOT), Style::default().fg(theme::ACCENT_DIM)),
            Span::styled(line, Style::default().fg(theme::TEXT)),
        ]))
        .collect();

    // Calculate and set max scroll
    let content_height = text.len();
    let viewport_height = content_area.height as usize;
    let max_scroll = content_height.saturating_sub(viewport_height) as f32;
    state.max_scroll = max_scroll;
    state.scroll_offset = state.scroll_offset.clamp(0.0, max_scroll);

    let paragraph = Paragraph::new(text)
        .style(base_style)
        .scroll((state.scroll_offset.round() as u16, 0));

    frame.render_widget(paragraph, content_area);
}

pub fn render_tmux(frame: &mut Frame, state: &mut State, area: Rect) {
    let base_style = Style::default().bg(theme::BG_SURFACE);
    let selected = state.context.get(state.selected_context);

    let (title, content, description, last_keys) = if let Some(ctx) = selected {
        let pane_id = ctx.tmux_pane_id.as_deref().unwrap_or("?");
        let lines = ctx.tmux_lines.unwrap_or(50);
        let content = capture_pane_content(pane_id, lines);
        let desc = ctx.tmux_description.clone().unwrap_or_default();
        let last = ctx.tmux_last_keys.clone();
        (format!("tmux {}", pane_id), content, desc, last)
    } else {
        ("Tmux".to_string(), String::new(), String::new(), None)
    };

    let inner_area = Rect::new(
        area.x + 1,
        area.y,
        area.width.saturating_sub(2),
        area.height
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(theme::BORDER))
        .style(base_style)
        .title(Span::styled(format!(" {} ", title), Style::default().fg(theme::ACCENT).bold()));

    let content_area = block.inner(inner_area);
    frame.render_widget(block, inner_area);

    let mut text: Vec<Line> = Vec::new();

    // Show description and last keys if present
    if !description.is_empty() {
        text.push(Line::from(vec![
            Span::styled(" ", base_style),
            Span::styled(&description, Style::default().fg(theme::TEXT_MUTED).italic()),
        ]));
    }
    if let Some(ref keys) = last_keys {
        text.push(Line::from(vec![
            Span::styled(" last: ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(keys, Style::default().fg(theme::ACCENT_DIM)),
        ]));
    }
    if !description.is_empty() || last_keys.is_some() {
        text.push(Line::from(vec![
            Span::styled(format!(" {}", chars::HORIZONTAL.repeat(content_area.width.saturating_sub(2) as usize)), Style::default().fg(theme::BORDER)),
        ]));
    }

    // Pane content
    for line in content.lines() {
        text.push(Line::from(vec![
            Span::styled(" ", base_style),
            Span::styled(line, Style::default().fg(theme::TEXT)),
        ]));
    }

    // Calculate and set max scroll
    let content_height = text.len();
    let viewport_height = content_area.height as usize;
    let max_scroll = content_height.saturating_sub(viewport_height) as f32;
    state.max_scroll = max_scroll;
    state.scroll_offset = state.scroll_offset.clamp(0.0, max_scroll);

    let paragraph = Paragraph::new(text)
        .style(base_style)
        .scroll((state.scroll_offset.round() as u16, 0));

    frame.render_widget(paragraph, content_area);
}

pub fn render_todo(frame: &mut Frame, state: &mut State, area: Rect) {
    let base_style = Style::default().bg(theme::BG_SURFACE);

    let inner_area = Rect::new(
        area.x + 1,
        area.y,
        area.width.saturating_sub(2),
        area.height
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(theme::BORDER))
        .style(base_style)
        .title(Span::styled(" Todo ", Style::default().fg(theme::ACCENT).bold()));

    let content_area = block.inner(inner_area);
    frame.render_widget(block, inner_area);

    let mut text: Vec<Line> = Vec::new();

    if state.todos.is_empty() {
        text.push(Line::from(vec![
            Span::styled(" ", base_style),
            Span::styled("No todos", Style::default().fg(theme::TEXT_MUTED).italic()),
        ]));
    } else {
        // Render todos hierarchically (flatten to avoid lifetime issues)
        fn collect_todo_lines(
            todos: &[crate::state::TodoItem],
            parent_id: Option<&String>,
            indent: usize,
            lines: &mut Vec<(usize, String, String, TodoStatus, String)>,
        ) {
            for todo in todos.iter().filter(|t| t.parent_id.as_ref() == parent_id) {
                lines.push((indent, todo.id.clone(), todo.name.clone(), todo.status, todo.description.clone()));
                collect_todo_lines(todos, Some(&todo.id), indent + 1, lines);
            }
        }

        let mut todo_lines: Vec<(usize, String, String, TodoStatus, String)> = Vec::new();
        collect_todo_lines(&state.todos, None, 0, &mut todo_lines);

        for (indent, id, name, status, description) in todo_lines {
            let prefix = "  ".repeat(indent);
            let (status_char, status_color) = match status {
                TodoStatus::Pending => (' ', theme::TEXT_MUTED),
                TodoStatus::InProgress => ('~', theme::WARNING),
                TodoStatus::Done => ('x', theme::SUCCESS),
            };

            let name_style = if status == TodoStatus::Done {
                Style::default().fg(theme::TEXT_MUTED)
            } else {
                Style::default().fg(theme::TEXT)
            };

            text.push(Line::from(vec![
                Span::styled(" ", base_style),
                Span::styled(prefix.clone(), base_style),
                Span::styled("[", Style::default().fg(theme::TEXT_MUTED)),
                Span::styled(format!("{}", status_char), Style::default().fg(status_color)),
                Span::styled("] ", Style::default().fg(theme::TEXT_MUTED)),
                Span::styled(id, Style::default().fg(theme::ACCENT_DIM)),
                Span::styled(" ", base_style),
                Span::styled(name, name_style),
            ]));

            if !description.is_empty() {
                let desc_prefix = "  ".repeat(indent + 1);
                text.push(Line::from(vec![
                    Span::styled(" ", base_style),
                    Span::styled(desc_prefix, base_style),
                    Span::styled(description, Style::default().fg(theme::TEXT_SECONDARY)),
                ]));
            }
        }
    }

    // Calculate and set max scroll
    let content_height = text.len();
    let viewport_height = content_area.height as usize;
    let max_scroll = content_height.saturating_sub(viewport_height) as f32;
    state.max_scroll = max_scroll;
    state.scroll_offset = state.scroll_offset.clamp(0.0, max_scroll);

    let paragraph = Paragraph::new(text)
        .style(base_style)
        .scroll((state.scroll_offset.round() as u16, 0));

    frame.render_widget(paragraph, content_area);
}

pub fn render_overview(frame: &mut Frame, state: &mut State, area: Rect) {
    let base_style = Style::default().bg(theme::BG_SURFACE);

    let inner_area = Rect::new(
        area.x + 1,
        area.y,
        area.width.saturating_sub(2),
        area.height
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(theme::BORDER))
        .style(base_style)
        .title(Span::styled(" Context Overview ", Style::default().fg(theme::ACCENT).bold()));

    let content_area = block.inner(inner_area);
    frame.render_widget(block, inner_area);

    let mut text: Vec<Line> = Vec::new();

    // Token usage header
    let total_tokens: usize = state.context.iter().map(|c| c.token_count).sum();
    let max_tokens = MAX_CONTEXT_TOKENS;
    let usage_pct = (total_tokens as f64 / max_tokens as f64 * 100.0).min(100.0);

    text.push(Line::from(vec![
        Span::styled(" ", base_style),
        Span::styled("TOKEN USAGE", Style::default().fg(theme::TEXT_MUTED).bold()),
    ]));
    text.push(Line::from(""));

    // Format numbers
    let current = format_number(total_tokens);
    let max = format_number(max_tokens);
    let pct = format!("{:.1}%", usage_pct);

    text.push(Line::from(vec![
        Span::styled(" ", base_style),
        Span::styled(&current, Style::default().fg(theme::TEXT).bold()),
        Span::styled(" / ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(&max, Style::default().fg(theme::ACCENT).bold()),
        Span::styled(format!(" ({})", pct), Style::default().fg(theme::TEXT_MUTED)),
    ]));

    // Progress bar
    let bar_width = content_area.width.saturating_sub(4) as usize;
    let filled = ((usage_pct / 100.0) * bar_width as f64) as usize;
    let empty = bar_width.saturating_sub(filled);

    let bar_color = if usage_pct > 80.0 {
        theme::WARNING
    } else {
        theme::ACCENT
    };

    text.push(Line::from(vec![
        Span::styled(" ", base_style),
        Span::styled(chars::BLOCK_FULL.repeat(filled), Style::default().fg(bar_color)),
        Span::styled(chars::BLOCK_LIGHT.repeat(empty), Style::default().fg(theme::BG_ELEVATED)),
    ]));

    text.push(Line::from(""));
    text.push(Line::from(vec![
        Span::styled(format!(" {}", chars::HORIZONTAL.repeat(content_area.width.saturating_sub(4) as usize)), Style::default().fg(theme::BORDER)),
    ]));
    text.push(Line::from(""));

    // Context elements header
    text.push(Line::from(vec![
        Span::styled(" ", base_style),
        Span::styled("CONTEXT ELEMENTS", Style::default().fg(theme::TEXT_MUTED).bold()),
    ]));
    text.push(Line::from(""));

    // Calculate ID width for alignment
    let id_width = state.context.iter().map(|c| c.id.len()).max().unwrap_or(2);

    for ctx in &state.context {
        let icon = ctx.context_type.icon();
        let type_name = match ctx.context_type {
            ContextType::Conversation => "conversation",
            ContextType::File => "file",
            ContextType::Tree => "tree",
            ContextType::Glob => "glob",
            ContextType::Tmux => "tmux",
            ContextType::Todo => "todo",
            ContextType::Memory => "memory",
            ContextType::Overview => "overview",
            ContextType::Tools => "tools",
        };

        let details = match ctx.context_type {
            ContextType::File => ctx.file_path.as_deref().unwrap_or("").to_string(),
            ContextType::Glob => ctx.glob_pattern.as_deref().unwrap_or("").to_string(),
            ContextType::Tmux => {
                let pane = ctx.tmux_pane_id.as_deref().unwrap_or("?");
                let desc = ctx.tmux_description.as_deref().unwrap_or("");
                if desc.is_empty() { pane.to_string() } else { format!("{}: {}", pane, desc) }
            }
            _ => String::new(),
        };

        let tokens = format_number(ctx.token_count);
        let shortcut = format!("{:>width$}", &ctx.id, width = id_width);

        let mut spans = vec![
            Span::styled(" ", base_style),
            Span::styled(format!("{} ", icon), Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(format!("{} ", shortcut), Style::default().fg(theme::ACCENT_DIM)),
            Span::styled(format!("{:<12}", type_name), Style::default().fg(theme::TEXT_SECONDARY)),
            Span::styled(format!("{:>8}", tokens), Style::default().fg(theme::ACCENT)),
        ];

        if !details.is_empty() {
            let max_detail_len = content_area.width.saturating_sub(35) as usize;
            let truncated_details = if details.len() > max_detail_len {
                format!("{}…", &details[..max_detail_len.saturating_sub(1)])
            } else {
                details
            };
            spans.push(Span::styled(format!("  {}", truncated_details), Style::default().fg(theme::TEXT_MUTED)));
        }

        text.push(Line::from(spans));
    }

    text.push(Line::from(""));
    text.push(Line::from(vec![
        Span::styled(format!(" {}", chars::HORIZONTAL.repeat(content_area.width.saturating_sub(4) as usize)), Style::default().fg(theme::BORDER)),
    ]));
    text.push(Line::from(""));

    // Statistics section
    text.push(Line::from(vec![
        Span::styled(" ", base_style),
        Span::styled("STATISTICS", Style::default().fg(theme::TEXT_MUTED).bold()),
    ]));
    text.push(Line::from(""));

    // Message counts
    let user_msgs = state.messages.iter().filter(|m| m.role == "user").count();
    let assistant_msgs = state.messages.iter().filter(|m| m.role == "assistant").count();
    let total_msgs = state.messages.len();

    text.push(Line::from(vec![
        Span::styled(" ", base_style),
        Span::styled("Messages: ", Style::default().fg(theme::TEXT_SECONDARY)),
        Span::styled(format!("{}", total_msgs), Style::default().fg(theme::TEXT).bold()),
        Span::styled(format!(" ({} user, {} assistant)", user_msgs, assistant_msgs), Style::default().fg(theme::TEXT_MUTED)),
    ]));

    // Todo summary
    let total_todos = state.todos.len();
    if total_todos > 0 {
        let done_todos = state.todos.iter().filter(|t| t.status == TodoStatus::Done).count();
        let in_progress = state.todos.iter().filter(|t| t.status == TodoStatus::InProgress).count();
        let pending = total_todos - done_todos - in_progress;

        text.push(Line::from(vec![
            Span::styled(" ", base_style),
            Span::styled("Todos: ", Style::default().fg(theme::TEXT_SECONDARY)),
            Span::styled(format!("{}/{}", done_todos, total_todos), Style::default().fg(theme::SUCCESS).bold()),
            Span::styled(" done", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(format!(", {} in progress, {} pending", in_progress, pending), Style::default().fg(theme::TEXT_MUTED)),
        ]));
    }

    // Memory summary
    let total_memories = state.memories.len();
    if total_memories > 0 {
        let critical = state.memories.iter().filter(|m| m.importance == MemoryImportance::Critical).count();
        let high = state.memories.iter().filter(|m| m.importance == MemoryImportance::High).count();
        let medium = state.memories.iter().filter(|m| m.importance == MemoryImportance::Medium).count();
        let low = state.memories.iter().filter(|m| m.importance == MemoryImportance::Low).count();

        text.push(Line::from(vec![
            Span::styled(" ", base_style),
            Span::styled("Memories: ", Style::default().fg(theme::TEXT_SECONDARY)),
            Span::styled(format!("{}", total_memories), Style::default().fg(theme::TEXT).bold()),
            Span::styled(format!(" ({} critical, {} high, {} medium, {} low)", critical, high, medium, low), Style::default().fg(theme::TEXT_MUTED)),
        ]));
    }

    // Calculate and set max scroll
    let content_height = text.len();
    let viewport_height = content_area.height as usize;
    let max_scroll = content_height.saturating_sub(viewport_height) as f32;
    state.max_scroll = max_scroll;
    state.scroll_offset = state.scroll_offset.clamp(0.0, max_scroll);

    let paragraph = Paragraph::new(text)
        .style(base_style)
        .scroll((state.scroll_offset.round() as u16, 0));

    frame.render_widget(paragraph, content_area);
}

pub fn render_memory(frame: &mut Frame, state: &mut State, area: Rect) {
    let base_style = Style::default().bg(theme::BG_SURFACE);

    let inner_area = Rect::new(
        area.x + 1,
        area.y,
        area.width.saturating_sub(2),
        area.height
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(theme::BORDER))
        .style(base_style)
        .title(Span::styled(" Memory ", Style::default().fg(theme::ACCENT).bold()));

    let content_area = block.inner(inner_area);
    frame.render_widget(block, inner_area);

    let mut text: Vec<Line> = Vec::new();

    if state.memories.is_empty() {
        text.push(Line::from(vec![
            Span::styled(" ", base_style),
            Span::styled("No memories", Style::default().fg(theme::TEXT_MUTED).italic()),
        ]));
    } else {
        // Sort by importance (critical first)
        let mut sorted_memories: Vec<_> = state.memories.iter().collect();
        sorted_memories.sort_by(|a, b| {
            let importance_order = |i: &MemoryImportance| match i {
                MemoryImportance::Critical => 0,
                MemoryImportance::High => 1,
                MemoryImportance::Medium => 2,
                MemoryImportance::Low => 3,
            };
            importance_order(&a.importance).cmp(&importance_order(&b.importance))
        });

        for memory in sorted_memories {
            let importance_color = match memory.importance {
                MemoryImportance::Critical => theme::WARNING,
                MemoryImportance::High => theme::ACCENT,
                MemoryImportance::Medium => theme::TEXT_SECONDARY,
                MemoryImportance::Low => theme::TEXT_MUTED,
            };

            let importance_badge = match memory.importance {
                MemoryImportance::Critical => "!!!",
                MemoryImportance::High => "!! ",
                MemoryImportance::Medium => "!  ",
                MemoryImportance::Low => "   ",
            };

            text.push(Line::from(vec![
                Span::styled(" ", base_style),
                Span::styled(importance_badge, Style::default().fg(importance_color).bold()),
                Span::styled(&memory.id, Style::default().fg(theme::ACCENT_DIM)),
                Span::styled(" ", base_style),
                Span::styled(&memory.content, Style::default().fg(theme::TEXT)),
            ]));
        }
    }

    // Calculate and set max scroll
    let content_height = text.len();
    let viewport_height = content_area.height as usize;
    let max_scroll = content_height.saturating_sub(viewport_height) as f32;
    state.max_scroll = max_scroll;
    state.scroll_offset = state.scroll_offset.clamp(0.0, max_scroll);

    let paragraph = Paragraph::new(text)
        .style(base_style)
        .scroll((state.scroll_offset.round() as u16, 0));

    frame.render_widget(paragraph, content_area);
}

pub fn render_tools(frame: &mut Frame, state: &mut State, area: Rect) {
    let base_style = Style::default().bg(theme::BG_SURFACE);

    let inner_area = Rect::new(
        area.x + 1,
        area.y,
        area.width.saturating_sub(2),
        area.height
    );

    // Count enabled tools
    let enabled_count = state.tools.iter().filter(|t| t.enabled).count();
    let total_count = state.tools.len();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(theme::BORDER))
        .style(base_style)
        .title(Span::styled(
            format!(" Tools ({}/{}) ", enabled_count, total_count),
            Style::default().fg(theme::ACCENT).bold()
        ));

    let content_area = block.inner(inner_area);
    frame.render_widget(block, inner_area);

    let mut text: Vec<Line> = Vec::new();

    // Helper to render a parameter with proper formatting
    fn render_param<'a>(param: &'a ToolParam, indent: usize, base_style: Style) -> Vec<Line<'a>> {
        let mut lines = Vec::new();
        let prefix = "  ".repeat(indent);

        // Parameter name with type
        let req_marker = if param.required { "*" } else { "" };
        let type_str = param.param_type.as_str();

        let mut spans = vec![
            Span::styled(format!("{}", prefix), base_style),
            Span::styled(format!("{}", param.name), Style::default().fg(theme::ACCENT_DIM)),
            Span::styled(format!("{}", req_marker), Style::default().fg(theme::WARNING)),
            Span::styled(": ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(type_str, Style::default().fg(theme::TEXT_SECONDARY)),
        ];

        // Add enum values if present
        if let Some(enum_vals) = &param.enum_values {
            spans.push(Span::styled(" [", Style::default().fg(theme::TEXT_MUTED)));
            spans.push(Span::styled(enum_vals.join("|"), Style::default().fg(theme::TEXT_SECONDARY).italic()));
            spans.push(Span::styled("]", Style::default().fg(theme::TEXT_MUTED)));
        }

        // Add default value if present
        if let Some(default) = &param.default {
            spans.push(Span::styled(" = ", Style::default().fg(theme::TEXT_MUTED)));
            spans.push(Span::styled(default.clone(), Style::default().fg(theme::TEXT_SECONDARY)));
        }

        lines.push(Line::from(spans));

        // Description on next line
        if let Some(desc) = &param.description {
            lines.push(Line::from(vec![
                Span::styled(format!("{}  ", prefix), base_style),
                Span::styled(desc.clone(), Style::default().fg(theme::TEXT_MUTED).italic()),
            ]));
        }

        // Handle nested object params in arrays
        if let ParamType::Array(inner) = &param.param_type {
            if let ParamType::Object(nested) = inner.as_ref() {
                lines.push(Line::from(vec![
                    Span::styled(format!("{}  ", prefix), base_style),
                    Span::styled("Item properties:", Style::default().fg(theme::TEXT_MUTED)),
                ]));
                for nested_param in nested {
                    lines.extend(render_param(nested_param, indent + 2, base_style));
                }
            }
        }

        lines
    }

    // Group tools by category
    let categories = [
        ToolCategory::FileSystem,
        ToolCategory::Context,
        ToolCategory::Tmux,
        ToolCategory::Tasks,
        ToolCategory::Memory,
    ];

    for category in categories {
        let cat_tools: Vec<_> = state.tools.iter()
            .filter(|t| t.category == category)
            .collect();

        if cat_tools.is_empty() {
            continue;
        }

        // Category header
        text.push(Line::from(vec![
            Span::styled(" ", base_style),
            Span::styled(category.icon(), Style::default().fg(theme::ACCENT)),
            Span::styled(format!(" {}", category.as_str().to_uppercase()), Style::default().fg(theme::TEXT_MUTED).bold()),
        ]));
        text.push(Line::from(""));

        for tool in cat_tools {
            let (status_char, status_color) = if tool.enabled {
                ('●', theme::SUCCESS)
            } else {
                ('○', theme::TEXT_MUTED)
            };

            let name_style = if tool.enabled {
                Style::default().fg(theme::TEXT)
            } else {
                Style::default().fg(theme::TEXT_MUTED)
            };

            // Tool name with ID
            text.push(Line::from(vec![
                Span::styled("  ", base_style),
                Span::styled(format!("{} ", status_char), Style::default().fg(status_color)),
                Span::styled(&tool.name, name_style.bold()),
                Span::styled(format!(" ({})", tool.id), Style::default().fg(theme::TEXT_MUTED)),
            ]));

            // Description
            text.push(Line::from(vec![
                Span::styled("    ", base_style),
                Span::styled(&tool.description, Style::default().fg(theme::TEXT_SECONDARY)),
            ]));

            // Parameters header
            if !tool.params.is_empty() {
                text.push(Line::from(vec![
                    Span::styled("    ", base_style),
                    Span::styled("Parameters ", Style::default().fg(theme::TEXT_MUTED)),
                    Span::styled("(* = required)", Style::default().fg(theme::WARNING).italic()),
                    Span::styled(":", Style::default().fg(theme::TEXT_MUTED)),
                ]));

                // Render each parameter
                for param in &tool.params {
                    let param_lines = render_param(param, 3, base_style);
                    text.extend(param_lines);
                }
            }

            text.push(Line::from(""));
        }
    }

    // Calculate and set max scroll
    let content_height = text.len();
    let viewport_height = content_area.height as usize;
    let max_scroll = content_height.saturating_sub(viewport_height) as f32;
    state.max_scroll = max_scroll;
    state.scroll_offset = state.scroll_offset.clamp(0.0, max_scroll);

    let paragraph = Paragraph::new(text)
        .style(base_style)
        .scroll((state.scroll_offset.round() as u16, 0));

    frame.render_widget(paragraph, content_area);
}
