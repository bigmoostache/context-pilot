import { kindOf } from "@/components/finder/support/kind"
import type { FinderNode } from "@/lib/types"

/**
 * One file attached to a thread via the chat composer. The composer uploads the
 * file to the realm's `.uploads/` and embeds these fields into the user message
 * as a ` ```file-upload ` YAML block (one block per file); the conversation view
 * parses the blocks back out and renders each as a clickable `FileUploadChip`.
 */
export interface UploadedFile {
  /** realm-relative stored path, e.g. `.uploads/report (1).pdf` */
  path: string
  /** stored filename */
  name: string
  /** byte count */
  size: number
  /** provenance note, e.g. `uploaded by user at 2026-…` */
  note: string
}

/**
 * A `/command` offered as a composer suggestion bubble (T348/T350). `command`
 * is the literal slash token (e.g. `/clean`); `name` + `description` label the
 * bubble; `body` is the expanded prompt seeded on click (when present).
 *
 * Lives here — beside {@link UploadedFile} — so the composer's two pill families
 * (file attachments + command suggestions) share ONE module and ONE rendered row
 * (`ComposerBubbles`), instead of two divergent ad-hoc blocks that can't coexist.
 */
export interface CommandSuggestion {
  command: string
  name: string
  description: string
  /** the prompt body the `/command` expands to; seeded into the composer on
   *  click. Falls back to the bare `command` literal when absent. */
  body?: string | undefined
}

/**
 * Compose a user message body carrying one ` ```file-upload ` YAML block per
 * uploaded file. The conversation renderer ({@link splitMessageSegments})
 * extracts these blocks and turns them into clickable preview chips rendered
 * **inline at the block's position**; the agent reads the same YAML as plain
 * context, so it knows which files were attached.
 */
export function buildUploadMessage(files: UploadedFile[]): string {
  return files
    .map((f) =>
      [
        "```file-upload",
        "file:",
        `  path: ${f.path}`,
        `  name: ${f.name}`,
        `  size: ${f.size}`,
        `  note: ${f.note}`,
        "```",
      ].join("\n"),
    )
    .join("\n\n")
}

const BLOCK_RE = /```file-upload\n([\s\S]*?)```/g

/** One `key: value` line inside a `file-upload` block body (indented under
 *  `file:`). A SINGLE literal regex, applied per line — no per-key dynamic
 *  `new RegExp(key)` (which the source keys are trusted-literal but the pattern
 *  the security detector rightly flags), and a `Map` sink rather than a plain
 *  object so there is no computed-key object-injection surface either. */
// `\w+` (the key) and `\s` (the indent) share no characters, and the value is
// an unanchored `(.*)` with NO leading `\s*` — dropping that post-colon `\s*`
// removes the only overlapping quantifier pair (`\s*`↔`.*` could both eat the
// value's leading spaces, the polynomial-backtracking window regexp/no-super-
// linear-backtracking rightly flags); the value is `.trim()`d anyway.
const FIELD_RE = /^\s*(\w+):(.*)$/

/** Parse the indented `key: value` lines of a `file-upload` block body into a
 *  first-wins lookup (one literal-regex scan). */
function fields(body: string): Map<string, string> {
  const out = new Map<string, string>()
  for (const line of body.split("\n")) {
    const m = FIELD_RE.exec(line)
    if (m === null) continue
    const key = m[1]
    if (key !== undefined && !out.has(key)) out.set(key, (m[2] ?? "").trim())
  }
  return out
}

/** Parse one `file-upload` block body into an {@link UploadedFile} (tolerant:
 *  a missing `name` falls back to the path's basename, a missing `size` to 0). */
function parseBlock(body: string): UploadedFile | null {
  const f = fields(body)
  const path = f.get("path") ?? ""
  if (!path) return null
  return {
    path,
    name: (f.get("name") ?? "") || path.split("/").pop() || path,
    size: Number(f.get("size") ?? "") || 0,
    note: f.get("note") ?? "",
  }
}

/** An ordered render segment of a message body: a run of prose, or one attached
 *  file parsed from a ` ```file-upload ` block at the exact position it appeared. */
export type MessageSegment = { type: "text"; text: string } | { type: "file"; file: UploadedFile }

/**
 * Split a message body into ordered segments, replacing each ` ```file-upload `
 * block **in place** with a `file` segment so the conversation renderer can draw
 * the attachment chip exactly where the block sat in the markdown — interleaved
 * with the surrounding prose, not hoisted into a separate trailing block.
 *
 * Whitespace-only text runs (the blank lines the composer puts between blocks)
 * are dropped so consecutive attachments don't render with empty paragraphs
 * between them. A message with no blocks yields a single `text` segment (the
 * common case — zero behavioural change for ordinary messages).
 */
export function splitMessageSegments(text: string): MessageSegment[] {
  const out: MessageSegment[] = []
  let last = 0
  let match: RegExpExecArray | null
  BLOCK_RE.lastIndex = 0
  while ((match = BLOCK_RE.exec(text)) !== null) {
    const before = text.slice(last, match.index)
    if (before.trim().length > 0) out.push({ type: "text", text: before })
    const file = parseBlock(match[1] ?? "")
    if (file) out.push({ type: "file", file })
    last = match.index + match[0].length
  }
  const tail = text.slice(last)
  if (tail.trim().length > 0) out.push({ type: "text", text: tail })
  return out
}

/** Synthesize a {@link FinderNode} for an attached file so it can drive the
 *  shared Quick Look drawer (kind inferred from the filename, like the Finder). */
export function uploadToNode(f: UploadedFile): FinderNode {
  return { name: f.name, path: f.path, kind: kindOf(f.name), size: f.size, modified: "" }
}
