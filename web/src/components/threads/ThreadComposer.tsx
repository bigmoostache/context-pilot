import { useEffect, useMemo, useRef, useState } from "react"
import { lineBounds, resolveEnter, resolveTab } from "@/lib/utils"
import { measure } from "@/lib/support/telemetry"
import { ArrowUp, Paperclip, Loader2, Clock, Pause } from "lucide-react"
import type { ThreadStatus } from "@/lib/types"
import { ComposerBubbles } from "./fileUpload"
import type { UploadedFile, CommandSuggestion } from "./fileUpload/helpers"
import { parseDraft } from "@/lib/support/threadMessages"

// CommandSuggestion now lives beside the file-chip abstraction in ./fileUpload
// (both composer pill families share ONE module + ONE rendered row). Re-exported
// here for the existing `import { type CommandSuggestion } from "./ThreadComposer"`
// consumers (ThreadConversation).
export type { CommandSuggestion } from "./fileUpload/helpers"

/** The turn-status banner shown above the composer input, or null. */
interface Banner {
  working: boolean
  paused: boolean
  color: string | undefined
  text: string
}

/**
 * Resolve the composer's turn-status banner from the thread state (T39/T371).
 *
 * A flat precedence chain (not a nested ternary): a paused thread shows the
 * amber pause notice; otherwise, only when the agent owes this thread a
 * response, an active spinner while streaming / working the FOCUSED thread, or
 * a static "will pick up soon" clock for a queued (non-focused) agent-turn
 * thread. Returns null on the user's turn (no banner).
 */
function resolveComposerBanner(
  paused: boolean,
  agentBusy: boolean,
  streaming: boolean,
  focused: boolean,
): Banner | null {
  if (paused) {
    return {
      working: false,
      paused: true,
      color: undefined,
      text: "Thread paused — the agent won't respond until resumed.",
    }
  }
  if (!agentBusy) return null
  if (streaming) {
    return { working: true, paused: false, color: "var(--ok)", text: "Agent is streaming…" }
  }
  if (focused) {
    return {
      working: true,
      paused: false,
      color: "var(--signal)",
      text: "Agent is working this thread…",
    }
  }
  return {
    working: false,
    paused: false,
    color: undefined,
    text: "Agent will pick up this thread soon.",
  }
}

/** Everything the composer render needs from its draft/keyboard logic. Flat
 *  (not nested under one object) so the render passes `textareaRef` to `ref=`
 *  as a bare identifier — the react-hooks/refs pass rejects reading a ref
 *  through a member access of a ref-bearing object. */
interface Composer {
  text: string
  caret: number
  textareaRef: React.RefObject<HTMLTextAreaElement | null>
  slashPrefix: string | null
  canSend: (pendingFiles: number) => boolean
  onChange: (e: React.ChangeEvent<HTMLTextAreaElement>) => void
  onSelect: (e: React.SyntheticEvent<HTMLTextAreaElement>) => void
  handleKeyDown: (e: React.KeyboardEvent<HTMLTextAreaElement>) => void
  handleSubmit: () => void
  prefill: (s: CommandSuggestion) => void
}

/**
 * Own the composer's draft text + caret, the persisted-draft round-trip, the
 * auto-grow textarea, and the keyboard/command-prefill handlers — extracted
 * from {@link ThreadComposer} so both units stay within the P8 budgets.
 *
 * The draft (text + caret) is seeded ONCE per mount from `draftKey` (a lazy
 * `useState` initializer — a stable value, not a ref written during render),
 * persisted per thread on every edit/caret move, and cleared on send. `onSend`
 * is invoked by a plain Enter that {@link resolveEnter} classifies as a send.
 */
function useComposer(
  draftKey: string | undefined,
  onSend: ((text: string) => void) | undefined,
): Composer {
  // Seed text + caret from the persisted draft ONCE per mount so a remount
  // (thread switch / return from another view) or a full reload restores both
  // what was being typed and where the cursor sat (T304).
  const [seed] = useState(() => parseDraft(draftKey))
  const [text, setText] = useState(() => seed.text)
  // Caret offset, tracked so we can tell which line the user is editing — used
  // to surface the /command bubbles when the current line is exactly `/` (T350).
  const [caret, setCaret] = useState(() => seed.selStart)
  const textareaRef = useRef<HTMLTextAreaElement>(null)

  // Persist the unsent draft + caret per thread: write JSON on every keystroke
  // and caret move, and remove the key once the draft is empty (sent or
  // cleared) so we never leave stale drafts littering localStorage.
  const persistDraft = (t: string, s: number, e: number) => {
    if (!draftKey) return
    if (t) localStorage.setItem(draftKey, JSON.stringify({ text: t, selStart: s, selEnd: e }))
    else localStorage.removeItem(draftKey)
  }

  // Apply the saved caret/selection once the textarea has mounted (T304).
  useEffect(() => {
    const el = textareaRef.current
    if (!el || !seed.text) return
    el.focus()
    el.setSelectionRange(seed.selStart, seed.selEnd)
  }, [seed])

  /**
   * Grow the textarea to fit its content, like the TUI input area. Driven by JS
   * (measure `scrollHeight`) rather than the experimental `field-sizing` CSS so
   * it works everywhere; capped at `MAX_H` px, beyond which it scrolls.
   */
  const MAX_H = 200
  const autoResize = () => {
    const el = textareaRef.current
    if (!el) return
    // Reading `scrollHeight` forces a synchronous reflow — instrument it so a
    // stall triggered by textarea autosize is named.
    measure("composer:autosize", () => {
      el.style.height = "auto"
      el.style.height = `${Math.min(el.scrollHeight, MAX_H)}px`
    })
  }
  useEffect(autoResize, [text])

  // The text typed after `/` on the current line, or null if the caret isn't
  // on a slash-prefixed line. `""` = bare `/`, `"bo"` = `/bo`. Drives both the
  // bubble visibility and the prefix filter (T556).
  const slashPrefix = useMemo((): string | null => {
    const { start, end } = lineBounds(text, caret)
    const line = text.slice(start, end)
    if (!line.startsWith("/")) return null
    return line.slice(1)
  }, [text, caret])

  const canSend = (pendingFiles: number) => text.trim().length > 0 || pendingFiles > 0

  /**
   * Splice a new value + caret into the textarea and React state in one shot,
   * keeping the persisted draft and auto-grow in sync. Caret is restored after
   * the controlled re-render via rAF (React resets it on value change).
   */
  const applyEdit = (value: string, next: number) => {
    setText(value)
    setCaret(next)
    persistDraft(value, next, next)
    requestAnimationFrame(() => {
      const el = textareaRef.current
      if (!el) return
      el.setSelectionRange(next, next)
      autoResize()
    })
  }

  /**
   * Prefill the composer from a suggested `/command` bubble (T348/T350). Seeds
   * the command's **expanded prompt body** when it carries one (falling back to
   * the `/command` literal), with a trailing newline and the caret on the fresh
   * blank line so the user can add context. Two modes: on a lone `/` line,
   * REPLACE just that line; otherwise seed the whole composer.
   */
  const prefill = (s: CommandSuggestion) => {
    const base = s.body && s.body.trim().length > 0 ? s.body.trimEnd() : s.command
    const seeded = `${base}\n`
    const { start, end } = lineBounds(text, caret)
    // Two modes: on a slash-prefixed line (bare `/` or partial like `/bo`),
    // REPLACE just that line; otherwise seed the whole composer.
    const onSlashLine = text.slice(start, end).startsWith("/")
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
    if (text.trim().length === 0 || !onSend) return
    onSend(text)
    setText("")
    setCaret(0)
    persistDraft("", 0, 0)
    // Collapse back to a single row after sending, then refocus.
    requestAnimationFrame(() => {
      const el = textareaRef.current
      if (el) el.style.height = "auto"
      el?.focus()
    })
  }

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    const el = e.currentTarget
    const { value, selectionStart: s, selectionEnd } = el

    // Tab / Shift+Tab indent/outdent a list item one level (T359). Only
    // hijacked on a list line — elsewhere the textarea's default Tab stands.
    if (e.key === "Tab" && !e.nativeEvent.isComposing) {
      const edit = resolveTab(value, s, selectionEnd, e.shiftKey)
      if (!edit) return
      e.preventDefault()
      applyEdit(edit.value, edit.caret)
      return
    }

    // Faithful port of the TUI input area (T359). `isComposing` guards an
    // in-flight IME/dead-key composition. Shift+Enter inserts a newline
    // (browser default). A plain Enter is fully hijacked: resolveEnter decides
    // send vs a value+caret splice (list-continue / empty-item-remove).
    if (e.key !== "Enter" || e.shiftKey || e.nativeEvent.isComposing) return
    e.preventDefault()

    const action = resolveEnter(value, s, selectionEnd)
    if (action.kind === "send") handleSubmit()
    else applyEdit(action.value, action.caret)
  }

  const onChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
    const v = e.target.value
    setText(v)
    setCaret(e.target.selectionStart)
    persistDraft(v, e.target.selectionStart, e.target.selectionEnd)
  }

  const onSelect = (e: React.SyntheticEvent<HTMLTextAreaElement>) => {
    // Caret / selection moved (arrow keys, click, drag) without necessarily
    // changing the text — persist the new range too (T304).
    const el = e.currentTarget
    setCaret(el.selectionStart)
    persistDraft(el.value, el.selectionStart, el.selectionEnd)
  }

  return {
    text,
    caret,
    textareaRef,
    slashPrefix,
    canSend,
    onChange,
    onSelect,
    handleKeyDown,
    handleSubmit,
    prefill,
  }
}

/**
 * The composer's input row: the file-picker + paperclip, the auto-growing
 * textarea, and the send button. Extracted from {@link ThreadComposer} so the
 * outer component stays within the P8 complexity budget; owns its own hidden
 * file-input ref. Receives the textarea's ref/value/handlers from the parent's
 * {@link useComposer} hook and passes `ref={textareaRef}` as a bare identifier
 * (the react-hooks/refs pass allows that but rejects a member-access read).
 */
function ComposerInputRow({
  textareaRef,
  text,
  sendable,
  onChange,
  onSelect,
  onKeyDown,
  onSubmit,
  onAttach,
}: {
  textareaRef: React.RefObject<HTMLTextAreaElement | null>
  text: string
  sendable: boolean
  onChange: (e: React.ChangeEvent<HTMLTextAreaElement>) => void
  onSelect: (e: React.SyntheticEvent<HTMLTextAreaElement>) => void
  onKeyDown: (e: React.KeyboardEvent<HTMLTextAreaElement>) => void
  onSubmit: () => void
  onAttach: ((files: File[]) => void | Promise<void>) | undefined
}) {
  const fileInputRef = useRef<HTMLInputElement>(null)
  return (
    <div className="card-shadow flex items-end gap-2 rounded-2xl border border-border bg-card px-3 py-2.5 focus-within:border-(--signal)/60">
      <input
        ref={fileInputRef}
        type="file"
        multiple
        className="hidden"
        onChange={(e) => {
          const files = [...(e.target.files ?? [])]
          if (files.length > 0) void onAttach?.(files)
          // Reset so picking the same file again re-fires onChange.
          e.target.value = ""
        }}
      />
      <button
        onClick={() => fileInputRef.current?.click()}
        disabled={!onAttach}
        title="Attach files"
        className="mb-0.5 text-muted-foreground/60 transition-colors hover:text-(--interactive) disabled:cursor-default disabled:opacity-40 disabled:hover:text-muted-foreground/60"
      >
        <Paperclip className="size-4" />
      </button>
      <textarea
        ref={textareaRef}
        autoFocus
        value={text}
        onChange={onChange}
        onSelect={onSelect}
        onKeyDown={onKeyDown}
        onPaste={(e) => {
          const items = [...e.clipboardData.items]
          const images = items
            .filter((i) => i.kind === "file" && i.type.startsWith("image/"))
            .map((i) => i.getAsFile())
            .filter((f): f is File => f !== null)
          if (images.length > 0 && onAttach) {
            e.preventDefault()
            void onAttach(images)
          }
        }}
        placeholder="Reply to this thread…"
        rows={1}
        className="max-h-[200px] min-h-[24px] flex-1 resize-none bg-transparent text-[13.5px] leading-relaxed text-foreground/90 outline-none placeholder:text-muted-foreground/60"
      />
      <button
        onClick={onSubmit}
        disabled={!sendable}
        className="flex size-7 items-center justify-center rounded-full bg-(--signal) text-(--primary-foreground) transition-[filter] hover:brightness-105 disabled:opacity-40 disabled:hover:brightness-100"
      >
        <ArrowUp className="size-4" strokeWidth={2.5} />
      </button>
    </div>
  )
}

/** The turn-status banner element, or null (see {@link resolveComposerBanner}). */
function ComposerBanner({ banner }: { banner: Banner }) {
  return (
    <div
      className={`mb-2 flex items-center justify-center gap-2 rounded-xl px-3 py-1.5 text-[11.5px] ${banner.paused ? "bg-amber-500/10 text-amber-600 dark:text-amber-400" : "bg-muted/40 text-muted-foreground"}`}
    >
      {banner.paused ? (
        <Pause className="size-3.5" />
      ) : banner.working ? (
        <Loader2 className="size-3.5 animate-spin" style={{ color: banner.color }} />
      ) : (
        <Clock className="size-3.5" />
      )}
      <span>{banner.text}</span>
    </div>
  )
}

/**
 * Thread composer — always active, regardless of turn status. Turn-status
 * banner reflects agent activity on this thread (T39/T371). Structure (P8):
 * draft logic in {@link useComposer}, banner in {@link ComposerBanner},
 * input row in {@link ComposerInputRow}. T556: prefix-filtered `/command`
 * bubbles with Tab autocomplete and Space expansion.
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
  focused?: boolean | undefined
  /** true when this thread has been paused by the user (T371) */
  paused?: boolean | undefined
  onSend?: ((text: string) => void) | undefined
  /** Upload picked files into this thread (paperclip). May be async (T471). */
  onAttach?: ((files: File[]) => void | Promise<void>) | undefined
  /** Files uploaded but not yet sent — rendered as removable chips (T331). */
  pendingFiles?: UploadedFile[] | undefined
  /** Remove a staged file by its index in pendingFiles. */
  onRemoveFile?: ((index: number) => void) | undefined
  /** `/command` suggestions (T348). Non-empty renders clickable bubbles; click prefills. */
  suggestions?: CommandSuggestion[] | undefined
  /** True when thread has no messages yet — scopes auto-show bubbles to first message (T350). */
  firstMessage?: boolean
  /** Opens the "create command" dialog (T350). Omit to hide the pill. */
  onCreateCommand?: (() => void) | undefined
  /** localStorage key for persisting the unsent draft + caret per thread (T304).
   *  Stored as `{text,selStart,selEnd}` JSON; legacy bare-string also read. */
  draftKey?: string | undefined
}) {
  const composer = useComposer(draftKey, onSend)

  const userTurn = status === "THEIR_TURN"
  const streaming = status === "ACTIVE"
  // The agent owes a response on this thread (its turn, or actively streaming).
  const agentBusy = !userTurn
  const banner = resolveComposerBanner(paused, agentBusy, streaming, focused)

  const sendable = composer.canSend(pendingFiles.length)

  // Whether the /command bubbles should be offered right now: mid-draft on a
  // slash-prefixed line (any thread), OR on a brand-new thread with an empty
  // composer (the first-message palette). File chips show independently of this.
  const commandsActive = composer.slashPrefix !== null || (firstMessage && !composer.text.trim())

  // Filter suggestions by the typed prefix (T556). `/bo` shows only commands
  // starting with `/bo`. A bare `/` (prefix "") shows all commands.
  const filteredSuggestions = useMemo(() => {
    const prefix = composer.slashPrefix
    if (prefix === null || prefix === "") return suggestions
    const lower = prefix.toLowerCase()
    return suggestions.filter((s) => s.command.slice(1).toLowerCase().startsWith(lower))
  }, [suggestions, composer.slashPrefix])

  // Wrap the composer's keydown handler to intercept Tab (autocomplete first
  // filtered suggestion) and Space (expand an exact command match) before the
  // base handler runs (T556).
  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    // Tab autocomplete: if slash bubbles are showing and there are matches,
    // pick the first one instead of indenting.
    if (e.key === "Tab" && !e.shiftKey && composer.slashPrefix !== null) {
      const first = filteredSuggestions[0]
      if (first) {
        e.preventDefault()
        composer.prefill(first)
        return
      }
    }

    // Space expansion: if the current line is exactly a known command (e.g.
    // `/boss-hunt`), pressing Space replaces it with the command's body.
    if (e.key === " " && composer.slashPrefix !== null) {
      const match = suggestions.find(
        (s) => s.command.slice(1).toLowerCase() === composer.slashPrefix?.toLowerCase(),
      )
      if (match) {
        e.preventDefault()
        composer.prefill(match)
        return
      }
    }

    composer.handleKeyDown(e)
  }

  return (
    <div className="shrink-0 px-5 pt-2 pb-4">
      {/* Unified bubble row (T350) — file-upload chips + /command suggestions +
          the create-command pill, all in ONE transparent, normal-flow container
          between the conversation and the textarea. */}
      {(pendingFiles.length > 0 || commandsActive) && (
        <ComposerBubbles
          files={pendingFiles}
          onRemoveFile={onRemoveFile}
          suggestions={commandsActive ? filteredSuggestions : []}
          onPick={composer.prefill}
          onCreateCommand={commandsActive ? onCreateCommand : undefined}
        />
      )}
      {banner && <ComposerBanner banner={banner} />}
      <ComposerInputRow
        textareaRef={composer.textareaRef}
        text={composer.text}
        sendable={sendable}
        onChange={composer.onChange}
        onSelect={composer.onSelect}
        onKeyDown={handleKeyDown}
        onSubmit={composer.handleSubmit}
        onAttach={onAttach}
      />
    </div>
  )
}
