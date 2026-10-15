# Docs index

Status of every doc in this folder, checked against the code on 2026-10-15. Since T860, "thread" in older docs means **agent** (`cp-mod-agents`), and conversations live in **groups** (`cp-groups`).

| Status | Meaning |
| --- | --- |
| **Shipped** | Implemented; the doc describes the code. |
| **Shipped, historical** | Implemented, but the code has since moved on; read for rationale, not for current details. |
| **In progress** | Partly implemented, still being built. |
| **Reference** | Not a design: a guide, research or a business doc. |

## Architecture and design

| Doc | Status | Code |
| --- | --- | --- |
| [design-orchestration.md](design-orchestration.md) | In progress (v2, T860; phases A–G) | `cp-mod-agents`, `cp-groups`, `cp-management`, `cp-coordination` |
| [design-multi-worker.md](design-multi-worker.md) | Shipped (v8, agent-centric core loop) | `cp-fleet`, `src/app` |
| [design-subworkers.md](design-subworkers.md) | Shipped, historical (superseded by design-multi-worker.md) | `cp-fleet` |
| [design-orchestration-backend.md](design-orchestration-backend.md) | Shipped, historical (original brainstorm) | `cp-orchestrator`, `cp-oplog`, `cp-mod-bridge` |
| [design-sync-dir-migration.md](design-sync-dir-migration.md) | Shipped (T713, PR #192) | `cp-mod-bridge/src/boot` |
| [design-auth.md](design-auth.md) | Shipped (v3 RBAC) | `cp-orchestrator/src/services/auth`, `transport/auth` |
| [auth-v3-plan.md](auth-v3-plan.md) | Shipped, historical (implementation plan for design-auth §13) | same |
| [update-policy.md](update-policy.md) | Frozen spec (v3) | `cp-orchestrator`, `deploy/` |
| [design-mobile.md](design-mobile.md) | Shipped | `web/src/mobile-components`, `web/src/switch`, `check-mobile-mirror.sh` |
| [forms.md](forms.md) | Shipped (T570) | `web/src/*/agents/forms`, Send validation |
| [design-conversation-search.md](design-conversation-search.md) | Shipped (T671) | `cp-mod-search/src/meili/conversation_search.rs` |
| [design-search-portability.md](design-search-portability.md) | Shipped (boot reconcile + embedding backup) | `cp-mod-search` |
| [design-memory-slots.md](design-memory-slots.md) | Shipped (T650) | `cp-mod-memory` |
| [design-shared-behaviours.md](design-shared-behaviours.md) | Shipped (T651) | `~/.context-pilot/behaviours/` loader |
| [design-thread-tasks.md](design-thread-tasks.md) | Shipped, historical (todos are agent-owned) | `cp-mod-todo` |
| [plan-todo-v2.md](plan-todo-v2.md) | Shipped, historical (implementation plan for the above) | `cp-mod-todo` |

## Reference

| Doc | What it is |
| --- | --- |
| [ENV.md](ENV.md) | Environment variables. **Generated** from `crates/cp-env`; never edit by hand. |
| [design-thread-collaboration.md](design-thread-collaboration.md) | Research review (T820) on LLM agent collectives. |
| [positioning.md](positioning.md) | Product positioning and value matrix (French). |
| [LCD_SCREEN.md](LCD_SCREEN.md) | Photonicat 2 LCD setup guide (French). |
| `trust-center/`, `blog/`, `documentation/`, `index.html` | Static site pages (published by `pages.yml`). |
| `presentations/` | Talk slides; `talk/img` is 22 MB of the 23 MB in `docs/`. |

When you change behaviour a doc describes, update the doc and its row here in the same PR.
