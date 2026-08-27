use serde::{Deserialize, Serialize};
use std::str::FromStr;

use cp_base::state::runtime::State;

/// Memory importance level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MemoryImportance {
    /// Low priority — nice-to-have context.
    Low,
    #[default]
    /// Default importance for general knowledge.
    Medium,
    /// High priority — impacts workflow or architecture.
    High,
    /// Must-read — critical decisions or constraints.
    Critical,
}

impl FromStr for MemoryImportance {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            "critical" => Ok(Self::Critical),
            _ => Err(()),
        }
    }
}

impl MemoryImportance {
    /// String representation for serialization/display.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }

    /// Sort rank — critical first (lower = earlier). Ordering within a tier.
    #[must_use]
    pub const fn rank(self) -> u8 {
        match self {
            Self::Critical => 0,
            Self::High => 1,
            Self::Medium => 2,
            Self::Low => 3,
        }
    }
}

/// A memory tier — a fixed namespace of individually-bounded slots.
///
/// The budget is **set in stone**: five tiers, 220 slots total. Each tier caps
/// a slot's `contents`. `Safe` is bounded in **characters** (it holds literal
/// values — keys, tokens — where a char cap is honest); the rest are bounded in
/// **tokens** via `estimate_tokens`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// Secrets / key-value repository — 50 slots, ≤ 200 chars.
    Safe,
    /// One-sentence facts — 100 slots, ≤ 60 tokens.
    Tiny,
    /// The preferred tier — 40 slots, ≤ 120 tokens.
    Short,
    /// Lengthier information — 20 slots, ≤ 200 tokens.
    Mid,
    /// Incompressible material, last resort — 10 slots, ≤ 400 tokens.
    Long,
}

impl FromStr for Tier {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "safe" => Ok(Self::Safe),
            "tiny" => Ok(Self::Tiny),
            "short" => Ok(Self::Short),
            "mid" => Ok(Self::Mid),
            "long" => Ok(Self::Long),
            _ => Err(()),
        }
    }
}

impl Tier {
    /// All tiers in canonical display order (safe → long).
    pub const ALL: [Self; 5] = [Self::Safe, Self::Tiny, Self::Short, Self::Mid, Self::Long];

    /// Tiers in **migration fill order** — largest slot first, so the most
    /// important old memories land in the roomiest tier and suffer the least
    /// truncation (long → mid → short → tiny → safe).
    pub const FILL_ORDER: [Self; 5] = [Self::Long, Self::Mid, Self::Short, Self::Tiny, Self::Safe];

    /// Parse a tier from its slot-id slug (`safe`/`tiny`/`short`/`mid`/`long`).
    #[must_use]
    pub fn from_str_slug(slug: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.slug() == slug)
    }

    /// The slug used in slot ids (`M-{slug}-{n}`).
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Safe => "safe",
            Self::Tiny => "tiny",
            Self::Short => "short",
            Self::Mid => "mid",
            Self::Long => "long",
        }
    }

    /// Number of slots in this tier.
    #[must_use]
    pub const fn slot_count(self) -> usize {
        match self {
            Self::Safe => 50,
            Self::Tiny => 100,
            Self::Short => 40,
            Self::Mid => 20,
            Self::Long => 10,
        }
    }

    /// True when the tier is bounded in **characters** (only `Safe`); otherwise
    /// bounded in **tokens**.
    #[must_use]
    pub const fn is_char_bound(self) -> bool {
        matches!(self, Self::Safe)
    }

    /// The **enforced** hard ceiling (chars for `Safe`, tokens otherwise). A
    /// write above this is rejected.
    #[must_use]
    pub const fn enforced_bound(self) -> usize {
        match self {
            Self::Tiny => 60,
            Self::Short => 120,
            Self::Long => 400,
            // Safe caps chars, Mid caps tokens; both land at 200.
            Self::Safe | Self::Mid => 200,
        }
    }

    /// The **advertised** ceiling quoted to the model — deliberately under the
    /// enforced cap so a marginal overrun still lands (same trick the old
    /// `tl_dr` used: advertise 80, enforce 120). Never surface `enforced_bound`.
    #[must_use]
    pub const fn advertised_bound(self) -> usize {
        match self {
            Self::Safe => 180,
            Self::Tiny => 50,
            Self::Short => 100,
            Self::Mid => 170,
            Self::Long => 360,
        }
    }

    /// Human unit for messages ("chars" or "tokens").
    #[must_use]
    pub const fn unit(self) -> &'static str {
        if self.is_char_bound() { "chars" } else { "tokens" }
    }
}

/// The always-visible title cap (chars). A real label, not a sentence.
pub const TITLE_MAX_CHARS: usize = 25;

/// Total slots across all tiers (50 + 100 + 40 + 20 + 10).
pub const TOTAL_SLOTS: usize = 220;

/// A single memory slot — occupied or empty. Its `id`/`tier` are fixed at
/// construction; only `title`/`contents`/`importance`/`occupied` mutate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySlot {
    /// Slot id, `M-{tier}-{n}` (1-based), e.g. `M-short-23`. Stable, positional.
    pub id: String,
    /// Owning tier (fixes the `contents` bound).
    pub tier: Tier,
    /// Always-visible label, ≤ [`TITLE_MAX_CHARS`].
    pub title: String,
    /// The tier-bounded value.
    pub contents: String,
    /// Importance — display ordering within the tier only.
    pub importance: MemoryImportance,
    /// `false` → renders as `**empty**`; `title`/`contents` are blank.
    pub occupied: bool,
}

impl MemorySlot {
    /// Build an empty slot for `tier` at 1-based index `n`.
    #[must_use]
    pub fn empty(tier: Tier, n: usize) -> Self {
        Self {
            id: format!("M-{}-{}", tier.slug(), n),
            tier,
            title: String::new(),
            contents: String::new(),
            importance: MemoryImportance::Medium,
            occupied: false,
        }
    }
}

/// Module-owned state for the Memory module — a **fixed** set of [`TOTAL_SLOTS`]
/// slots (occupied or empty). Ids are positional, not allocated, so there is no
/// id counter.
#[derive(Debug)]
pub struct MemoryState {
    /// All 220 slots, grouped by tier in [`Tier::ALL`] order.
    pub slots: Vec<MemorySlot>,
}

impl Default for MemoryState {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryState {
    /// Create the fixed budget: every tier's slots, all empty.
    #[must_use]
    pub fn new() -> Self {
        let mut slots = Vec::with_capacity(TOTAL_SLOTS);
        for tier in Tier::ALL {
            for n in 1..=tier.slot_count() {
                slots.push(MemorySlot::empty(tier, n));
            }
        }
        Self { slots }
    }

    /// Locate a slot by id.
    #[must_use]
    pub fn slot(&self, id: &str) -> Option<&MemorySlot> {
        self.slots.iter().find(|s| s.id == id)
    }

    /// Locate a slot by id, mutably.
    pub fn slot_mut(&mut self, id: &str) -> Option<&mut MemorySlot> {
        self.slots.iter_mut().find(|s| s.id == id)
    }

    /// Count of occupied slots across all tiers.
    #[must_use]
    pub fn occupied_count(&self) -> usize {
        self.slots.iter().filter(|s| s.occupied).count()
    }

    /// Get shared ref from State's `TypeMap`.
    ///
    /// # Panics
    ///
    /// Panics if an internal invariant is violated.
    #[must_use]
    pub fn get(state: &State) -> &Self {
        state.ext::<Self>()
    }

    /// Get mutable ref from State's `TypeMap`.
    ///
    /// # Panics
    ///
    /// Panics if an internal invariant is violated.
    pub fn get_mut(state: &mut State) -> &mut Self {
        state.ext_mut::<Self>()
    }
}
