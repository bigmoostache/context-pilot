import { createElement, type ReactElement } from "react"
import {
  Activity,
  AlignLeft,
  Archive,
  ArrowUp,
  BookOpen,
  Bot,
  Boxes,
  Braces,
  Bug,
  CheckCircle,
  Clock,
  Cog,
  Copy,
  Cpu,
  Database,
  Download,
  Eye,
  FileEdit,
  Filter,
  FlaskConical,
  FolderOpen,
  FolderPlus,
  GitBranch,
  GitCommit,
  GitMerge,
  Globe,
  Hammer,
  Hash,
  Image,
  Key,
  Layers,
  ListChecks,
  Lock,
  MapPin,
  MessageSquare,
  Move,
  Network,
  Package,
  Pencil,
  Play,
  RefreshCw,
  Rocket,
  Save,
  Search,
  Send,
  Settings,
  ShieldCheck,
  Sparkles,
  Tag,
  Trash2,
  Unlock,
  Upload,
  Wand2,
  Waypoints,
  Wrench,
  Zap,
  type LucideIcon,
} from "lucide-react"
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

// ── verb → icon (T584) ────────────────────────────────────────────────
//
// Each auto tool-activity trace carries a one-word gerund `verb` (e.g.
// "Reading", "Committing") minted by the agent for the call. The thread
// timeline renders a semantically-matched icon in place of the tool name. This
// is pure presentation (like a status→colour map), so it lives frontend-side —
// no model, no embeddings: a curated dictionary keyed on the lowercased verb,
// with a Levenshtein nearest-key fallback for any verb outside the table.
//
// Keys are lowercase; lookup lowercases the incoming verb. Values are
// `lucide-react` icon components (already the app's icon library — zero new
// dependency). Kept next to `parseAutoLine` (which extracts the verb) so the
// parse-then-decorate pair stays co-located and shared by both UI twins.

const VERB_ICONS: Record<string, LucideIcon> = {
  // read / inspect
  reading: BookOpen,
  opening: FolderOpen,
  inspecting: Eye,
  checking: CheckCircle,
  verifying: ShieldCheck,
  comparing: Layers,
  locating: MapPin,
  counting: Hash,
  tracing: Waypoints,
  reviewing: Eye,
  // search
  searching: Search,
  finding: Search,
  filtering: Filter,
  // edit / write
  fixing: Wrench,
  editing: FileEdit,
  writing: Pencil,
  formatting: AlignLeft,
  silencing: Lock,
  creating: FolderPlus,
  renaming: Tag,
  moving: Move,
  deleting: Trash2,
  // git
  staging: Package,
  committing: GitCommit,
  pushing: ArrowUp,
  pulling: Download,
  merging: GitMerge,
  branching: GitBranch,
  basing: GitBranch,
  fetching: Download,
  stashing: Archive,
  dropping: Trash2,
  // run / build
  running: Play,
  building: Hammer,
  compiling: Cog,
  linting: Bug,
  testing: FlaskConical,
  waiting: Clock,
  flushing: Zap,
  starting: Play,
  // think / plan
  thinking: Sparkles,
  planning: ListChecks,
  scoping: ListChecks,
  designing: Wand2,
  // data / net
  querying: Database,
  saving: Save,
  copying: Copy,
  uploading: Upload,
  downloading: Download,
  importing: Download,
  exporting: Upload,
  // web
  browsing: Globe,
  scraping: Globe,
  crawling: Network,
  // comms
  reporting: Send,
  sending: Send,
  messaging: MessageSquare,
  // misc
  configuring: Settings,
  unlocking: Unlock,
  authorizing: Key,
  rendering: Image,
  parsing: Braces,
  refreshing: RefreshCw,
  spawning: Cpu,
  launching: Rocket,
  scaffolding: Boxes,
  monitoring: Activity,
}

/** Fallback icon when a verb has no dictionary entry and no close neighbour. */
const DEFAULT_VERB_ICON: LucideIcon = Bot

/** A verb that is fewer than this many edits from a known key still maps to it;
 *  beyond it the guess is too weak to trust, so we fall back to the generic
 *  icon rather than show a misleading one. */
const MAX_FUZZY_DISTANCE = 3

/**
 * Levenshtein edit distance between two strings (classic two-row DP, O(a·b)
 * time, O(b) space). Used only for the ≤~65-entry verb table on a single short
 * word, so the cost is negligible; kept dependency-free.
 */
function levenshtein(a: string, b: string): number {
  if (a === b) return 0
  if (a.length === 0) return b.length
  if (b.length === 0) return a.length
  let prev = Array.from({ length: b.length + 1 }, (_, i) => i)
  let curr = Array.from<number>({ length: b.length + 1 })
  for (let i = 1; i <= a.length; i++) {
    curr[0] = i
    for (let j = 1; j <= b.length; j++) {
      const cost = a[i - 1] === b[j - 1] ? 0 : 1
      curr[j] = Math.min(
        (prev[j] ?? 0) + 1, // deletion
        (curr[j - 1] ?? 0) + 1, // insertion
        (prev[j - 1] ?? 0) + cost, // substitution
      )
    }
    ;[prev, curr] = [curr, prev]
  }
  return prev[b.length] ?? 0
}

/**
 * Resolve a trace `verb` to its display icon: an exact dictionary hit when the
 * verb is known, else the nearest key within {@link MAX_FUZZY_DISTANCE} edits
 * (so "Committed"→"committing" style near-misses still land), else the generic
 * fallback. Case-insensitive.
 */
export function iconForVerb(verb: string): LucideIcon {
  const key = verb.trim().toLowerCase()
  if (!key) return DEFAULT_VERB_ICON
  const exact = VERB_ICONS[key]
  if (exact) return exact
  let best: LucideIcon = DEFAULT_VERB_ICON
  let bestDist = MAX_FUZZY_DISTANCE + 1
  for (const [dictKey, icon] of Object.entries(VERB_ICONS)) {
    const dist = levenshtein(key, dictKey)
    if (dist < bestDist) {
      bestDist = dist
      best = icon
    }
  }
  return bestDist <= MAX_FUZZY_DISTANCE ? best : DEFAULT_VERB_ICON
}

/**
 * Render the icon for a trace `verb` as a stable, module-scope component.
 *
 * Wraps {@link iconForVerb} so callers never bind its result to a capitalized
 * local inside their own render (which `@eslint-react/static-components` reads
 * as creating a component during render). Uses `createElement` rather than JSX
 * so this stays a plain `.ts` module. `aria-label` carries the verb for screen
 * readers now that the visible tool name is gone.
 */
export function VerbIcon({ verb, className }: { verb: string; className?: string }): ReactElement {
  return createElement(iconForVerb(verb), { className, "aria-label": verb })
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
