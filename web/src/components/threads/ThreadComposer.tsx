import { useEffect, useRef, useState } from "react"
import { ArrowUp, Paperclip, Loader2, Clock, X } from "lucide-react"
import type { ThreadStatus } from "@/lib/types"
import type { UploadedFile } from "./fileUpload"
import { FileUploadChip } from "./fileUpload"

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
  onSend,
  onAttach,
  pendingFiles = [],
  onRemoveFile,
  draftKey,
}: {
  status: ThreadStatus
  /** true when this is the single thread the agent is currently focused on */
  focused?: boolean
  onSend?: (text: string) => void
  /** upload one or more picked files into this thread (paperclip button) */
  onAttach?: (files: File[]) => void
  /** files uploaded but not yet sent — rendered as removable chips (T331) */
  pendingFiles?: UploadedFile[]
  /** remove a staged file by its index in pendingFiles */
  onRemoveFile?: (index: number) => void
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
  const banner = !agentBusy
    ? null
    : streaming
      ? { working: true, color: "var(--ok)", text: "Agent is streaming…" }
      : focused
        ? { working: true, color: "var(--signal)", text: "Agent is working this thread…" }
        : { working: false, color: undefined, text: "Agent will pick up this thread soon." }

  const canSend = text.trim().length > 0 || pendingFiles.length > 0

  const handleSubmit = () => {
    if (!canSend || !onSend) return
    onSend(text)
    setText("")
    persistDraft("", 0, 0)
    // Collapse back to a single row after sending (matches the TUI clearing
    // its input), then refocus for the next message.
    requestAnimationFrame(() => {
      const el = textareaRef.current
      if (el) el.style.height = "auto"
      el?.focus()
    })
  }

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    // Enter sends, Shift+Enter inserts a newline — matching the TUI input.
    // `isComposing` guards an in-flight IME/dead-key composition (e.g. accents,
    // CJK candidates): committing the composition with Enter must NOT fire a
    // send. We read it off the native event because React's synthetic event
    // doesn't surface `isComposing`.
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault()
      handleSubmit()
    }
  }

  return (
    <div className="shrink-0 px-5 pb-4 pt-2">
      {banner && (
        <div className="mb-2 flex items-center justify-center gap-2 rounded-xl bg-muted/40 px-3 py-1.5 text-[11.5px] text-muted-foreground">
          {banner.working ? (
            <Loader2 className="size-3.5 animate-spin" style={{ color: banner.color }} />
          ) : (
            <Clock className="size-3.5" />
          )}
          <span>{banner.text}</span>
        </div>
      )}

      {/* Pending file attachments — uploaded but not yet sent (T331). Shown as
          removable chips so the user can review / discard before sending. */}
      {pendingFiles.length > 0 && (
        <div className="mb-2 flex flex-wrap gap-1.5">
          {pendingFiles.map((f, i) => (
            <span key={`${f.path}-${i}`} className="inline-flex items-center gap-1">
              <FileUploadChip file={f} size={f.size} />
              <button
                onClick={() => onRemoveFile?.(i)}
                className="flex size-4 items-center justify-center rounded-full bg-muted text-muted-foreground/70 transition-colors hover:bg-destructive/20 hover:text-destructive"
                title="Remove attachment"
              >
                <X className="size-2.5" strokeWidth={3} />
              </button>
            </span>
          ))}
        </div>
      )}
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
            persistDraft(v, e.target.selectionStart, e.target.selectionEnd)
          }}
          onSelect={(e) => {
            // Caret / selection moved (arrow keys, click, drag) without
            // necessarily changing the text — persist the new range too (T304).
            const el = e.currentTarget
            persistDraft(el.value, el.selectionStart, el.selectionEnd)
          }}
          onKeyDown={handleKeyDown}
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
