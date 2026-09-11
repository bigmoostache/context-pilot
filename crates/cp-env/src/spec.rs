//! Declarative description of one environment variable.
//!
//! A [`Spec`] is the single source of truth a variable has: the validator, the
//! typed model and the generated `docs/ENV.md` are all derived from the same
//! entry, so none of them can drift from the others.

/// Which binary parses the variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Read by `cp-orchestrator` only.
    Orchestrator,
    /// Read by the `cpilot` agent only.
    Agent,
    /// Read by both binaries.
    Both,
    /// Never read by either binary: injected into child processes (callback
    /// scripts, test harnesses) or consumed by tooling. Registered so strict
    /// mode tolerates it and the docs list it; never parsed.
    ChildOnly,
}

/// The binary asking for its configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// The orchestration backend.
    Orchestrator,
    /// The agent TUI.
    Agent,
    /// Every non-child variable - the lenient in-process fallback used when no
    /// binary installed a validated environment (tests).
    Any,
}

impl Target {
    /// Human label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Orchestrator => "orchestrator",
            Self::Agent => "agent",
            Self::Any => "any",
        }
    }
}

impl Scope {
    /// Whether a variable of this scope is parsed when `target` loads its
    /// configuration. Variables of the *other* binary's scope are known (so
    /// strict mode never flags them as unknown) but left unparsed.
    #[must_use]
    pub const fn parsed_for(self, target: Target) -> bool {
        match self {
            Self::ChildOnly => false,
            Self::Both => true,
            Self::Orchestrator => matches!(target, Target::Orchestrator | Target::Any),
            Self::Agent => matches!(target, Target::Agent | Target::Any),
        }
    }

    /// Human label used in the docs table.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Orchestrator => "orchestrator",
            Self::Agent => "agent",
            Self::Both => "both",
            Self::ChildOnly => "child only",
        }
    }
}

/// Filesystem precondition checked on an **explicitly set** path. Literal
/// defaults are never checked (a missing default only logs a warning), so a
/// developer without the production layout can still boot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exist {
    /// No check - the path is created on first use.
    No,
    /// Must be an existing regular file.
    File,
    /// Must be an existing directory.
    Dir,
    /// The parent directory must exist (the file itself is created later).
    Parent,
    /// Must be an existing file with an execute bit set.
    Executable,
}

/// Value type. Validation, normalisation and the docs "type" column follow it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `0`, `1`, `true` or `false` (case-insensitive); stored as `1`/`0`.
    Bool,
    /// Integer in `1..=65535` (a port).
    U16,
    /// Unsigned integer, at least [`Spec::min`].
    U64,
    /// Free text; whitespace-trimmed, must be non-empty once set.
    Str,
    /// Filesystem path with the given precondition.
    Path(Exist),
    /// `http://` or `https://` URL; trailing slashes are trimmed.
    Url,
    /// A credential. Declared here for the docs and the unknown-name check
    /// only - never read by this crate; the vault resolves it.
    Secret,
}

impl Kind {
    /// Human label used in the docs table.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Bool => "bool (0/1/true/false)",
            Self::U16 => "port (1-65535)",
            Self::U64 => "integer",
            Self::Str => "text",
            Self::Path(exist) => match exist {
                Exist::No => "path",
                Exist::File => "path (existing file)",
                Exist::Dir => "path (existing directory)",
                Exist::Parent => "path (parent directory must exist)",
                Exist::Executable => "path (executable)",
            },
            Self::Url => "http(s) URL",
            Self::Secret => "secret (vault)",
        }
    }
}

/// What the variable resolves to when unset (or set to the empty string).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fallback {
    /// Nothing - the value is optional, or required (see [`Spec::required`]).
    None,
    /// A literal default, parsed exactly like an explicit value.
    Literal(&'static str),
    /// Computed by the typed model from other values (documented as shown).
    Derived(&'static str),
}

impl Fallback {
    /// Docs rendering of the default.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "-",
            Self::Literal(text) | Self::Derived(text) => text,
        }
    }
}

/// Documentation group; also the section order of `docs/ENV.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    /// Shared by both binaries.
    Core,
    /// Orchestrator topology.
    Orchestrator,
    /// Authentication and sessions.
    Auth,
    /// First-boot account seeding.
    Seed,
    /// Optional LLM gateway.
    Gateway,
    /// Agent <-> orchestrator bridge.
    Bridge,
    /// Appliance-only gates: Caddy, network applier, uplink supervisor.
    Appliance,
    /// Behaviour flags.
    Features,
    /// Developer conveniences.
    Dev,
    /// Credentials resolved by the vault.
    Secrets,
    /// Registered names neither binary parses.
    External,
}

impl Group {
    /// Every group, in docs order.
    pub const ALL: [Self; 11] = [
        Self::Core,
        Self::Orchestrator,
        Self::Auth,
        Self::Seed,
        Self::Gateway,
        Self::Bridge,
        Self::Appliance,
        Self::Features,
        Self::Dev,
        Self::Secrets,
        Self::External,
    ];

    /// Section title.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Core => "Core",
            Self::Orchestrator => "Orchestrator",
            Self::Auth => "Authentication",
            Self::Seed => "Account seeding",
            Self::Gateway => "LLM gateway",
            Self::Bridge => "Agent bridge",
            Self::Appliance => "Appliance gates",
            Self::Features => "Feature flags",
            Self::Dev => "Developer",
            Self::Secrets => "Credentials",
            Self::External => "External names",
        }
    }

    /// One-paragraph section introduction.
    #[must_use]
    pub const fn blurb(self) -> &'static str {
        match self {
            Self::Core => "Read by both binaries.",
            Self::Orchestrator => "Where the orchestrator listens and where it finds its agents and front.",
            Self::Auth => "Login enforcement and session lifetime.",
            Self::Seed => {
                "Accounts created once, at the first boot with an empty user table. Every seeded account must change its password on first login. Requires `CP_AUTH_ENABLED=1`; an email without a password is an error."
            }
            Self::Gateway => {
                "Optional LiteLLM-style proxy. When set, Anthropic, Grok, Groq and DeepSeek traffic goes through it and needs no local provider key. Claude Code OAuth and MiniMax always talk to their own API."
            }
            Self::Bridge => {
                "How a spawned agent finds its orchestrator. The orchestrator sets these on every agent it starts."
            }
            Self::Appliance => {
                "Gates that turn the orchestrator from \"persist the document\" into \"reconfigure this machine\". Leave every one of them UNSET outside the appliance (containers, developer machines): absent, the Caddy integration and the network applier are inert."
            }
            Self::Features => {
                "Behaviour switches, enforced server-side and mirrored by the cockpit through `GET /api/features`. Set every flag explicitly in each deployment profile."
            }
            Self::Dev => "Local development only.",
            Self::Secrets => {
                "Resolved by the vault (process environment, then `~/.context-pilot/.env`, then keychain/credential file). Never read by the configuration layer; listed here so the table is complete."
            }
            Self::External => {
                "Names that use the `CP_` prefix but are consumed by child processes or tooling, never by the binaries. Registered so strict validation tolerates them."
            }
        }
    }
}

/// One environment variable.
#[derive(Debug, Clone, Copy)]
pub struct Spec {
    /// Exact variable name.
    pub name: &'static str,
    /// Value type.
    pub kind: Kind,
    /// Default when unset.
    pub fallback: Fallback,
    /// Which binary parses it.
    pub scope: Scope,
    /// Refuse to start when unset and no literal default applies.
    pub required: bool,
    /// Never echo the value (reports, `Debug`).
    pub sensitive: bool,
    /// Lower bound for [`Kind::U64`] (ignored by other kinds).
    pub min: u64,
    /// Docs section.
    pub group: Group,
    /// One-line description.
    pub doc: &'static str,
}

impl Spec {
    /// The common shape every table entry starts from: optional, not
    /// sensitive, no lower bound. Tables spell out the six meaningful fields
    /// and take the rest from here (`..Spec::BASE`).
    pub const BASE: Self = Self {
        name: "",
        kind: Kind::Str,
        fallback: Fallback::None,
        scope: Scope::Both,
        required: false,
        sensitive: false,
        min: 0,
        group: Group::Core,
        doc: "",
    };

    /// Mark the variable as mandatory.
    #[must_use]
    pub const fn required(self) -> Self {
        Self { required: true, ..self }
    }

    /// Mark the value as sensitive (redacted everywhere).
    #[must_use]
    pub const fn sensitive(self) -> Self {
        Self { sensitive: true, ..self }
    }

    /// Set the lower bound of an integer variable.
    #[must_use]
    pub const fn min(self, min: u64) -> Self {
        Self { min, ..self }
    }

    /// Whether the vault, not this crate, resolves the value.
    #[must_use]
    pub const fn is_secret(&self) -> bool {
        matches!(self.kind, Kind::Secret)
    }
}
