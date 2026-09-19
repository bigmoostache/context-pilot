//! The typed configuration the binaries read after validation.
//!
//! Every field is derived from a validated [`Raw`]; the accessors here never
//! consult the environment again. Derived defaults (`$HOME/…`, `<cwd>/…`) are
//! computed in this module and documented as such in the spec table.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::resolve::{self, Raw, Report, Strictness};
use crate::spec::Target;

pub mod appliance;
pub mod features;
pub mod gateway;
pub mod seed;

/// Everything both binaries share.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Core {
    /// `$HOME`.
    pub home: PathBuf,
    /// The agents registry directory.
    pub agents_dir: PathBuf,
    /// `$XDG_CONFIG_HOME`, or `$HOME/.config`.
    pub xdg_config_home: PathBuf,
}

/// Orchestrator topology.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Orch {
    /// API port.
    pub port: u16,
    /// Listen address.
    pub bind: String,
    /// Registry scan period.
    pub scan_interval: Duration,
    /// Root of new agents' project directories.
    pub agents_root: PathBuf,
    /// The agent binary to spawn.
    pub agent_binary: PathBuf,
    /// The served SPA, if any.
    pub web_root: Option<PathBuf>,
    /// The durable provisioned-flag file.
    pub provision_flag: PathBuf,
    /// Manual release selection re-enabled.
    pub releases_break_glass: bool,
}

/// Authentication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Auth {
    /// Login required.
    pub enabled: bool,
    /// Session lifetime.
    pub session_ttl: Duration,
    /// The auth `SQLite` database.
    pub db_path: PathBuf,
}

/// Agent <-> orchestrator bridge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bridge {
    /// Bridge active (`CP_BRIDGE=1`).
    pub enabled: bool,
    /// Orchestrator API base URL.
    pub url: String,
}

/// Developer conveniences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dev {
    /// Flame-graph profiling.
    pub flamegraph: bool,
    /// Supervised by `run.sh` (no self re-exec).
    pub run_sh: bool,
    /// Show `.context-pilot/` in the tree tool.
    pub show_context_pilot_in_tree: bool,
}

/// The whole validated configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Env {
    /// Shared basics.
    pub core: Core,
    /// Orchestrator topology.
    pub orch: Orch,
    /// Authentication.
    pub auth: Auth,
    /// First-boot accounts.
    pub seed: seed::Seed,
    /// Optional LLM gateway.
    pub gateway: gateway::Gateway,
    /// Agent bridge.
    pub bridge: Bridge,
    /// Appliance gates.
    pub appliance: appliance::Appliance,
    /// Behaviour flags.
    pub features: features::Features,
    /// Developer conveniences.
    pub dev: Dev,
}

impl Env {
    /// Build the typed view of validated values. `cwd` anchors the one
    /// default that is relative to the working directory (`CP_AGENT_BINARY`).
    ///
    /// Every `unwrap_or*` below covers a variable that [`resolve::resolve`]
    /// guarantees is present (required, or literally defaulted); they exist
    /// to keep this constructor infallible, not because the value can be
    /// missing.
    #[must_use]
    pub fn from_raw(raw: &Raw, cwd: &Path) -> Self {
        let home = raw.path("HOME").unwrap_or_else(|| PathBuf::from("."));
        let agents_dir = raw.path("CP_AGENTS_DIR").unwrap_or_else(|| home.join(".context-pilot/agents"));
        let core = Core {
            xdg_config_home: raw.path("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config")),
            agents_dir: agents_dir.clone(),
            home: home.clone(),
        };
        Self {
            orch: orch_from_raw(raw, &home, &agents_dir, cwd),
            auth: Auth {
                enabled: raw.flag("CP_AUTH_ENABLED").unwrap_or_default(),
                session_ttl: Duration::from_secs(raw.integer("CP_SESSION_TTL_SECS").unwrap_or_default()),
                db_path: raw.path("CP_AUTH_DB").unwrap_or_else(|| home.join(".context-pilot/orchestrator/auth.db")),
            },
            seed: seed::Seed::from_raw(raw),
            gateway: gateway::Gateway::from_raw(raw),
            bridge: Bridge {
                enabled: raw.flag("CP_BRIDGE").unwrap_or_default(),
                url: raw.text("CP_BRIDGE_URL").unwrap_or_default().to_owned(),
            },
            appliance: appliance::Appliance::from_raw(raw),
            features: features::Features::from_raw(raw),
            dev: Dev {
                flamegraph: raw.flag("CP_FLAMEGRAPH").unwrap_or_default(),
                run_sh: raw.flag("CP_RUN_SH").unwrap_or_default(),
                show_context_pilot_in_tree: raw.flag("SHOW_CONTEXT_PILOT_IN_TREE").unwrap_or_default(),
            },
            core,
        }
    }

    /// Validate an in-memory environment strictly and build the typed view -
    /// the test entry point: no process environment involved.
    ///
    /// # Errors
    ///
    /// The aggregated [`Report`] when validation fails.
    pub fn from_map(map: &BTreeMap<String, String>, target: Target) -> Result<Self, Report> {
        let raw = resolve::resolve(target, map, Strictness::Strict)?;
        Ok(Self::from_raw(&raw, Path::new(".")))
    }
}

/// The orchestrator block.
fn orch_from_raw(raw: &Raw, home: &Path, agents_dir: &Path, cwd: &Path) -> Orch {
    Orch {
        port: raw.integer("CP_ORCH_PORT").and_then(|port| u16::try_from(port).ok()).unwrap_or_default(),
        bind: raw.text("CP_ORCH_BIND").unwrap_or_default().to_owned(),
        scan_interval: Duration::from_millis(raw.integer("CP_SCAN_INTERVAL_MS").unwrap_or_default()),
        agents_root: raw.path("CP_AGENTS_ROOT").unwrap_or_else(|| home.join("code")),
        agent_binary: raw.path("CP_AGENT_BINARY").unwrap_or_else(|| cwd.join("target/release/tui")),
        web_root: raw.path("CP_WEB_ROOT"),
        provision_flag: raw.path("CP_PROVISION_FLAG").unwrap_or_else(|| agents_dir.join(".provisioned")),
        releases_break_glass: raw.flag("CP_RELEASES_BREAK_GLASS").unwrap_or_default(),
    }
}
