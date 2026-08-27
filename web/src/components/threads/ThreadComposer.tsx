import { useEffect, useMemo, useRef, useState } from "react"
import { lineBounds, resolveEnter, resolveTab } from "@/lib/utils"
import { measure } from "@/lib/support/telemetry"
import type { ThreadStatus } from "@/lib/types"
import { ComposerBubbles } from "./fileUpload"
import type { UploadedFile, CommandSuggestion } from "./fileUpload/helpers"
import { ComposerInputRow, ComposerBanner } from "./fileUpload/composerInput"
import { parseDraft, resolveComposerBanner } from "@/lib/support/threadMessages"

// CommandSuggestion now lives beside the file-chip abstraction in ./fileUpload
// (both composer pill families share ONE module + ONE rendered row). Re-exported
// here for the existing `import { type CommandSuggestion } from "./ThreadComposer"`
// consumers (ThreadConversation).
export type { CommandSuggestion } from "./fileUpload/helpers"

/** Everything the composer render needs from its draft/keyboard logic. Flat
 *  (not nested under one object) so the render passes `textareaRef` to `ref=`
 *  as a bare identifier — the react-hooks/refs pass rejects reading a ref
 *  through a member access of a ref-bearing object. */
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
  commandKey: string | undefined,
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
  /** `/command` suggestions (T348). Non-empty renders clickable bubbles; click prefills. */
  suggestions?: CommandSuggestion[] | undefined
  /** True when thread has no messages yet — scopes auto-show bubbles to first message (T350). */
  firstMessage?: boolean
  /** Opens the "create command" dialog (T350). Omit to hide the pill. */
  onCreateCommand?: (() => void) | undefined
  /** Opens the command editor prefilled (T654). Omit to hide the per-pill edit button. */
  onEditCommand?: ((s: CommandSuggestion) => void) | undefined
  /** localStorage key for persisting the unsent draft + caret per thread (T304).
   *  Stored as `{text,selStart,selEnd}` JSON; legacy bare-string also read. */
  draftKey?: string | undefined
  /** localStorage key for the durably-attached `/command` prefix per thread
   *  (T654). Omit to disable command attachment persistence. */
  commandKey?: string | undefined
}) {
  const composer = useComposer(draftKey, commandKey, onSend)

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
    // attach the first one instead of indenting (T654).
    if (e.key === "Tab" && !e.shiftKey && composer.slashPrefix !== null) {
      const first = filteredSuggestions[0]
      if (first) {
        e.preventDefault()
        composer.attachCmd(first)
        return
      }
    }

    // Space expansion: if the current line is exactly a known command (e.g.
    // `/boss-hunt`), pressing Space attaches it (T654).
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
    // No padding — the composer runs flush to its column. `shrink-0` stays: the
    // conversation above is the flex child that scrolls, so without it the
    // composer would compress as the log grows instead of holding its height.
    <div className="shrink-0">
      {/* Unified bubble row (T350) — file-upload chips + /command suggestions +
          the create-command pill, all in ONE transparent, normal-flow container
          between the conversation and the textarea. */}
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
    </div>
  )
}
