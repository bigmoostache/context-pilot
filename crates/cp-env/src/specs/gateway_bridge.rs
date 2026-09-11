//! The optional LLM gateway and the agent <-> orchestrator bridge.

use crate::spec::{Fallback, Group, Kind, Scope, Spec};

/// Gateway and bridge.
pub(super) static SPECS: &[Spec] = &[
    Spec {
        name: "CP_LLM_GATEWAY",
        kind: Kind::Url,
        fallback: Fallback::None,
        scope: Scope::Both,
        group: Group::Gateway,
        doc: "Base URL of the gateway. Unset or empty means no gateway: each provider is called directly with its own key.",
        ..Spec::BASE
    },
    Spec {
        name: "CP_LLM_GATEWAY_KEY",
        kind: Kind::Str,
        fallback: Fallback::None,
        scope: Scope::Both,
        group: Group::Gateway,
        doc: "Key presented to the gateway on every call. Requires `CP_LLM_GATEWAY`.",
        ..Spec::BASE
    }
    .sensitive(),
    Spec {
        name: "CP_BRIDGE",
        kind: Kind::Bool,
        fallback: Fallback::Literal("0"),
        scope: Scope::Agent,
        group: Group::Bridge,
        doc: "Activate the orchestration bridge module and the bridge-backed vault. The orchestrator sets 1 on every agent it spawns; `cpilot --bridge` is the CLI equivalent.",
        ..Spec::BASE
    },
    Spec {
        name: "CP_BRIDGE_URL",
        kind: Kind::Url,
        fallback: Fallback::Literal("http://127.0.0.1:7878"),
        scope: Scope::Agent,
        group: Group::Bridge,
        doc: "The orchestrator API as seen by the agent.",
        ..Spec::BASE
    },
];
