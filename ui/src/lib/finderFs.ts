import type { FinderNode } from "./types"

// ── Finder mock filesystem — one rich realm, re-rooted per agent ──────────────
//
// The Finder is confined to a single agent's folder (its realm). For the
// maquette we model one detailed, believable project tree and graft it onto
// whichever agent folder is active. Every file carries enough metadata + a
// preview payload to drive the QuickLook pane.

const SAMPLE_RS = `pub(crate) fn execute_send(tool: &ToolUse, state: &mut State) -> ToolResult {
    let tid = tool.input.get("thread_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis().to_u64());

    let msg = ThreadMessage {
        author: ThreadAuthor::Assistant,
        content: markdown,
        timestamp: now,
        acknowledged: true,
    };

    let ts = ThreadsState::get_mut(state);
    if let Some(thread) = ts.threads.iter_mut().find(|t| t.id == tid) {
        thread.messages.push(msg);
        thread.status = ThreadStatus::TheirTurn;
    }

    ToolResult::new(tool.id.clone(), format!("Sent to {tid}"), false)
}`.split("\n")

const SAMPLE_TS = `import { useState } from "react"
import { Finder } from "@/components/finder/Finder"

/** Per-agent file manager, confined to the agent's realm. */
export function Workspace({ root }: { root: string }) {
  const [cwd, setCwd] = useState(root)
  return <Finder root={root} cwd={cwd} onNavigate={setCwd} />
}`.split("\n")

const README_MD = `# context-pilot

A Rust TUI AI coding assistant. ~65K LOC across 22 crates.

## Philosophy
- The compiler is the reviewer.
- 980 forbid-level lints. Zero suppressions.
- Self-hosting: the tool builds the tool.

## Quick start
\`\`\`bash
cargo build --release
./run.sh
\`\`\`

See \`docs/\` for architecture notes.`

const ROADMAP_SHEET = {
  columns: ["Milestone", "Owner", "Status", "Target", "Confidence"],
  rows: [
    ["Threads module", "core", "Shipped", "Q2", "100%"],
    ["Finder view", "ui", "In progress", "Q3", "80%"],
    ["Multi-worker", "core", "Design", "Q3", "60%"],
    ["Memory v2", "core", "Paused", "Q4", "40%"],
    ["Web frontend", "ui", "Maquette", "Q4", "55%"],
    ["Cache engine v2", "core", "Shipped", "Q2", "100%"],
  ],
}

const DECK_SLIDES = [
  {
    title: "Context Pilot",
    bullets: ["A side project that took over everything", "65K LOC · 22 crates · 3 months"],
  },
  {
    title: "The Three WHYs",
    bullets: ["Context is everything", "Feedback must be instant", "The LLM is one knob of many"],
  },
  {
    title: "The Self-Improving Loop",
    bullets: ["The tool builds the tool", "Every commit sharpens the next", "Compound returns, daily"],
  },
]

const SPEC_PDF = {
  pages: 12,
  title: "Threads — Design Specification",
  excerpt: [
    "A thread is a parallel discussion or work topic owned by a single agent.",
    "Each thread carries a MY_TURN / THEIR_TURN status that drives focus.",
    "The Send tool posts a message; Read pulls history and sets focus.",
    "Coucou integration enables thread-scoped, recurrent scheduled nudges.",
  ],
}

/** Build the realm tree for an agent, rooted at its folder. */
export function buildRealm(folder: string, name: string): FinderNode {
  const p = (rel: string) => `${folder}/${rel}`
  return {
    name,
    path: folder,
    kind: "folder",
    modified: "just now",
    children: [
      {
        name: "src",
        path: p("src"),
        kind: "folder",
        modified: "12m ago",
        children: [
          {
            name: "lib.rs",
            path: p("src/lib.rs"),
            kind: "code",
            size: 18_432,
            modified: "12m ago",
            code: { lang: "rust", lines: SAMPLE_RS },
          },
          {
            name: "workspace.tsx",
            path: p("src/workspace.tsx"),
            kind: "code",
            size: 2_104,
            modified: "1h ago",
            code: { lang: "tsx", lines: SAMPLE_TS },
          },
          {
            name: "config.json",
            path: p("src/config.json"),
            kind: "json",
            size: 642,
            modified: "3h ago",
            text: `{
  "model": "claude-opus-4-8",
  "max_tokens": 64000,
  "thinking": { "type": "adaptive" },
  "cache": { "breakpoints": 4, "ttl_secs": 300 }
}`,
          },
        ],
      },
      {
        name: "docs",
        path: p("docs"),
        kind: "folder",
        modified: "2d ago",
        children: [
          {
            name: "threads-spec.pdf",
            path: p("docs/threads-spec.pdf"),
            kind: "pdf",
            size: 284_900,
            modified: "2d ago",
            pdf: SPEC_PDF,
          },
          {
            name: "architecture.png",
            path: p("docs/architecture.png"),
            kind: "image",
            size: 1_204_233,
            modified: "5d ago",
            image: { gradient: "linear-gradient(135deg,#00ADB5,#4F1C51)", w: 1600, h: 900 },
          },
          {
            name: "talk-deck.key",
            path: p("docs/talk-deck.key"),
            kind: "slides",
            size: 4_882_010,
            modified: "1w ago",
            slides: DECK_SLIDES,
          },
        ],
      },
      {
        name: "planning",
        path: p("planning"),
        kind: "folder",
        modified: "4h ago",
        children: [
          {
            name: "roadmap.xlsx",
            path: p("planning/roadmap.xlsx"),
            kind: "sheet",
            size: 38_220,
            modified: "4h ago",
            sheet: ROADMAP_SHEET,
          },
          {
            name: "budget.xlsx",
            path: p("planning/budget.xlsx"),
            kind: "sheet",
            size: 21_004,
            modified: "1d ago",
            sheet: {
              columns: ["Month", "API", "Infra", "Total"],
              rows: [
                ["April", "$142", "$30", "$172"],
                ["May", "$210", "$30", "$240"],
                ["June", "$318", "$45", "$363"],
              ],
            },
          },
        ],
      },
      {
        name: "README.md",
        path: p("README.md"),
        kind: "markdown",
        size: 1_842,
        modified: "6h ago",
        text: README_MD,
      },
      {
        name: "Cargo.toml",
        path: p("Cargo.toml"),
        kind: "code",
        size: 3_201,
        modified: "1d ago",
        code: {
          lang: "toml",
          lines: `[package]
name = "context-pilot"
edition = "2024"

[workspace]
members = ["crates/*"]

[lints.clippy]
all = "deny"
pedantic = "deny"`.split("\n"),
        },
      },
      {
        name: "assets.zip",
        path: p("assets.zip"),
        kind: "archive",
        size: 9_004_882,
        modified: "2w ago",
      },
    ],
  }
}

/** Depth-first lookup of a node by path. */
export function findNode(root: FinderNode, path: string): FinderNode | null {
  if (root.path === path) return root
  for (const c of root.children ?? []) {
    const hit = findNode(c, path)
    if (hit) return hit
  }
  return null
}

/** The chain of nodes from the realm root down to (and including) `path`. */
export function pathChain(root: FinderNode, path: string): FinderNode[] {
  const chain: FinderNode[] = []
  const walk = (node: FinderNode): boolean => {
    chain.push(node)
    if (node.path === path) return true
    for (const c of node.children ?? []) {
      if (walk(c)) return true
    }
    chain.pop()
    return false
  }
  walk(root)
  return chain
}

/** Human-readable byte size. */
export function fmtBytes(n?: number): string {
  if (n === undefined) return "—"
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)} KB`
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`
}

/** Folders first, then by the chosen key. */
export function sortNodes(
  nodes: FinderNode[],
  key: "name" | "size" | "modified" | "kind",
  asc: boolean,
): FinderNode[] {
  const dir = asc ? 1 : -1
  return [...nodes].sort((a, b) => {
    const ad = a.kind === "folder"
    const bd = b.kind === "folder"
    if (ad !== bd) return ad ? -1 : 1
    let cmp = 0
    if (key === "name") cmp = a.name.localeCompare(b.name)
    else if (key === "size") cmp = (a.size ?? 0) - (b.size ?? 0)
    else if (key === "kind") cmp = a.kind.localeCompare(b.kind)
    else cmp = a.name.localeCompare(b.name)
    return cmp * dir
  })
}
