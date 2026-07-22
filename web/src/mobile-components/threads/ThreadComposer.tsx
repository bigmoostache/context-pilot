import { useEffect, useMemo, useRef, useState } from "react"
import { animate, createSpring } from "animejs"
import { lineBounds, resolveEnter, resolveTab, prefersReducedMotion } from "@/lib/utils"
import { measure } from "@/lib/support/telemetry"
import { ArrowUp, Plus, Loader2, Clock, Pause } from "lucide-react"
import type { ThreadStatus } from "@/lib/types"
import { ComposerBubbles } from "@/mobile-components/threads/fileUpload"
import type { UploadedFile, CommandSuggestion } from "@/mobile-components/threads/fileUpload/helpers"
import { FrostedBottomBar } from "@/mobile-components/shell/FrostedBottomBar"
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
 * Resolve the composer's turn-status banner from thread state (T39/T371). Flat
 * precedence: paused shows the amber pause notice; else only when the agent owes
 * this thread — an active spinner while streaming / working the FOCUSED thread,
 * or a static "will pick up soon" clock for a queued (non-focused) agent turn.
 * Null on the user's turn.
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

/** Everything the composer render needs from its draft/keyboard logic. Flat (not
 *  nested under one object) so the render passes `textareaRef` to `ref=` as a
 *  bare identifier — the react-hooks/refs pass rejects reading a ref through a
 *  member access of a ref-bearing object. */
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
 * desktop (shared behaviour), extracted to keep both units within P8 budgets.
 */
function useComposer(
  draftKey: string | undefined,
  onSend: ((text: string) => void) | undefined,
): Composer {
  // Seed text + caret from the persisted draft ONCE per mount so a remount
  // (thread switch / return) or a full reload restores what was being typed
  // and where the cursor sat (T304).
  const [seed] = useState(() => parseDraft(draftKey))
  const [text, setText] = useState(() => seed.text)
  const [caret, setCaret] = useState(() => seed.selStart)
  const textareaRef = useRef<HTMLTextAreaElement>(null)

  const persistDraft = (t: string, s: number, e: number) => {
    if (!draftKey) return
    if (t) localStorage.setItem(draftKey, JSON.stringify({ text: t, selStart: s, selEnd: e }))
    else localStorage.removeItem(draftKey)
  }

  // Restore the saved caret/selection once the textarea mounts (T304); do NOT
  // focus — focusing on thread-open pops the mobile keyboard unbidden (T622).
  // Post-user-action focus (send, /command pick) is handled elsewhere.
  useEffect(() => {
    const el = textareaRef.current
    if (!el || !seed.text) return
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

  // Text typed after `/` on the current line, or null when not on a slash line.
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
 * The composer's input row — mobile-tuned twin. Same structure as desktop
 * (paperclip + auto-grow textarea + send) with touch-first sizing: 16px textarea
 * font (below 16px iOS Safari auto-zooms the viewport on focus), and 36px tap
 * targets for the paperclip + send.
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
  // #4 Send-button pop (anime.js): spring the send button in when it becomes
  // available (iMessage pops it, not a hard cut). Conditionally rendered, so
  // this fires when `sendable` flips true. Reduced-motion skips it.
  const sendBtnRef = useRef<HTMLButtonElement>(null)
  useEffect(() => {
    const btn = sendBtnRef.current
    if (!btn || !sendable || prefersReducedMotion()) return
    animate(btn, {
      scale: [0, 1],
      opacity: [0, 1],
      ease: createSpring({ stiffness: 600, damping: 20 }),
    })
  }, [sendable])
  return (
    // iMessage-style row: standalone round attach button on the LEFT, then a
    // single rounded pill holding the textarea with send tucked inside its edge.
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
        className="flex size-9 shrink-0 items-center justify-center rounded-full bg-card/60 text-muted-foreground/70 backdrop-blur-[3px] transition-colors active:bg-muted active:text-(--interactive) disabled:cursor-default disabled:opacity-40"
      >
        <Plus className="size-5.5" strokeWidth={2.25} />
      </button>

      {/* The input pill — thin border, subtle fill, fully rounded. Send button
          lives INSIDE the pill's right edge (iMessage convention). */}
      <div className="flex min-w-0 flex-1 items-end gap-1 rounded-[1.35rem] border border-border bg-card/70 py-1 pr-1 pl-3.5 backdrop-blur-[3px] focus-within:border-(--signal)/60">
        <textarea
          ref={textareaRef}
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
          className="max-h-[200px] min-h-[30px] min-w-0 flex-1 resize-none self-center bg-transparent py-1 text-[16px] leading-snug text-foreground/90 outline-none placeholder:text-muted-foreground/50"
        />
        {/* Send appears only when there's something to send (empty pill stays
            clean). */}
        {sendable && (
          <button
            ref={sendBtnRef}
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
 * Mobile thread composer — divergent twin of `components/threads/ThreadComposer`.
 * Always active; the turn-status banner reflects agent activity (T39/T371).
 * Behaviour (draft persistence, list-aware Enter/Tab, `/command` bubbles with Tab
 * autocomplete + Space expansion) is byte-for-byte the desktop logic — only the
 * input row's touch sizing forks (16px font vs iOS focus-zoom, 36px targets) and
 * the outer padding carries `safe-area-inset-bottom` to clear the home indicator.
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

  // /command bubbles on a slash line, or on a brand-new empty thread.
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
    <FrostedBottomBar className="px-3 pt-3 pb-[max(1rem,env(safe-area-inset-bottom))]">
      {/* Unified bubble row (T350) — file chips + /command suggestions +
          create-command pill in ONE container. */}
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
    </FrostedBottomBar>
  )
}
