//! Authentication switches and the first-boot account seed.

use crate::spec::{Exist, Fallback, Group, Kind, Scope, Spec};

/// Authentication and account seeding.
pub(super) static SPECS: &[Spec] = &[
    Spec {
        name: "CP_AUTH_ENABLED",
        kind: Kind::Bool,
        fallback: Fallback::Literal("0"),
        scope: Scope::Orchestrator,
        group: Group::Auth,
        doc: "Require login and enforce role-based access. Every production profile sets 1; off, every caller is treated as a superadmin.",
        ..Spec::BASE
    },
    Spec {
        name: "CP_SESSION_TTL_SECS",
        kind: Kind::U64,
        fallback: Fallback::Literal("2592000"),
        scope: Scope::Orchestrator,
        group: Group::Auth,
        doc: "Session lifetime in seconds (default 30 days).",
        ..Spec::BASE
    }
    .min(60),
    Spec {
        name: "CP_AUTH_DB",
        kind: Kind::Path(Exist::Parent),
        fallback: Fallback::Derived("$HOME/.context-pilot/orchestrator/auth.db"),
        scope: Scope::Orchestrator,
        group: Group::Auth,
        doc: "The auth SQLite database (users, sessions, agent ACL). Orchestrator-level, never inside the agents directory.",
        ..Spec::BASE
    },
    // ── Vendor account ──────────────────────────────────────────────────
    Spec {
        name: "CP_SEED_SUPERADMIN_EMAIL",
        kind: Kind::Str,
        fallback: Fallback::None,
        scope: Scope::Orchestrator,
        group: Group::Seed,
        doc: "Email of the vendor account (superadmin: provider secrets, IT settings, the only role that can create another superadmin). Setting it enables the seed.",
        ..Spec::BASE
    },
    Spec {
        name: "CP_SEED_SUPERADMIN_NAME",
        kind: Kind::Str,
        fallback: Fallback::Literal("superadmin"),
        scope: Scope::Orchestrator,
        group: Group::Seed,
        doc: "Display name of the vendor account.",
        ..Spec::BASE
    },
    Spec {
        name: "CP_SEED_SUPERADMIN_PASSWORD",
        kind: Kind::Str,
        fallback: Fallback::None,
        scope: Scope::Orchestrator,
        group: Group::Seed,
        doc: "Initial password of the vendor account (changed at first login).",
        ..Spec::BASE
    }
    .sensitive(),
    Spec {
        name: "CP_SEED_SUPERADMIN_PASSWORD_FILE",
        kind: Kind::Path(Exist::File),
        fallback: Fallback::None,
        scope: Scope::Orchestrator,
        group: Group::Seed,
        doc: "File holding the initial password of the vendor account (preferred: keeps the secret out of the process environment). Wins over the inline variable.",
        ..Spec::BASE
    },
    // ── Client account ──────────────────────────────────────────────────
    Spec {
        name: "CP_SEED_ADMIN_EMAIL",
        kind: Kind::Str,
        fallback: Fallback::None,
        scope: Scope::Orchestrator,
        group: Group::Seed,
        doc: "Email of the client's top account (admin: everything but provider secrets). Optional; a superadmin can create it from the cockpit.",
        ..Spec::BASE
    },
    Spec {
        name: "CP_SEED_ADMIN_NAME",
        kind: Kind::Str,
        fallback: Fallback::Literal("admin"),
        scope: Scope::Orchestrator,
        group: Group::Seed,
        doc: "Display name of the client's top account.",
        ..Spec::BASE
    },
    Spec {
        name: "CP_SEED_ADMIN_PASSWORD",
        kind: Kind::Str,
        fallback: Fallback::None,
        scope: Scope::Orchestrator,
        group: Group::Seed,
        doc: "Initial password of the client's top account (changed at first login).",
        ..Spec::BASE
    }
    .sensitive(),
    Spec {
        name: "CP_SEED_ADMIN_PASSWORD_FILE",
        kind: Kind::Path(Exist::File),
        fallback: Fallback::None,
        scope: Scope::Orchestrator,
        group: Group::Seed,
        doc: "File holding the initial password of the client's top account. Wins over the inline variable.",
        ..Spec::BASE
    },
];
