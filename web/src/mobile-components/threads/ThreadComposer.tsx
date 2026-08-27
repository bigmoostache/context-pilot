import { useEffect, useMemo, useRef, useState } from "react"
import { lineBounds, resolveEnter, resolveTab } from "@/lib/utils"
import { measure } from "@/lib/support/telemetry"
import type { ThreadStatus } from "@/lib/types"
import { ComposerBubbles } from "@/mobile-components/threads/fileUpload"
import type {
  UploadedFile,
  CommandSuggestion,
} from "@/mobile-components/threads/fileUpload/helpers"
import {
  ComposerInputRow,
  ComposerBanner,
  type Banner,
} from "@/mobile-components/threads/fileUpload/composerInput"
import { FrostedBottomBar } from "@/mobile-components/shell/chrome/FrostedBottomBar"
import { parseDraft } from "@/lib/support/threadMessages"

// CommandSuggestion lives beside the file-chip abstraction in ./fileUpload (both
// composer pill families share ONE module + ONE rendered row). Re-exported for
// the mobile ThreadConversation consumer, matching the desktop twin's surface.
export type { CommandSuggestion } from "@/mobile-components/threads/fileUpload/helpers"

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
  /** the currently-attached `/command` (prepended to the message on send), or
   *  null. Durable per-thread (T654). */
  attachedCmd: CommandSuggestion | null
  canSend: (pendingFiles: number) => boolean
  onChange: (e: React.ChangeEvent<HTMLTextAreaElement>) => void
  onSelect: (e: React.SyntheticEvent<HTMLTextAreaElement>) => void
  handleKeyDown: (e: React.KeyboardEvent<HTMLTextAreaElement>) => void
  handleSubmit: () => void
  /** Attach a `/command` as the message prefix (replaces any current one, max
   *  one) and strip the slash trigger line from the draft. */
  attachCmd: (s: CommandSuggestion) => void
  /** Remove the attached `/command`. */
  detachCmd: () => void
}

/** Read the persisted attached command for a thread (T654). Tolerant: a missing
 *  or malformed value yields null. */
function readAttachedCmd(commandKey: string | undefined): CommandSuggestion | null {
  if (!commandKey) return null
  try {
    const raw = localStorage.getItem(commandKey)
    if (!raw) return null
    const parsed: unknown = JSON.parse(raw)
    if (parsed && typeof parsed === "object" && "command" in parsed) {
      return parsed as CommandSuggestion
    }
  } catch {
    // corrupt entry — treat as none
  }
  return null
}

/**
 * Own the composer's draft text + caret, the persisted-draft round-trip, the
 * auto-grow textarea, and the keyboard/command-prefill handlers — identical to
 * desktop (shared behaviour), extracted to keep both units within P8 budgets.
 */
function useComposer(
  draftKey: string | undefined,
  commandKey: string | undefined,
  onSend: ((text: string) => void) | undefined,
): Composer {
  // Seed text + caret from the persisted draft ONCE per mount so a remount
  // (thread switch / return) or a full reload restores what was being typed
  // and where the cursor sat (T304).
  const [seed] = useState(() => parseDraft(draftKey))
  const [text, setText] = useState(() => seed.text)
  const [caret, setCaret] = useState(() => seed.selStart)
  // The attached `/command` (T654) — seeded once from localStorage so it
  // survives a refresh, prepended to the message on send, at most one.
  const [attachedCmd, setAttachedCmd] = useState<CommandSuggestion | null>(() =>
    readAttachedCmd(commandKey),
  )
  const textareaRef = useRef<HTMLTextAreaElement>(null)

  // Persist the attached command per thread; remove the key when detached.
  const persistCmd = (c: CommandSuggestion | null) => {
    if (!commandKey) return
    if (c) localStorage.setItem(commandKey, JSON.stringify(c))
    else localStorage.removeItem(commandKey)
  }

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

  const canSend = (pendingFiles: number) =>
    text.trim().length > 0 || pendingFiles > 0 || attachedCmd !== null

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
   * Attach a suggested `/command` as the message prefix (T654). Replaces any
   * currently-attached command (at most one), persists it durably, and strips
   * the slash trigger line (bare `/` or partial like `/bo`) out of the draft so
   * the composer is left clean — the command now rides as a chip, not text.
   */
  const attachCmd = (s: CommandSuggestion) => {
    const { start, end } = lineBounds(text, caret)
    const onSlashLine = text.slice(start, end).startsWith("/")
    if (onSlashLine) {
      const next = text.slice(0, start) + text.slice(end)
      setText(next)
      setCaret(start)
      persistDraft(next, start, start)
      requestAnimationFrame(() => {
        const el = textareaRef.current
        if (!el) return
        el.setSelectionRange(start, start)
        autoResize()
      })
    }
    setAttachedCmd(s)
    persistCmd(s)
  }

  /** Remove the attached `/command` (T654). */
  const detachCmd = () => {
    setAttachedCmd(null)
    persistCmd(null)
  }

  const handleSubmit = () => {
    if (!onSend) return
    // Prepend the attached command's prompt (T654): its body rides as a prefix,
    // separated from the user's message by a blank line. Sending is allowed with
    // only a command attached (empty text). After send, the command detaches.
    const base = attachedCmd?.body?.trim() ? attachedCmd.body.trimEnd() : ""
    const msg = base && text.trim() ? `${base}\n\n${text}` : base || text
    if (msg.trim().length === 0) return
    onSend(msg)
    setText("")
    setCaret(0)
    persistDraft("", 0, 0)
    if (attachedCmd) {
      setAttachedCmd(null)
      persistCmd(null)
    }
    requestAnimationFrame(() => {
      const el = textareaRef.current
      if (el) el.style.height = "auto"
      // Mobile divergence: BLUR after send (not focus like desktop) so the caret
      // leaves the field and the on-screen keyboard dismisses — sending is a
      // "done typing" gesture on a phone, whereas desktop keeps focus to fire off
      // several messages in a row.
      el?.blur()
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
    attachedCmd,
    canSend,
    onChange,
    onSelect,
    handleKeyDown,
    handleSubmit,
    attachCmd,
    detachCmd,
  }
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
  onEditCommand,
  commandKey,
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
  /** Opens the command editor prefilled (T654). Omit to hide the per-pill edit button. */
  onEditCommand?: ((s: CommandSuggestion) => void) | undefined
  /** localStorage key for persisting the unsent draft + caret per thread (T304). */
  draftKey?: string | undefined
  /** localStorage key for the durably-attached `/command` prefix per thread
   *  (T654). Omit to disable command attachment persistence. */
  commandKey?: string | undefined
}) {
  const composer = useComposer(draftKey, commandKey, onSend)

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
        composer.attachCmd(first)
        return
      }
    }

    if (e.key === " " && composer.slashPrefix !== null) {
      const match = suggestions.find(
        (s) => s.command.slice(1).toLowerCase() === composer.slashPrefix?.toLowerCase(),
      )
      if (match) {
        e.preventDefault()
        composer.attachCmd(match)
        return
      }
    }

    composer.handleKeyDown(e)
  }

  return (
    <FrostedBottomBar className="px-3 pt-3 pb-[max(1rem,env(safe-area-inset-bottom))]">
      {/* Unified bubble row (T350) — file chips + /command suggestions +
          create-command pill in ONE container. */}
      {(pendingFiles.length > 0 || commandsActive || composer.attachedCmd) && (
        <ComposerBubbles
          files={pendingFiles}
          onRemoveFile={onRemoveFile}
          suggestions={commandsActive ? filteredSuggestions : []}
          onPick={composer.attachCmd}
          attachedCommand={composer.attachedCmd}
          onDetachCommand={composer.detachCmd}
          onCreateCommand={commandsActive ? onCreateCommand : undefined}
          onEditCommand={onEditCommand}
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
