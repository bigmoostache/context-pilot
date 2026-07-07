import type { FinderNode, FinderSortKey } from "../types"

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

See the **docs/** folder for architecture notes.`

const CHANGELOG_MD = `# Changelog

## v0.2.10 — Deadman aware of retry
- Stream-retry activity now bumps the progress clock.
- The deadman no longer fights the in-process retry budget.

## v0.2.9 — Stream completion timeout
- 90s wall-clock guard from stream start.

## v0.2.8 — Dedicated-thread deadman
- Re-exec / abort on wedged headless loop.`

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
    bullets: [
      "The tool builds the tool",
      "Every commit sharpens the next",
      "Compound returns, daily",
    ],
  },
  {
    title: "Where it's going",
    bullets: ["Multi-worker parallelism", "A web frontend", "Your whole desk, piloted"],
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

// Pseudo-random but deterministic waveform peaks for the audio preview.
const PEAKS = Array.from(
  { length: 64 },
  (_, i) => 0.25 + 0.7 * Math.abs(Math.sin(i * 0.7) * Math.cos(i * 0.31) + Math.sin(i * 1.9) * 0.4),
)

/** Build the realm tree for an agent, rooted at its folder. */
export function buildRealm(folder: string, name: string): FinderNode {
  const p = (rel: string) => `${folder}/${rel}`
  return {
    name,
    path: folder,
    kind: "folder",
    modified: "just now",
    created: "3 months ago",
    children: [
      {
        name: "src",
        path: p("src"),
        kind: "folder",
        modified: "12m ago",
        created: "3 months ago",
        starred: true,
        children: [
          {
            name: "lib.rs",
            path: p("src/lib.rs"),
            kind: "code",
            size: 18_432,
            modified: "12m ago",
            created: "2 months ago",
            tags: ["green"],
            starred: true,
            code: { lang: "rust", lines: SAMPLE_RS },
          },
          {
            name: "workspace.tsx",
            path: p("src/workspace.tsx"),
            kind: "code",
            size: 2104,
            modified: "1h ago",
            created: "1w ago",
            tags: ["blue"],
            code: { lang: "tsx", lines: SAMPLE_TS },
          },
          {
            name: "config.json",
            path: p("src/config.json"),
            kind: "json",
            size: 642,
            modified: "3h ago",
            created: "2 months ago",
            text: `{
  "model": "claude-opus-4-8",
  "max_tokens": 64000,
  "thinking": { "type": "adaptive" },
  "cache": { "breakpoints": 4, "ttl_secs": 300 }
}`,
          },
          {
            name: "modules",
            path: p("src/modules"),
            kind: "folder",
            modified: "2d ago",
            created: "2 months ago",
            children: [
              {
                name: "threads.rs",
                path: p("src/modules/threads.rs"),
                kind: "code",
                size: 9210,
                modified: "2d ago",
                created: "2 months ago",
                code: { lang: "rust", lines: SAMPLE_RS },
              },
              {
                name: "cache.rs",
                path: p("src/modules/cache.rs"),
                kind: "code",
                size: 14_004,
                modified: "5d ago",
                created: "2 months ago",
                code: { lang: "rust", lines: SAMPLE_RS },
              },
            ],
          },
        ],
      },
      {
        name: "docs",
        path: p("docs"),
        kind: "folder",
        modified: "2d ago",
        created: "3 months ago",
        children: [
          {
            name: "threads-spec.pdf",
            path: p("docs/threads-spec.pdf"),
            kind: "pdf",
            size: 284_900,
            modified: "2d ago",
            created: "2w ago",
            tags: ["red"],
            pdf: SPEC_PDF,
          },
          {
            name: "architecture.png",
            path: p("docs/architecture.png"),
            kind: "image",
            size: 1_204_233,
            modified: "5d ago",
            created: "1mo ago",
            tags: ["orange"],
            image: { gradient: "linear-gradient(135deg,#FFD369,#393E46 70%)", w: 1600, h: 900 },
          },
          {
            name: "logo.png",
            path: p("docs/logo.png"),
            kind: "image",
            size: 88_400,
            modified: "1mo ago",
            created: "2mo ago",
            image: {
              gradient: "radial-gradient(circle at 35% 30%,#6fb585,#222831 75%)",
              w: 512,
              h: 512,
            },
          },
          {
            name: "talk-deck.key",
            path: p("docs/talk-deck.key"),
            kind: "slides",
            size: 4_882_010,
            modified: "1w ago",
            created: "1mo ago",
            tags: ["purple"],
            slides: DECK_SLIDES,
          },
          {
            name: "CHANGELOG.md",
            path: p("docs/CHANGELOG.md"),
            kind: "markdown",
            size: 2410,
            modified: "1d ago",
            created: "3 months ago",
            text: CHANGELOG_MD,
          },
        ],
      },
      {
        name: "media",
        path: p("media"),
        kind: "folder",
        modified: "6h ago",
        created: "1w ago",
        children: [
          {
            name: "demo.mp4",
            path: p("media/demo.mp4"),
            kind: "video",
            size: 18_442_200,
            modified: "6h ago",
            created: "1w ago",
            tags: ["blue"],
            media: {
              kind: "video",
              duration: "2:14",
              poster: "linear-gradient(135deg,#393E46,#222831)",
            },
          },
          {
            name: "voice-note.m4a",
            path: p("media/voice-note.m4a"),
            kind: "audio",
            size: 1_002_400,
            modified: "1d ago",
            created: "1d ago",
            media: { kind: "audio", duration: "0:48", peaks: PEAKS },
          },
        ],
      },
      {
        name: "planning",
        path: p("planning"),
        kind: "folder",
        modified: "4h ago",
        created: "2mo ago",
        children: [
          {
            name: "roadmap.xlsx",
            path: p("planning/roadmap.xlsx"),
            kind: "sheet",
            size: 38_220,
            modified: "4h ago",
            created: "2mo ago",
            tags: ["blue", "yellow"],
            starred: true,
            sheet: ROADMAP_SHEET,
          },
          {
            name: "budget.xlsx",
            path: p("planning/budget.xlsx"),
            kind: "sheet",
            size: 21_004,
            modified: "1d ago",
            created: "2mo ago",
            tags: ["green"],
            sheet: {
              columns: ["Month", "API", "Infra", "Total"],
              rows: [
                ["April", "$142", "$30", "$172"],
                ["May", "$210", "$30", "$240"],
                ["June", "$318", "$45", "$363"],
                ["July", "$402", "$45", "$447"],
              ],
            },
          },
        ],
      },
      {
        name: "README.md",
        path: p("README.md"),
        kind: "markdown",
        size: 1842,
        modified: "6h ago",
        created: "3 months ago",
        starred: true,
        text: README_MD,
      },
      {
        name: "Cargo.toml",
        path: p("Cargo.toml"),
        kind: "code",
        size: 3201,
        modified: "1d ago",
        created: "3 months ago",
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
        created: "2w ago",
        tags: ["gray"],
      },
    ],
  }
}

/** Depth-first lookup of a node by path. */
export function findNode(root: FinderNode, path: string): FinderNode | null {
  if (root.path === path) return root
  const kids = root.children ?? []
  for (const c of kids) {
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
    const kids = node.children ?? []
    for (const c of kids) {
      if (walk(c)) return true
    }
    chain.pop()
    return false
  }
  walk(root)
  return chain
}

/** Flatten every starred node in the realm (for the Favorites sidebar). */
export function collectStarred(root: FinderNode): FinderNode[] {
  const out: FinderNode[] = []
  const walk = (n: FinderNode) => {
    if (n.starred && n.path !== root.path) out.push(n)
    const kids = n.children ?? []
    for (const c of kids) walk(c)
  }
  walk(root)
  return out
}

/** Human-readable byte size. */
export function fmtBytes(n?: number | null): string {
  if (n == null) return "—"
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)} KB`
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`
}

/** Total byte size of a node (recursive for folders). */
export function nodeSize(n: FinderNode): number {
  if (n.kind !== "folder") return n.size ?? 0
  return (n.children ?? []).reduce((s, c) => s + nodeSize(c), 0)
}

/** Count of direct children, split into folders / files. */
export function childCounts(n: FinderNode): { folders: number; files: number } {
  const kids = n.children ?? []
  const folders = kids.filter((k) => k.kind === "folder").length
  return { folders, files: kids.length - folders }
}

/** Folders first, then by the chosen key. */
export function sortNodes(nodes: FinderNode[], key: FinderSortKey, asc: boolean): FinderNode[] {
  const dir = asc ? 1 : -1
  return nodes.toSorted((a, b) => {
    const ad = a.kind === "folder"
    const bd = b.kind === "folder"
    if (ad !== bd) return ad ? -1 : 1
    let cmp: number
    switch (key) {
      case "name": {
        cmp = a.name.localeCompare(b.name)
        break
      }
      case "size": {
        cmp = (a.size ?? 0) - (b.size ?? 0)
        break
      }
      case "kind": {
        cmp = a.kind.localeCompare(b.kind)
        break
      }
      default: {
        cmp = a.name.localeCompare(b.name)
      }
    }
    return cmp * dir
  })
}
