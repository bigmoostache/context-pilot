//! Behaviour flags.
//!
//! [`Feature`] is the one place a flag is named: its environment variable,
//! its JSON key in `GET /api/features`, its default and its description all
//! hang off the enum, and the spec table is generated from it.

use crate::resolve::Raw;

/// One behaviour switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Feature {
    /// Offer the Claude Code subscription (OAuth) as a provider.
    ClaudeOauth,
    /// Run the day-0 identity/TLS setup for the first IT-capable login.
    Day0Setup,
    /// Expose the IT pane (identity, TLS trust, network) and its routes.
    ItPane,
    /// Expose the OTA updater (pane, routes, scheduler).
    Updater,
    /// Let superadmins edit provider keys from the cockpit.
    KeysEditable,
    /// Run the first-run product onboarding tour.
    Onboarding,
}

impl Feature {
    /// Every flag, in declaration order.
    pub const ALL: [Self; 6] =
        [Self::ClaudeOauth, Self::Day0Setup, Self::ItPane, Self::Updater, Self::KeysEditable, Self::Onboarding];

    /// The environment variable.
    #[must_use]
    pub const fn env_name(self) -> &'static str {
        match self {
            Self::ClaudeOauth => "CP_FEATURE_CLAUDE_OAUTH",
            Self::Day0Setup => "CP_FEATURE_DAY0_SETUP",
            Self::ItPane => "CP_FEATURE_IT_PANE",
            Self::Updater => "CP_FEATURE_UPDATER",
            Self::KeysEditable => "CP_FEATURE_KEYS_EDITABLE",
            Self::Onboarding => "CP_FEATURE_ONBOARDING",
        }
    }

    /// The JSON key in `GET /api/features`.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::ClaudeOauth => "claude_oauth",
            Self::Day0Setup => "day0_setup",
            Self::ItPane => "it_pane",
            Self::Updater => "updater",
            Self::KeysEditable => "keys_editable",
            Self::Onboarding => "onboarding",
        }
    }

    /// The value when the variable is unset.
    #[must_use]
    pub const fn default_on(self) -> bool {
        match self {
            Self::ClaudeOauth | Self::KeysEditable | Self::Onboarding => true,
            Self::Day0Setup | Self::ItPane | Self::Updater => false,
        }
    }

    /// The default as the spec table spells it.
    #[must_use]
    pub const fn default_literal(self) -> &'static str {
        if self.default_on() { "1" } else { "0" }
    }

    /// One-line description for the docs.
    #[must_use]
    pub const fn doc(self) -> &'static str {
        match self {
            Self::ClaudeOauth => {
                "Offer the Claude Code subscription (OAuth) as a provider. Off: the login routes answer 404, the provider is dropped from the catalogue, the token sweeper is not started and the cockpit hides the subscription UI."
            }
            Self::Day0Setup => {
                "Run the day-0 identity/TLS setup for the first IT-capable login. Off (cloud, containers): the box counts as provisioned and no login ever lands on that step. Requires `CP_FEATURE_IT_PANE=1`."
            }
            Self::ItPane => {
                "Expose the IT pane (identity, TLS trust, network) and every `/api/it/*` route. Off: the routes answer 404 and the pane is hidden. Requires `CP_CADDYFILE`."
            }
            Self::Updater => {
                "Expose the OTA updater: the Update pane, `/api/releases/*` and `/api/update/*`, and the nightly scheduler. Requires `CP_WEB_ROOT`."
            }
            Self::KeysEditable => {
                "Let superadmins reveal and edit provider keys from the cockpit (written to `~/.context-pilot/.env`). Off: keys come from the environment only and the pane is read-only."
            }
            Self::Onboarding => "Run the first-run product onboarding tour for the first manager-level login.",
        }
    }
}

/// The resolved state of every flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Features {
    /// One slot per [`Feature`], in [`Feature::ALL`] order.
    on: [bool; 6],
}

impl Features {
    /// Build from a predicate over every flag.
    fn build<F>(pick: F) -> Self
    where
        F: Fn(Feature) -> bool,
    {
        Self {
            on: [
                pick(Feature::ClaudeOauth),
                pick(Feature::Day0Setup),
                pick(Feature::ItPane),
                pick(Feature::Updater),
                pick(Feature::KeysEditable),
                pick(Feature::Onboarding),
            ],
        }
    }

    /// From the validated values (every flag is literally defaulted, so the
    /// fallback to the enum default is a formality).
    pub(crate) fn from_raw(raw: &Raw) -> Self {
        Self::build(|feature| raw.flag(feature.env_name()).unwrap_or_else(|| feature.default_on()))
    }

    /// Whether `feature` is enabled.
    #[must_use]
    pub const fn is_on(self, feature: Feature) -> bool {
        match feature {
            Feature::ClaudeOauth => self.on[0],
            Feature::Day0Setup => self.on[1],
            Feature::ItPane => self.on[2],
            Feature::Updater => self.on[3],
            Feature::KeysEditable => self.on[4],
            Feature::Onboarding => self.on[5],
        }
    }

    /// A copy with `feature` set to `on` (tests, fixtures).
    #[must_use]
    pub fn with(self, feature: Feature, on: bool) -> Self {
        Self::build(|each| if each == feature { on } else { self.is_on(each) })
    }

    /// Every flag with its state, in [`Feature::ALL`] order.
    #[must_use]
    pub fn entries(self) -> [(Feature, bool); 6] {
        Feature::ALL.map(|feature| (feature, self.is_on(feature)))
    }
}

impl Default for Features {
    fn default() -> Self {
        Self::build(Feature::default_on)
    }
}
