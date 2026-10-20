import type { ChatMessage, AgentDetail, AgentMsg } from "@/lib/types"

/** Format a whole-second age as a compact "Xm ago" relative label. */
function relAge(seconds: number): string {
  const s = Math.max(0, seconds)
  if (s < 5) return "just now"
  if (s < 60) return `${s}s ago`
  const m = Math.floor(s / 60)
  if (m < 60) return `${m}m ago`
  const h = Math.floor(m / 60)
  if (h < 24) return `${h}h ago`
  return `${Math.floor(h / 24)}d ago`
}

/** Epoch-ms sentinel: 2020-01-01 00:00:00 UTC — anything above is a real date. */
const EPOCH_2020 = 1_577_836_800_000

/**
 * Normalise a agent message's `ts` into a human-readable relative age.
 *
 * The field arrives as either an epoch-ms number (REST backstop poll), an
 * ISO 8601 string (SSE delta reducer), or an already-formatted relative
 * string — this helper collapses all three into a single "Xm ago" label so
 * the Message renderer never shows a raw timestamp.
 */
export function formatTs(ts: string | number | undefined): string {
  if (ts === undefined) return ""
  // Epoch-ms as a number or numeric string.
  const n = typeof ts === "number" ? ts : Number(ts)
  if (!Number.isNaN(n) && n > EPOCH_2020) {
    return relAge(Math.floor((Date.now() - n) / 1000))
  }
  // ISO 8601 string (from the SSE reducer).
  if (typeof ts === "string") {
    const t = new Date(ts).getTime()
    if (!Number.isNaN(t) && t > EPOCH_2020) {
      return relAge(Math.floor((Date.now() - t) / 1000))
    }
  }
  // Already formatted or unknown — pass through.
  return String(ts)
}

/** Map a agent message onto the shared ChatMessage shape for the renderer. */
export function toChatMessage(m: AgentMsg): ChatMessage {
  return {
    id: m.id,
    role: m.tool ? "tool" : m.author,
    text: m.text,
    tool: m.tool,
    ts: formatTs(m.ts),
    streaming: m.streaming,
    pending: m.pending,
  }
}

/** Parse an auto-trace message into its three columns: verb, tool, intent. */
export function parseAutoLine(m: AgentMsg): { verb: string; tool: string; intent: string } {
  const raw = m.text ?? ""
  const t = raw.startsWith("/* auto */ ") ? raw.slice("/* auto */ ".length) : raw
  const dotIdx = t.indexOf(" · ")
  if (dotIdx === -1) return { verb: t, tool: "", intent: "" }
  const verb = t.slice(0, dotIdx)
  const rest = t.slice(dotIdx + 3)
  const dashIdx = rest.indexOf(" — ")
  if (dashIdx === -1) return { verb, tool: rest, intent: "" }
  return { verb, tool: rest.slice(0, dashIdx), intent: rest.slice(dashIdx + 3) }
}

/**
 * A rendered segment of the conversation: either a single normal message, or a
 * *run* of consecutive auto tool-activity traces collapsed into one block.
 */
export type Segment = { type: "msg"; msg: AgentMsg } | { type: "auto"; msgs: AgentMsg[] }

/**
 * Fold the flat message log into render segments, collapsing every maximal run
 * of consecutive `auto` traces into a single {@link Segment} so the live
 * tool-activity stream renders as one quiet, expandable group instead of a wall
 * of bubbles.
 */
export function segmentLog(log: AgentMsg[]): Segment[] {
  const out: Segment[] = []
  for (const m of log) {
    if (m.auto) {
      const tail = out.at(-1)
      if (tail?.type === "auto") tail.msgs.push(m)
      else out.push({ type: "auto", msgs: [m] })
    } else {
      out.push({ type: "msg", msg: m })
    }
  }
  return out
}

/**
 * Flatten markdown to a one-line plain-text snippet for a list-row preview.
 *
 * A agent row shows a single truncated line, so rendering rich markdown there
 * is wrong (headings/lists/code blocks would break the layout) — every chat
 * client shows a flattened text snippet instead. This strips the syntax that
 * would otherwise leak through as literal characters (`## `, `**bold**`, list
 * bullets, links, fenced code, stray HTML tags) and collapses all whitespace
 * to single spaces. Intentionally lightweight (a preview, not a parser): a
 * stray `_` inside an identifier is left alone rather than risk mangling words.
 */
function flattenMarkdown(md: string): string {
  return md
    .replaceAll(/```[\s\S]*?```/g, " ") // drop fenced code blocks
    .replaceAll(/!\[([^\]]*)\]\([^)]*\)/g, "$1") // image → alt text
    .replaceAll(/\[([^\]]*)\]\([^)]*\)/g, "$1") // link → label
    .replaceAll(/<[^>]+>/g, " ") // strip HTML tags
    .replaceAll(/^\s{0,3}(?:#{1,6}|[-*+>]|\d+\.)\s+/gm, "") // heading/quote/bullet markers
    .replaceAll(/\*\*|\*|__|~~|`/g, "") // emphasis / code / strike markers
    .replaceAll(/\s+/g, " ")
    .trim()
}

/** Last-message preview text for a agent row + search matching. */
export function previewOf(t: AgentDetail): string {
  // Auto tool-activity traces are collapsed noise — never surface one as the
  // row preview; show the last real message instead.
  let last: AgentDetail["log"][number] | undefined
  for (let i = t.log.length - 1; i >= 0; i--) {
    const m = t.log[i]
    if (m && !m.auto) {
      last = m
      break
    }
  }
  if (!last) return ""
  if (last.text) return flattenMarkdown(last.text)
  return last.tool ? `⛭ ${last.tool.name}` : ""
}

/** A persisted composer draft: the unsent text plus the caret/selection range
 *  to restore (T304). Stored as JSON under the composer's `draftKey`. */
export interface Draft {
  text: string
  selStart: number
  selEnd: number
}

/** Clamp `n` into `[lo, hi]`. */
function clampRange(n: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, n))
}

/**
 * Read and parse a persisted {@link Draft} from localStorage.
 *
 * Tolerant of the legacy format: early T304 drafts were stored as a bare text
 * string (no cursor). A value that isn't our `{text,selStart,selEnd}` JSON
 * object — a legacy plain string, or any non-object JSON — is treated as raw
 * text with the caret at the end, so an in-flight draft from the old format is
 * never lost on upgrade. Cursor offsets are clamped to the text length.
 */
export function parseDraft(key: string | undefined): Draft {
  const empty: Draft = { text: "", selStart: 0, selEnd: 0 }
  if (!key) return empty
  const raw = localStorage.getItem(key)
  if (raw == null) return empty
  try {
    const o: unknown = JSON.parse(raw)
    if (o && typeof o === "object" && typeof (o as Draft).text === "string") {
      // A legacy/hand-rolled draft may carry `text` without the cursor fields,
      // so read the numeric offsets through a Partial view: `selStart`/`selEnd`
      // are genuinely `number | undefined` at runtime and fall back to the text
      // end. (The full `as Draft` cast would type them as always-present.)
      const d = o as Partial<Draft> & { text: string }
      const t = d.text
      const s = clampRange(d.selStart ?? t.length, 0, t.length)
      const e = clampRange(d.selEnd ?? s, 0, t.length)
      return { text: t, selStart: s, selEnd: e }
    }
  } catch {
    // not our JSON — fall through to the legacy plain-string path
  }
  return { text: raw, selStart: raw.length, selEnd: raw.length }
}

/**
 * A agent's status-dot colour: the orange signal accent while the agent is
 * working (`MY_TURN` / `ACTIVE`), muted otherwise.
 */
export function dotColor(status: AgentDetail["status"]): string {
  if (status === "MY_TURN" || status === "ACTIVE") return "var(--signal)"
  return "var(--muted-foreground)"
}

/** Where a sidebar row's hierarchy lines run (see {@link treeGuides}). */
export interface TreeGuide {
  /** per ancestor column (0..depth-2): the line passes straight through */
  through: boolean[]
  /** a later sibling follows, so the row's own column continues below the elbow */
  more: boolean
  /** the next row is a child, so a stub runs from the dot down to it */
  kids: boolean
}

/**
 * Guide-line layout for a list already in tree order (each row's `depth`,
 * sub-agents right under their parent). A column's line continues only while
 * a later row still hangs off it; the last child therefore draws just the upper
 * half of its line, and a parent gets a stub below its dot down to its children.
 */
export function treeGuides(
  rows: { id: string; depth?: number | undefined }[],
): Map<string, TreeGuide> {
  const depthAt = (i: number) => rows[i]?.depth ?? 0
  // Does a row at exactly `d` follow `i` before the list climbs above `d`?
  const continues = (i: number, d: number) => {
    for (let j = i + 1; j < rows.length; j++) {
      const dj = depthAt(j)
      if (dj < d) return false
      if (dj === d) return true
    }
    return false
  }
  const out = new Map<string, TreeGuide>()
  for (const [i, r] of rows.entries()) {
    const d = depthAt(i)
    out.set(r.id, {
      through: Array.from({ length: Math.max(0, d - 1) }, (_, c) => continues(i, c + 1)),
      more: d > 0 && continues(i, d),
      kids: depthAt(i + 1) > d,
    })
  }
  return out
}

/**
 * Every agent row-action tooltip, in one place.
 *
 * Lives here rather than inline in the row markup for two reasons: reviewing
 * user-facing copy should not mean reading component JSX, and a sentence this
 * long inside a JSX prop forces the formatter to explode the call site across
 * six lines. Each entry names what the action DOES, not what its icon depicts.
 */
export const ROW_ACTION_COPY = {
  archive: {
    title: "Archive",
    body: "Move this agent out of the active list. You can restore it later.",
  },
  restore: { title: "Restore", body: "Move this agent back into the active list." },
  remove: {
    title: "Delete permanently",
    body: "Remove this agent and its whole conversation. This cannot be undone.",
  },
  pause: { title: "Pause", body: "Stop this agent from running. Queued messages wait." },
  resume: { title: "Resume", body: "Let this agent run again." },
  rename: { title: "Rename", body: "Change this agent's name. Enter saves, Esc cancels." },
} as const

/** The turn-status banner shown above the composer input, or null. */
export interface Banner {
  working: boolean
  paused: boolean
  color: string | undefined
  text: string
}

/**
 * Resolve the composer's turn-status banner from the agent state (T39/T371).
 *
 * A flat precedence chain (not a nested ternary): a paused agent shows the
 * amber pause notice; otherwise, only when the agent owes this agent a
 * response, an active spinner while streaming / working the FOCUSED agent, or
 * a static "will pick up soon" clock for a queued (non-focused) agent-turn
 * agent. Returns null on the user's turn (no banner).
 *
 * Lives here, beside {@link ROW_ACTION_COPY}, for the same reason: it is a pure
 * agent-state → copy derivation with no JSX in it, and the sentences it picks
 * are user-facing copy that should be reviewable without opening a component.
 */
export function resolveComposerBanner(
  paused: boolean,
  fleetBusy: boolean,
  streaming: boolean,
  focused: boolean,
): Banner | null {
  if (paused) {
    return {
      working: false,
      paused: true,
      color: undefined,
      text: "Agent paused — it won't respond until resumed.",
    }
  }
  if (!fleetBusy) return null
  if (streaming) {
    return { working: true, paused: false, color: "var(--ok)", text: "Agent is streaming…" }
  }
  if (focused) {
    return {
      working: true,
      paused: false,
      color: "var(--signal)",
      text: "Agent is working…",
    }
  }
  return {
    working: false,
    paused: false,
    color: undefined,
    text: "Agent will pick this up soon.",
  }
}
