//! Variables both binaries read, plus the developer conveniences.

use crate::spec::{Exist, Fallback, Group, Kind, Scope, Spec};

/// Shared by the orchestrator and the agent.
pub(super) static CORE: &[Spec] = &[
    Spec {
        name: "HOME",
        kind: Kind::Path(Exist::Dir),
        fallback: Fallback::None,
        scope: Scope::Both,
        group: Group::Core,
        doc: "Home of the process. `~/.context-pilot` (global `.env`, registry, `auth.db`, releases, Meilisearch), the Claude credentials file and the default agent code root all live under it.",
        ..Spec::BASE
    }
    .required(),
    Spec {
        name: "XDG_CONFIG_HOME",
        kind: Kind::Path(Exist::No),
        fallback: Fallback::Derived("$HOME/.config"),
        scope: Scope::Both,
        group: Group::Core,
        doc: "Parent of the central `context-pilot/config.json` store (Linux only).",
        ..Spec::BASE
    },
    Spec {
        name: "CP_AGENTS_DIR",
        kind: Kind::Path(Exist::No),
        fallback: Fallback::Derived("$HOME/.context-pilot/agents"),
        scope: Scope::Both,
        group: Group::Core,
        doc: "Registry directory shared by the orchestrator and its agents (created on demand). The orchestrator passes its own value to every agent it spawns.",
        ..Spec::BASE
    },
];

/// Local development only.
pub(super) static DEV: &[Spec] = &[
    Spec {
        name: "CP_FLAMEGRAPH",
        kind: Kind::Bool,
        fallback: Fallback::Literal("0"),
        scope: Scope::Agent,
        group: Group::Dev,
        doc: "Write flame-graph profiling data (`run.sh --flamegraph`).",
        ..Spec::BASE
    },
    Spec {
        name: "CP_RUN_SH",
        kind: Kind::Bool,
        fallback: Fallback::Literal("0"),
        scope: Scope::Agent,
        group: Group::Dev,
        doc: "Set by `run.sh`: the supervisor script handles reloads, so the agent must not re-exec itself.",
        ..Spec::BASE
    },
    Spec {
        name: "SHOW_CONTEXT_PILOT_IN_TREE",
        kind: Kind::Bool,
        fallback: Fallback::Literal("0"),
        scope: Scope::Agent,
        group: Group::Dev,
        doc: "Show the `.context-pilot/` directory in the tree tool.",
        ..Spec::BASE
    },
];
