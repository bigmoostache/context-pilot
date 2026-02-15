use crate::state::State;

/// A command that can be executed from the palette
#[derive(Debug, Clone)]
pub struct PaletteCommand {
    /// Unique command ID
    pub id: String,
    /// Display label shown in the palette
    pub label: String,
    /// Short description/hint
    pub description: String,
    /// Keywords for fuzzy matching (including label)
    pub keywords: Vec<String>,
}

impl PaletteCommand {
    pub fn new(id: impl Into<String>, label: impl Into<String>, description: impl Into<String>) -> Self {
        let label = label.into();
        let keywords = vec![label.to_lowercase()];
        Self { id: id.into(), label, description: description.into(), keywords }
    }

    pub fn with_keywords(mut self, keywords: Vec<&str>) -> Self {
        self.keywords.extend(keywords.iter().map(|s| s.to_lowercase()));
        self
    }

    /// Check if this command matches the query (fuzzy match)
    pub fn matches(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        let query_lower = query.to_lowercase();

        // Check if any keyword contains the query
        self.keywords.iter().any(|k| k.contains(&query_lower))
            || self.label.to_lowercase().contains(&query_lower)
            || self.id.to_lowercase().contains(&query_lower)
            || self.description.to_lowercase().contains(&query_lower)
    }

    /// Score how well this command matches (higher = better match)
    pub fn match_score(&self, query: &str) -> i32 {
        if query.is_empty() {
            return 0;
        }
        let query_lower = query.to_lowercase();
        let mut score = 0;

        // Exact ID match (highest priority)
        if self.id.to_lowercase() == query_lower {
            score += 1000;
        } else if self.id.to_lowercase().starts_with(&query_lower) {
            score += 500;
        }

        // Label match
        if self.label.to_lowercase().starts_with(&query_lower) {
            score += 100;
        } else if self.label.to_lowercase().contains(&query_lower) {
            score += 50;
        }

        // Keyword match
        for keyword in &self.keywords {
            if keyword.starts_with(&query_lower) {
                score += 30;
            } else if keyword.contains(&query_lower) {
                score += 10;
            }
        }

        score
    }
}

/// Build the list of available commands based on current state
pub fn get_available_commands(state: &State) -> Vec<PaletteCommand> {
    let mut commands = Vec::new();

    // System commands at the top
    commands.push(
        PaletteCommand::new("quit", "Quit", "Exit the application (Ctrl+Q)").with_keywords(vec!["exit", "close", "q"]),
    );

    commands.push(PaletteCommand::new("reload", "Reload", "Reload the TUI").with_keywords(vec!["restart", "refresh"]));

    commands.push(PaletteCommand::new("config", "Config", "Open configuration panel (Ctrl+H)").with_keywords(vec![
        "settings",
        "options",
        "preferences",
        "provider",
        "model",
    ]));

    // Conversation entry (special: no Px ID, always first in panels)
    if let Some(conv) = state.context.iter().find(|c| c.context_type == crate::state::ContextType::Conversation) {
        let icon = conv.context_type.icon();
        commands.push(
            PaletteCommand::new(&conv.id, format!("{} Conversation", icon), "Go to conversation").with_keywords(vec![
                "conversation",
                "chat",
                "messages",
                "panel",
                "go",
                "navigate",
            ]),
        );
    }

    // Panel navigation commands (P1, P2, ...)
    // Sort by P-number for consistent ordering
    let mut sorted_contexts: Vec<_> =
        state.context.iter().filter(|c| c.context_type != crate::state::ContextType::Conversation).collect();
    sorted_contexts.sort_by(|a, b| {
        let id_a = a.id.strip_prefix('P').and_then(|n| n.parse::<usize>().ok()).unwrap_or(usize::MAX);
        let id_b = b.id.strip_prefix('P').and_then(|n| n.parse::<usize>().ok()).unwrap_or(usize::MAX);
        id_a.cmp(&id_b)
    });

    for ctx in sorted_contexts {
        let icon = ctx.context_type.icon();
        commands.push(
            PaletteCommand::new(
                &ctx.id,
                format!("{} {} {}", &ctx.id, icon, &ctx.name),
                format!("Go to {} panel", &ctx.name),
            )
            .with_keywords(vec![&ctx.name, "panel", "go", "navigate"]),
        );
    }

    commands
}
