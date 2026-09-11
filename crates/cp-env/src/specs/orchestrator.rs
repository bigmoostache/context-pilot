//! Orchestrator topology: where it listens, where its agents and front are.

use crate::spec::{Exist, Fallback, Group, Kind, Scope, Spec};

/// Orchestrator topology.
pub(super) static SPECS: &[Spec] = &[
    Spec {
        name: "CP_ORCH_PORT",
        kind: Kind::U16,
        fallback: Fallback::Literal("7878"),
        scope: Scope::Orchestrator,
        group: Group::Orchestrator,
        doc: "TCP port of the REST + SSE API.",
        ..Spec::BASE
    },
    Spec {
        name: "CP_ORCH_BIND",
        kind: Kind::Str,
        fallback: Fallback::Literal("127.0.0.1"),
        scope: Scope::Orchestrator,
        group: Group::Orchestrator,
        doc: "Listen address. Loopback by default: the backend speaks cleartext and its auth model assumes an encrypted transport, so only the reverse proxy faces the LAN. Containers set `0.0.0.0` and publish the port on loopback instead.",
        ..Spec::BASE
    },
    Spec {
        name: "CP_SCAN_INTERVAL_MS",
        kind: Kind::U64,
        fallback: Fallback::Literal("2000"),
        scope: Scope::Orchestrator,
        group: Group::Orchestrator,
        doc: "Registry scan period, in milliseconds.",
        ..Spec::BASE
    }
    .min(1),
    Spec {
        name: "CP_AGENTS_ROOT",
        kind: Kind::Path(Exist::Dir),
        fallback: Fallback::Derived("$HOME/code"),
        scope: Scope::Orchestrator,
        group: Group::Orchestrator,
        doc: "Where the project directories of newly created agents are made.",
        ..Spec::BASE
    },
    Spec {
        name: "CP_AGENT_BINARY",
        kind: Kind::Path(Exist::Executable),
        fallback: Fallback::Derived("<cwd>/target/release/tui"),
        scope: Scope::Orchestrator,
        group: Group::Orchestrator,
        doc: "The agent binary the supervisor spawns. A persisted active release overrides it after an OTA update.",
        ..Spec::BASE
    },
    Spec {
        name: "CP_WEB_ROOT",
        kind: Kind::Path(Exist::Dir),
        fallback: Fallback::None,
        scope: Scope::Orchestrator,
        group: Group::Orchestrator,
        doc: "Directory of the built cockpit (SPA) served by the orchestrator. Unset, only the API is served. The updater repoints this path after a release swap.",
        ..Spec::BASE
    },
    Spec {
        name: "CP_PROVISION_FLAG",
        kind: Kind::Path(Exist::Parent),
        fallback: Fallback::Derived("<CP_AGENTS_DIR>/.provisioned"),
        scope: Scope::Orchestrator,
        group: Group::Orchestrator,
        doc: "Durable flag file written once the box identity is set (day-0 setup).",
        ..Spec::BASE
    },
    Spec {
        name: "CP_RELEASES_BREAK_GLASS",
        kind: Kind::Bool,
        fallback: Fallback::Literal("0"),
        scope: Scope::Orchestrator,
        group: Group::Orchestrator,
        doc: "Re-enable manual version selection in the releases API. The auto-updater owns version choice otherwise.",
        ..Spec::BASE
    },
];
