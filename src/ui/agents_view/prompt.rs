//! Name-entry prompts in the agents view message pane: "+ New Agent" and
//! the Ctrl+R rename. Border + title are layout chrome; content goes through
//! the IR pipeline.

use ratatui::Frame;
use ratatui::prelude::{Rect, Style};
use ratatui::widgets::{Block as RBlock, Borders, Paragraph};

use cp_render::{Block as IrBlock, Semantic, Span as S};

use crate::ui::{ir, theme};

/// Copy for a name-entry prompt: border title + two instruction lines.
pub(super) struct TitlePrompt {
    /// Border title.
    header: &'static str,
    /// Instruction lines above the draft.
    lines: [&'static str; 2],
}

/// Prompt shown on the virtual "+ New Agent" row.
pub(super) const NEW_AGENT_PROMPT: TitlePrompt = TitlePrompt {
    header: " New Agent ",
    lines: ["Type a name for the new agent below,", "then press Enter to create it."],
};

/// Prompt shown while a Ctrl+R rename is pending.
pub(super) const RENAME_PROMPT: TitlePrompt =
    TitlePrompt { header: " Rename Agent ", lines: ["Edit the agent's name below,", "Enter renames it, Esc cancels."] };

/// Render a name-entry prompt with the live `draft` in the message area.
/// Border + title are layout chrome; content goes through IR pipeline.
pub(super) fn render_title_prompt(frame: &mut Frame<'_>, prompt: &TitlePrompt, draft: &str, area: Rect) {
    let border = RBlock::default()
        .borders(Borders::ALL)
        .border_style(ir::semantic_to_style(Semantic::Border))
        .title(ratatui::text::Span::styled(prompt.header, ir::semantic_to_style(Semantic::Accent)))
        .style(Style::default().bg(theme::bg_base()));

    let inner = border.inner(area);
    frame.render_widget(border, area);

    let input_preview = if draft.is_empty() { "\u{2026}".to_owned() } else { draft.to_owned() };
    let [first, second] = prompt.lines;

    let ir_blocks = vec![
        IrBlock::Empty,
        IrBlock::Line(vec![S::muted(first.to_owned())]),
        IrBlock::Line(vec![S::muted(second.to_owned())]),
        IrBlock::Empty,
        IrBlock::Line(vec![S::accent("  \u{279c} ".to_owned()), S::new(input_preview)]),
    ];

    let lines = ir::blocks_to_lines(&ir_blocks);
    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}
