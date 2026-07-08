import type { LibraryItem, User } from "../types"

// ── Surviving fixtures ────────────────────────────────────────────────
//
// The app is fully demaquetted (M63): every cockpit / usage / finder surface
// now renders live backend data, so the old maquette payloads (panels, threads,
// spine, usage, tree, radar, memory, entities, tool/queue/scratch/callback rows)
// were deleted. Only two fixtures remain, each still consumed by a live view
// that has no backend source yet:
//   • `library` — the fleet dashboard's Prompts page (global prompt library),
//     used as the fallback when an agent's live library hasn't loaded.
//   • `currentUser` — the account identity shown when auth is disabled.

/** Local account identity — the fallback user when auth is off. */
export const currentUser: User = {
  name: "Guillaume Draznieks",
  email: "g.draznieks@gmail.com",
  initials: "GD",
  accent: "interactive",
  managedByCompany: false,
  company: undefined,
  role: "Admin",
}

// ── Global prompt library (Prompts page) ──────────────────────────
// Drawn from the TUI's prompt library; presented as if it were already global
// (the captain's intent), shared across every agent.
export const library: LibraryItem[] = [
  // agents
  {
    id: "threaded-consciousness",
    name: "Threaded Consciousness",
    kind: "agent",
    description: "Two-surface model — private reasoning loop, polished thread replies.",
    meta: "active on 1 agent",
    active: true,
  },
  {
    id: "default",
    name: "Default",
    kind: "agent",
    description: "General-purpose coding assistant.",
    meta: "built-in",
    builtin: true,
  },
  {
    id: "worker",
    name: "Worker",
    kind: "agent",
    description: "Focused implementation & testing — heads-down execution.",
    meta: "built-in",
    builtin: true,
  },
  {
    id: "planner",
    name: "Planner",
    kind: "agent",
    description: "Task planning and breakdown before any code is touched.",
    meta: "built-in",
    builtin: true,
  },
  {
    id: "context-builder",
    name: "Context Builder",
    kind: "agent",
    description: "Explores an unfamiliar codebase and maps its structure.",
    meta: "built-in",
    builtin: true,
  },
  {
    id: "context-cleaner",
    name: "Context Cleaner",
    kind: "agent",
    description: "Trims and reshapes context for hygiene.",
    meta: "built-in",
    builtin: true,
  },
  {
    id: "cartographer",
    name: "Cartographer",
    kind: "agent",
    description: "Background agent that describes files & folders in the tree.",
    meta: "reverie",
    builtin: true,
  },
  {
    id: "pirate-coder",
    name: "Pirate Coder",
    kind: "agent",
    description: "A salty buccaneer who loves the sea and clean diffs.",
    meta: "custom",
  },

  // skills
  {
    id: "frontend",
    name: "frontend-design",
    kind: "skill",
    description: "Distinctive, production-grade frontend interfaces — avoids generic AI slop.",
    meta: "loaded",
    active: true,
  },
  {
    id: "egui",
    name: "egui",
    kind: "skill",
    description: "egui immediate-mode GUI framework knowledge & patterns.",
    meta: "—",
  },
  {
    id: "brave-goggles",
    name: "Brave Goggles",
    kind: "skill",
    description: "Curated Brave Search goggles for domain re-ranking.",
    meta: "—",
  },
  {
    id: "setup-guides",
    name: "Setup Guides",
    kind: "skill",
    description: "How to wire Telegram, Discord, Slack, Brave, Firecrawl, GitHub.",
    meta: "—",
  },

  // commands
  {
    id: "boss-hunt",
    name: "/boss-hunt",
    kind: "command",
    description: "Slow, methodical lint & quality sweep.",
    meta: "/boss-hunt",
  },
  {
    id: "clean",
    name: "/clean",
    kind: "command",
    description: "Launch a reverie cleaner, then resume work in progress.",
    meta: "/clean",
  },
  {
    id: "hello",
    name: "/hello",
    kind: "command",
    description: "A simple greeting — handy for smoke-testing.",
    meta: "/hello",
    builtin: true,
  },
]
