import type { ChatMessage, ThreadDetail, ThreadMsg } from "@/lib/types"

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
 * Normalise a thread message's `ts` into a human-readable relative age.
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

/** Map a thread message onto the shared ChatMessage shape for the renderer. */
export function toChatMessage(m: ThreadMsg): ChatMessage {
  return {
    id: m.id,
    role: m.tool ? "tool" : m.author,
    text: m.text,
    tool: m.tool,
    ts: formatTs(m.ts),
    streaming: m.streaming,
  }
}

/** Parse an auto-trace message into its three columns: verb, tool, intent. */
export function parseAutoLine(m: ThreadMsg): { verb: string; tool: string; intent: string } {
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
export type Segment = { type: "msg"; msg: ThreadMsg } | { type: "auto"; msgs: ThreadMsg[] }

/**
 * Fold the flat message log into render segments, collapsing every maximal run
 * of consecutive `auto` traces into a single {@link Segment} so the live
 * tool-activity stream renders as one quiet, expandable group instead of a wall
 * of bubbles.
 */
export function segmentLog(log: ThreadMsg[]): Segment[] {
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
 * A thread row shows a single truncated line, so rendering rich markdown there
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

/** Last-message preview text for a thread row + search matching. */
export function previewOf(t: ThreadDetail): string {
  // Auto tool-activity traces are collapsed noise — never surface one as the
  // row preview; show the last real message instead.
  let last: ThreadDetail["log"][number] | undefined
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
