import { useEffect, useMemo, useRef, useState } from "react"
import { lineBounds, resolveEnter, resolveTab } from "@/lib/utils"
import { ArrowUp, Paperclip, Loader2, Clock, Pause } from "lucide-react"
import type { ThreadStatus } from "@/lib/types"
import { ComposerBubbles, type UploadedFile, type CommandSuggestion } from "./fileUpload"

/** A persisted composer draft: the unsent text plus the caret/selection range
 *  to restore (T304). Stored as JSON under the composer's `draftKey`. */
interface Draft {
  text: string
  selStart: number
  selEnd: number
}

/** Clamp `n` into `[lo, hi]`. */
function clampRange(n: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, n))
}

// CommandSuggestion now lives beside the file-chip abstraction in ./fileUpload
// (both composer pill families share ONE module + ONE rendered row). Re-exported
// here for the existing `import { type CommandSuggestion } from "./ThreadComposer"`
// consumers (ThreadConversation).
export type { CommandSuggestion } from "./fileUpload"

/**
 * Read and parse a persisted {@link Draft} from localStorage.
 *
 * Tolerant of the legacy format: early T304 drafts were stored as a bare text
 * string (no cursor). A value that isn't our `{text,selStart,selEnd}` JSON
 * object — a legacy plain string, or any non-object JSON — is treated as raw
 * text with the caret at the end, so an in-flight draft from the old format is
 * never lost on upgrade. Cursor offsets are clamped to the text length.
 */
function parseDraft(key: string | undefined): Draft {
  const empty: Draft = { text: "", selStart: 0, selEnd: 0 }
  if (!key) return empty
  const raw = localStorage.getItem(key)
  if (raw == null) return empty
  try {
    const o: unknown = JSON.parse(raw)
    if (o && typeof o === "object" && typeof (o as Draft).text === "string") {
      const t = (o as Draft).text
      const s = clampRange((o as Draft).selStart ?? t.length, 0, t.length)
      const e = clampRange((o as Draft).selEnd ?? s, 0, t.length)
      return { text: t, selStart: s, selEnd: e }
    }
  } catch {
    // not our JSON — fall through to the legacy plain-string path
  }
  return { text: raw, selStart: raw.length, selEnd: raw.length }
}

/**
 * Thread composer — always active, regardless of turn status. The hint above
 * the input reflects what the agent is doing with *this* thread when it is the
 * agent's turn (`MY_TURN` / `ACTIVE`):
 *
 * - **Focused** (the one thread the agent is on right now) → an active spinner:
 *   "Agent is streaming…" while `ACTIVE`, else "Agent is working this thread…".
 * - **Not focused** (the agent owes this thread a response but is busy on
 *   another) → a static clock: "Agent will pick up this thread soon." — it's
 *   queued, not being worked this instant.
 *
 * On the user's turn (`THEIR_TURN`) no hint shows. The textarea is always
 * usable so a message can be sent at any time.
 */
export function ThreadComposer({
  status,
  focused = false,
  paused = false,
  onSend,
  onAttach,
  pendingFiles = [],
  onRemoveFile,
  draftKey,
  suggestions = [],
  firstMessage = false,
  onCreateCommand,
}: {
  status: ThreadStatus
  /** true when this is the single thread the agent is currently focused on */
  focused?: boolean
  /** true when this thread has been paused by the user (T371) */
  paused?: boolean
  onSend?: (text: string) => void
  /** upload one or more picked files into this thread (paperclip button). May
   *  be async so a caller can await it (T471); the composer itself fires and
   *  forgets. */
  onAttach?: (files: File[]) => void | Promise<void>
  /** files uploaded but not yet sent — rendered as removable chips (T331) */
  pendingFiles?: UploadedFile[]
  /** remove a staged file by its index in pendingFiles */
  onRemoveFile?: (index: number) => void
  /**
   * `/command` first-message suggestions (T348). When non-empty, each renders
   * as a clickable bubble above the textarea; clicking prefills the composer
   * with the command's literal text (the user can edit before sending).
   * Callers pass these only for an EMPTY thread — the suggestions are a
   * jumping-off point for the first message, not a persistent palette.
   */
  suggestions?: CommandSuggestion[]
  /**
   * True when the thread has no messages yet (T350). Scopes the *empty-composer*
   * auto-show of the suggestion bubbles to a first message only — on a
   * non-empty thread the bubbles still appear, but solely via the lone-`/` line
   * trigger, never just because the composer happens to be empty.
   */
  firstMessage?: boolean
  /**
   * Opens the "create command" dialog (T350). When provided, a pill styled
   * exactly like the suggestion bubbles is rendered alongside them (and shown
   * even when there are no commands yet, so the first one can be bootstrapped);
   * clicking it invokes this callback. Omit to hide the pill.
   */
  onCreateCommand?: () => void
  /**
   * localStorage key under which the UNSENT draft is persisted (T304). When
   * provided, what you type — and **where your caret is** — survives a reload,
   * a view switch, and switching threads; each thread keeps its own pending
   * draft. The composer is keyed by thread id upstream, so it remounts per
   * thread and lazily seeds its text + selection from this key; every keystroke
   * and caret move rewrites the draft, and it is cleared on send (or when the
   * draft is emptied). The stored value is `{text,selStart,selEnd}` JSON (a
   * legacy bare-string draft is still read, caret at end). Omit for an
   * ephemeral composer.
   */
  draftKey?: string
}) {
  // Seed text + caret from the persisted draft ONCE per mount so a remount
  // (thread switch / return from another view) or a full reload restores both
  // what was being typed and where the cursor sat (T304). The ref is read by
  // the mount effect below to apply the saved selection range.
  const seedRef = useRef<Draft | null>(null)
  if (seedRef.current === null) seedRef.current = parseDraft(draftKey)
  const [text, setText] = useState(() => seedRef.current?.text ?? "")
  // Caret offset, tracked so we can tell which line the user is editing — used
  // to surface the /command bubbles when the current line is exactly `/` (T350).
  const [caret, setCaret] = useState(() => seedRef.current?.selStart ?? 0)
  const textareaRef = useRef<HTMLTextAreaElement>(null)
  const fileInputRef = useRef<HTMLInputElement>(null)

  // Persist the unsent draft + caret per thread: write JSON on every keystroke
  // and caret move, and remove the key once the draft is empty (sent or
  // cleared) so we never leave stale drafts littering localStorage. Called
  // explicitly from onChange/onSelect/submit with the textarea's live
  // selection. No-op when no draftKey is supplied.
  const persistDraft = (t: string, s: number, e: number) => {
    if (!draftKey) return
    if (t) localStorage.setItem(draftKey, JSON.stringify({ text: t, selStart: s, selEnd: e }))
    else localStorage.removeItem(draftKey)
  }

  // Apply the saved caret/selection once the textarea has mounted (T304). Runs
  // a single time; `autoFocus` puts the caret at the default position, this
  // overrides it with the persisted range. Skipped when there is no draft.
  useEffect(() => {
    const el = textareaRef.current
    const seed = seedRef.current
    if (!el || !seed || !seed.text) return
    el.focus()
    el.setSelectionRange(seed.selStart, seed.selEnd)
  }, [])

  /**
   * Grow the textarea to fit its content, just like the TUI input area which
   * expands line-by-line as you type. Driven by JS (measure `scrollHeight`)
   * rather than the experimental `field-sizing` CSS so it works in every
   * browser. Capped at `MAX_H` px, beyond which it scrolls internally.
   */
  const MAX_H = 200
  const autoResize = () => {
    const el = textareaRef.current
    if (!el) return
    el.style.height = "auto"
    el.style.height = `${Math.min(el.scrollHeight, MAX_H)}px`
  }
  useEffect(autoResize, [text])



  const userTurn = status === "THEIR_TURN"
  const streaming = status === "ACTIVE"
  // The agent owes a response on this thread (its turn, or actively streaming).
  const agentBusy = !userTurn
  // Only the FOCUSED thread is being worked right now; any other agent-turn
  // thread is queued and will be picked up soon (T39).
  const banner = paused
    ? { working: false, paused: true, color: undefined, text: "Thread paused — the agent won't respond until resumed." }
    : !agentBusy
      ? null
      : streaming
        ? { working: true, paused: false, color: "var(--ok)", text: "Agent is streaming…" }
        : focused
          ? { working: true, paused: false, color: "var(--signal)", text: "Agent is working this thread…" }
          : { working: false, paused: false, color: undefined, text: "Agent will pick up this thread soon." }

  const canSend = text.trim().length > 0 || pendingFiles.length > 0

  // The line the caret sits on is exactly `/` — a lightweight in-composer
  // trigger for the /command bubbles mid-draft (T350). Recomputed on every
  // edit + caret move; the moment the line becomes anything other than `/`
  // (e.g. `/c`, text, blank) the bubbles vanish.
  const slashActive = useMemo(() => {
    const { start, end } = lineBounds(text, caret)
    return text.slice(start, end) === "/"
  }, [text, caret])

  // Whether the /command bubbles should be offered right now: mid-draft on a
  // lone `/` line (any thread), OR on a brand-new thread with an empty composer
  // (the first-message palette). File chips show independently of this.
  const commandsActive = slashActive || (firstMessage && !text.trim())

  /**
   * Prefill the composer from a suggested `/command` bubble (T348/T350). We
   * seed the command's **expanded prompt body** when it carries one (so a
   * `/boss-hunt` bubble fills with the actual prompt, not the bare token),
   * falling back to the `/command` literal otherwise. A trailing newline is
   * appended and the caret placed on that fresh blank line, so the user can
   * immediately add their own context beneath the seeded prompt. We fill rather
   * than auto-send so they can review or extend it first; the textarea is
   * refocused + regrown.
   */
  const prefill = (s: CommandSuggestion) => {
    const base = s.body && s.body.trim().length > 0 ? s.body.trimEnd() : s.command
    const seeded = `${base}\n`
    // Two seeding modes (T350):
    //  • slash trigger — the caret's line is exactly `/`: REPLACE just that `/`
    //    line with the expanded prompt, preserving any other lines, caret on the
    //    fresh blank line below it.
    //  • empty composer — seed the whole composer with the prompt.
    const { start, end } = lineBounds(text, caret)
    const onSlashLine = text.slice(start, end) === "/"
    const next = onSlashLine ? text.slice(0, start) + seeded + text.slice(end) : seeded
    const caretPos = onSlashLine ? start + seeded.length : seeded.length
    setText(next)
    setCaret(caretPos)
    persistDraft(next, caretPos, caretPos)
    requestAnimationFrame(() => {
      const el = textareaRef.current
      if (!el) return
      el.focus()
      el.setSelectionRange(caretPos, caretPos)
      autoResize()
    })
  }

  const handleSubmit = () => {
    if (!canSend || !onSend) return
    onSend(text)
    setText("")
    setCaret(0)
    persistDraft("", 0, 0)
    // Collapse back to a single row after sending (matches the TUI clearing
    // its input), then refocus for the next message.
    requestAnimationFrame(() => {
      const el = textareaRef.current
      if (el) el.style.height = "auto"
      el?.focus()
    })
  }

  /**
   * Splice a new value + caret into the textarea and React state in one shot,
   * keeping the persisted draft and auto-grow in sync. Caret is restored after
   * the controlled re-render via rAF (React resets it on value change).
   */
  const applyEdit = (value: string, caret: number) => {
    setText(value)
    setCaret(caret)
    persistDraft(value, caret, caret)
    requestAnimationFrame(() => {
      const el = textareaRef.current
      if (!el) return
      el.setSelectionRange(caret, caret)
      autoResize()
    })
  }

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    const el = e.currentTarget
    const { value, selectionStart: s, selectionEnd } = el

    // Tab / Shift+Tab indent/outdent a list item one level (T359). Only
    // hijacked on a list line — elsewhere the textarea's default Tab (focus
    // move) stands. resolveTab returns null when there's nothing to nest.
    if (e.key === "Tab" && !e.nativeEvent.isComposing) {
      const edit = resolveTab(value, s, selectionEnd, e.shiftKey)
      if (!edit) return
      e.preventDefault()
      applyEdit(edit.value, edit.caret)
      return
    }

    // Faithful port of the TUI input area (T359). `isComposing` guards an
    // in-flight IME/dead-key composition (accents, CJK candidates) — committing
    // it with Enter must never act. Shift+Enter always inserts a newline (we
    // let the browser's default handle it). A plain Enter is fully hijacked:
    // resolveEnter decides send vs a full value+caret splice (list-continue /
    // empty-item-remove / newline), mirroring src/modules/conversation/{list,
    // panel}.rs exactly.
    if (e.key !== "Enter" || e.shiftKey || e.nativeEvent.isComposing) return
    e.preventDefault()

    const action = resolveEnter(value, s, selectionEnd)
    if (action.kind === "send") handleSubmit()
    else applyEdit(action.value, action.caret)
  }

  return (
    <div className="shrink-0 px-5 pb-4 pt-2">
      {/* Unified bubble row (T350) — file-upload chips + /command suggestions +
          the create-command pill, all in ONE transparent, normal-flow container
          between the conversation and the textarea. The two pill families now
          coexist (a staged file no longer hides the slash bubbles). Commands are
          offered only in slash / first-message mode; files whenever staged. */}
      {(pendingFiles.length > 0 || commandsActive) && (
        <ComposerBubbles
          files={pendingFiles}
          onRemoveFile={onRemoveFile}
          suggestions={commandsActive ? suggestions : []}
          onPick={prefill}
          onCreateCommand={commandsActive ? onCreateCommand : undefined}
        />
      )}
      {banner && (
        <div className={`mb-2 flex items-center justify-center gap-2 rounded-xl px-3 py-1.5 text-[11.5px] ${banner.paused ? "bg-amber-500/10 text-amber-600 dark:text-amber-400" : "bg-muted/40 text-muted-foreground"}`}>
          {banner.paused ? (
            <Pause className="size-3.5" />
          ) : banner.working ? (
            <Loader2 className="size-3.5 animate-spin" style={{ color: banner.color }} />
          ) : (
            <Clock className="size-3.5" />
          )}
          <span>{banner.text}</span>
        </div>
      )}

      {/* Pending file attachments now render inside the unified ComposerBubbles
          row above (T350), alongside the /command bubbles — no separate row. */}

      <div className="flex items-end gap-2 rounded-2xl border border-border bg-card px-3 py-2.5 card-shadow focus-within:border-[var(--signal)]/60">
        <input
          ref={fileInputRef}
          type="file"
          multiple
          className="hidden"
          onChange={(e) => {
            const files = Array.from(e.target.files ?? [])
            if (files.length > 0) onAttach?.(files)
            // Reset so picking the same file again re-fires onChange.
            e.target.value = ""
          }}
        />
        <button
          onClick={() => fileInputRef.current?.click()}
          disabled={!onAttach}
          title="Attach files"
          className="mb-0.5 text-muted-foreground/60 transition-colors hover:text-[var(--interactive)] disabled:cursor-default disabled:opacity-40 disabled:hover:text-muted-foreground/60"
        >
          <Paperclip className="size-4" />
        </button>
        <textarea
          ref={textareaRef}
          autoFocus
          value={text}
          onChange={(e) => {
            const v = e.target.value
            setText(v)
            setCaret(e.target.selectionStart)
            persistDraft(v, e.target.selectionStart, e.target.selectionEnd)
          }}
          onSelect={(e) => {
            // Caret / selection moved (arrow keys, click, drag) without
            // necessarily changing the text — persist the new range too (T304).
            const el = e.currentTarget
            setCaret(el.selectionStart)
            persistDraft(el.value, el.selectionStart, el.selectionEnd)
          }}
          onKeyDown={handleKeyDown}
          onPaste={(e) => {
            const items = Array.from(e.clipboardData.items)
            const images = items
              .filter((i) => i.kind === "file" && i.type.startsWith("image/"))
              .map((i) => i.getAsFile())
              .filter((f): f is File => f !== null)
            if (images.length > 0 && onAttach) {
              e.preventDefault()
              onAttach(images)
            }
          }}
          placeholder="Reply to this thread…"
          rows={1}
          className="max-h-[200px] min-h-[24px] flex-1 resize-none bg-transparent text-[13.5px] leading-relaxed text-foreground/90 placeholder:text-muted-foreground/60 outline-none"
        />
        <button
          onClick={handleSubmit}
          disabled={!canSend}
          className="flex size-7 items-center justify-center rounded-full bg-[var(--signal)] text-[var(--primary-foreground)] transition-[filter] hover:brightness-105 disabled:opacity-40 disabled:hover:brightness-100"
        >
          <ArrowUp className="size-4" strokeWidth={2.5} />
        </button>
      </div>
    </div>
  )
}
