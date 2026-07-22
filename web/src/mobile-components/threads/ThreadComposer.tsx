import { useEffect, useMemo, useRef, useState } from "react"
import { lineBounds, resolveEnter, resolveTab } from "@/lib/utils"
import { measure } from "@/lib/support/telemetry"
import { ArrowUp, Plus, Loader2, Clock, Pause } from "lucide-react"
import type { ThreadStatus } from "@/lib/types"
import { ComposerBubbles } from "@/mobile-components/threads/fileUpload"
import type { UploadedFile, CommandSuggestion } from "@/mobile-components/threads/fileUpload/helpers"
import { parseDraft } from "@/lib/support/threadMessages"

// CommandSuggestion lives beside the file-chip abstraction in ./fileUpload (both
// composer pill families share ONE module + ONE rendered row). Re-exported for
// the mobile ThreadConversation consumer, matching the desktop twin's surface.
export type { CommandSuggestion } from "@/mobile-components/threads/fileUpload/helpers"

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
 * auto-grow textarea, and the keyboard/command-prefill handlers — identical to
 * the desktop twin (shared draft/keyboard behaviour), extracted so both units
 * stay within the P8 budgets.
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
  const [caret, setCaret] = useState(() => seed.selStart)
  const textareaRef = useRef<HTMLTextAreaElement>(null)

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

  const MAX_H = 200
  const autoResize = () => {
    const el = textareaRef.current
    if (!el) return
    measure("composer:autosize", () => {
      el.style.height = "auto"
      el.style.height = `${Math.min(el.scrollHeight, MAX_H)}px`
    })
  }
  useEffect(autoResize, [text])

  // The text typed after `/` on the current line, or null if the caret isn't on
  // a slash-prefixed line. Drives both the bubble visibility and prefix filter.
  const slashPrefix = useMemo((): string | null => {
    const { start, end } = lineBounds(text, caret)
    const line = text.slice(start, end)
    if (!line.startsWith("/")) return null
    return line.slice(1)
  }, [text, caret])

  const canSend = (pendingFiles: number) => text.trim().length > 0 || pendingFiles > 0

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

  const prefill = (s: CommandSuggestion) => {
    const base = s.body && s.body.trim().length > 0 ? s.body.trimEnd() : s.command
    const seeded = `${base}\n`
    const { start, end } = lineBounds(text, caret)
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
    requestAnimationFrame(() => {
      const el = textareaRef.current
      if (el) el.style.height = "auto"
      el?.focus()
    })
  }

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    const el = e.currentTarget
    const { value, selectionStart: s, selectionEnd } = el

    if (e.key === "Tab" && !e.nativeEvent.isComposing) {
      const edit = resolveTab(value, s, selectionEnd, e.shiftKey)
      if (!edit) return
      e.preventDefault()
      applyEdit(edit.value, edit.caret)
      return
    }

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
 * The composer's input row — the mobile-tuned twin. Same structure as desktop
 * (paperclip + auto-grow textarea + send) with touch-first sizing:
 *
 *   • **16px textarea font** — iOS Safari auto-zooms the viewport when a focused
 *     input has a font smaller than 16px; the desktop 13.5px would jank the
 *     whole layout on every tap. 16px pins the zoom off.
 *   • **Larger tap targets** — the paperclip and send button grow to 36px, and
 *     the input row gets more vertical padding, for comfortable thumb use.
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
    // iMessage-style row: a standalone round attach button on the LEFT, then a
    // single rounded pill holding the textarea with the send button tucked
    // inside its right edge — not a heavy bordered card wrapping everything.
    <div className="flex items-end gap-2">
      <input
        ref={fileInputRef}
        type="file"
        multiple
        className="hidden"
        onChange={(e) => {
          const files = [...(e.target.files ?? [])]
          if (files.length > 0) void onAttach?.(files)
          e.target.value = ""
        }}
      />
      {/* Standalone attach affordance (like iMessage's +), outside the pill. */}
      <button
        onClick={() => fileInputRef.current?.click()}
        disabled={!onAttach}
        title="Attach files"
        aria-label="Attach files"
        className="flex size-9 shrink-0 items-center justify-center rounded-full text-muted-foreground/70 transition-colors active:bg-muted active:text-(--interactive) disabled:cursor-default disabled:opacity-40"
      >
        <Plus className="size-5.5" strokeWidth={2.25} />
      </button>

      {/* The input pill — thin border, subtle fill, fully rounded so a single
          line reads as a capsule and it softens as it grows. The send button
          lives INSIDE the pill's right edge (iMessage convention). */}
      <div className="flex flex-1 items-end gap-1 rounded-[1.35rem] border border-border bg-card py-1 pr-1 pl-3.5 focus-within:border-(--signal)/60">
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
          placeholder="Message…"
          rows={1}
          className="max-h-[200px] min-h-[30px] flex-1 resize-none self-center bg-transparent py-1 text-[16px] leading-snug text-foreground/90 outline-none placeholder:text-muted-foreground/50"
        />
        {/* Send appears only when there's something to send (iMessage hides it
            on an empty field), so the empty pill stays clean. */}
        {sendable && (
          <button
            onClick={onSubmit}
            aria-label="Send message"
            className="mb-0.5 flex size-8 shrink-0 items-center justify-center rounded-full bg-(--signal) text-(--primary-foreground) transition-[filter] active:brightness-110"
          >
            <ArrowUp className="size-4.5" strokeWidth={2.75} />
          </button>
        )}
      </div>
    </div>
  )
}

/** The turn-status banner element, or null (see {@link resolveComposerBanner}). */
function ComposerBanner({ banner }: { banner: Banner }) {
  return (
    <div
      className={`mb-2 flex items-center justify-center gap-2 rounded-xl px-3 py-1.5 text-[12px] ${banner.paused ? "bg-amber-500/10 text-amber-600 dark:text-amber-400" : "bg-muted/40 text-muted-foreground"}`}
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
 * Mobile thread composer — the divergent twin of `components/threads/
 * ThreadComposer`. Always active regardless of turn status; the turn-status
 * banner reflects agent activity on this thread (T39/T371).
 *
 * Behaviour (draft persistence, list-aware Enter/Tab, `/command` bubbles with
 * Tab autocomplete + Space expansion) is byte-for-byte the desktop logic — only
 * the input row's sizing forks for touch (16px font to defeat iOS focus-zoom,
 * 36px tap targets) and the outer padding carries a `safe-area-inset-bottom`
 * so the composer clears the phone's home indicator.
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
  /** `/command` suggestions (T348). Non-empty renders clickable bubbles. */
  suggestions?: CommandSuggestion[] | undefined
  /** True when thread has no messages yet — scopes auto-show bubbles (T350). */
  firstMessage?: boolean
  /** Opens the "create command" dialog (T350). Omit to hide the pill. */
  onCreateCommand?: (() => void) | undefined
  /** localStorage key for persisting the unsent draft + caret per thread (T304). */
  draftKey?: string | undefined
}) {
  const composer = useComposer(draftKey, onSend)

  const userTurn = status === "THEIR_TURN"
  const streaming = status === "ACTIVE"
  const agentBusy = !userTurn
  const banner = resolveComposerBanner(paused, agentBusy, streaming, focused)

  const sendable = composer.canSend(pendingFiles.length)

  // Offer /command bubbles mid-draft on a slash line (any thread) OR on a
  // brand-new thread with an empty composer. File chips show independently.
  const commandsActive = composer.slashPrefix !== null || (firstMessage && !composer.text.trim())

  const filteredSuggestions = useMemo(() => {
    const prefix = composer.slashPrefix
    if (prefix === null || prefix === "") return suggestions
    const lower = prefix.toLowerCase()
    return suggestions.filter((s) => s.command.slice(1).toLowerCase().startsWith(lower))
  }, [suggestions, composer.slashPrefix])

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Tab" && !e.shiftKey && composer.slashPrefix !== null) {
      const first = filteredSuggestions[0]
      if (first) {
        e.preventDefault()
        composer.prefill(first)
        return
      }
    }

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
    <div className="shrink-0 px-3 pt-2 pb-[max(1rem,env(safe-area-inset-bottom))]">
      {/* Unified bubble row (T350) — file-upload chips + /command suggestions +
          create-command pill, all in ONE normal-flow container. */}
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
