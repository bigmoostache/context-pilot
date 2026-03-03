use serde::{Deserialize, Serialize};

/// Controls sidebar display: full, collapsed (icons only), or hidden.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SidebarMode {
    #[default]
    Normal,
    Collapsed,
    Hidden,
}

impl SidebarMode {
    /// Cycle to the next mode: Normal → Collapsed → Hidden → Normal
    pub fn next(self) -> Self {
        match self {
            Self::Normal => Self::Collapsed,
            Self::Collapsed => Self::Hidden,
            Self::Hidden => Self::Normal,
        }
    }

    /// Width in columns for this sidebar mode
    pub fn width(self) -> u16 {
        match self {
            Self::Normal => 36,
            Self::Collapsed => 14,
            Self::Hidden => 0,
        }
    }
}
