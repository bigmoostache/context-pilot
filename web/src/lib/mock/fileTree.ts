import type { FsNode } from "../types"

/**
 * Mock filesystem for the workspace browser. Folders carrying an `agentId`
 * already host an agent; plain folders can have one initialized in them.
 *
 * Extracted from `mock/index.ts` (web-lint P2) so the barrel stays under the
 * 500-line structure cap after the Prettier reflow expanded its object
 * literals; re-exported from `./index` so `@/lib/mock` importers are unchanged.
 */
export const fileTree: FsNode = {
  name: "code",
  path: "~/code",
  kind: "dir",
  children: [
    {
      name: "context-pilot",
      path: "~/code/context-pilot",
      kind: "dir",
      agentId: "a-cp",
      children: [
        {
          name: "crates",
          path: "~/code/context-pilot/crates",
          kind: "dir",
          children: [
            {
              name: "cp-base",
              path: "~/code/context-pilot/crates/cp-base",
              kind: "dir",
              children: [],
            },
            {
              name: "cp-mod-threads",
              path: "~/code/context-pilot/crates/cp-mod-threads",
              kind: "dir",
              children: [],
            },
          ],
        },
        {
          name: "ui",
          path: "~/code/context-pilot/ui",
          kind: "dir",
          children: [
            { name: "src", path: "~/code/context-pilot/ui/src", kind: "dir", children: [] },
            { name: "package.json", path: "~/code/context-pilot/ui/package.json", kind: "file" },
          ],
        },
        { name: "Cargo.toml", path: "~/code/context-pilot/Cargo.toml", kind: "file" },
        { name: "README.md", path: "~/code/context-pilot/README.md", kind: "file" },
      ],
    },
    {
      name: "opio-rag",
      path: "~/code/opio-rag",
      kind: "dir",
      agentId: "a-opio",
      children: [
        { name: "src", path: "~/code/opio-rag/src", kind: "dir", children: [] },
        { name: "pyproject.toml", path: "~/code/opio-rag/pyproject.toml", kind: "file" },
      ],
    },
    {
      name: "maths",
      path: "~/code/maths",
      kind: "dir",
      children: [
        {
          name: "lean-proofs",
          path: "~/code/maths/lean-proofs",
          kind: "dir",
          agentId: "a-lean",
          children: [{ name: "Q6a.lean", path: "~/code/maths/lean-proofs/Q6a.lean", kind: "file" }],
        },
        {
          name: "scratch-notes",
          path: "~/code/maths/scratch-notes",
          kind: "dir",
          children: [
            { name: "ideas.md", path: "~/code/maths/scratch-notes/ideas.md", kind: "file" },
          ],
        },
      ],
    },
    {
      name: "website",
      path: "~/code/website",
      kind: "dir",
      children: [
        { name: "index.html", path: "~/code/website/index.html", kind: "file" },
        { name: "styles.css", path: "~/code/website/styles.css", kind: "file" },
      ],
    },
    {
      name: "experiments",
      path: "~/code/experiments",
      kind: "dir",
      children: [
        { name: "wasm-spike", path: "~/code/experiments/wasm-spike", kind: "dir", children: [] },
        { name: "notes.txt", path: "~/code/experiments/notes.txt", kind: "file" },
      ],
    },
  ],
}
