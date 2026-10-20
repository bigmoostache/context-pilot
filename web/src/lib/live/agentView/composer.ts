// ── Composer draft/keyboard logic (shared, non-forked) ──────────────
//
// The agent composer's behaviour, extracted out of the desktop AND mobile
// `AgentComposer` twins so both trees run the exact same draft, caret,
// `/command` and Enter/Tab logic (design-mobile.md §3.2, rule M141: no logic
// duplication across component trees). Only the presentation forks.
//
// The two real device differences are a `device` option, not a fork:
//   • mount restore — desktop focuses the textarea; mobile does NOT, because
//     focusing on agent-open pops the on-screen keyboard unbidden (T622).
//   • after send — desktop refocuses so several messages can go out in a row;
//     mobile blurs, so the keyboard dismisses ("done typing" on a phone).

import { useEffect, useMemo, useRef, useState } from "react"
import { lineBounds, resolveEnter, resolveTab } from "@/lib/utils"
import { measure } from "@/lib/support/telemetry"
import { parseDraft } from "@/lib/support/agentMessages"
import type { CommandSuggestion } from "@/lib/live/agentUpload"

/** Which component tree hosts the composer — picks focus-vs-blur behaviour. */
export type ComposerDevice = "desktop" | "mobile"

/** Everything the composer render needs from its draft/keyboard logic. Flat
 *  (not nested under one object) so the render passes `textareaRef` to `ref=`
 *  as a bare identifier — the react-hooks/refs pass rejects reading a ref
 *  through a member access of a ref-bearing object. */
export interface Composer {
  text: string
  caret: number
  textareaRef: React.RefObject<HTMLTextAreaElement | null>
  slashPrefix: string | null
  /** the currently-attached `/command` (prepended to the message on send), or
   *  null. Durable per-agent (T654). */
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

/** Inputs of {@link useComposer}. */
export interface ComposerOpts {
  /** localStorage key of the per-agent unsent draft (text + caret, T304). */
  draftKey: string | undefined
  /** localStorage key of the per-agent attached `/command` (T654). */
  commandKey: string | undefined
  /** may resolve `false` on failure — the composer then restores the text */
  onSend: ((text: string) => Promise<boolean>) | undefined
  device: ComposerDevice
}

/** Textarea auto-grow cap in px; beyond it the field scrolls. */
const MAX_H = 200

/** Read the persisted attached command for an agent (T654). Tolerant: a
 *  missing or malformed value yields null. */
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

/** Write a JSON value under `key`, or remove the key when `value` is falsy. */
function persist(key: string | undefined, value: unknown) {
  if (!key) return
  if (value) localStorage.setItem(key, JSON.stringify(value))
  else localStorage.removeItem(key)
}

/**
 * Own the composer's draft text + caret, the persisted-draft round-trip, the
 * auto-grow textarea, and the keyboard/command-prefill handlers.
 *
 * The draft is seeded ONCE per mount from `draftKey` (lazy `useState`, so a
 * remount or full reload restores text and caret), persisted on every
 * edit/caret move, and cleared on send. `onSend` fires on a plain Enter that
 * {@link resolveEnter} classifies as a send, or on {@link Composer.handleSubmit}.
 */
export function useComposer({ draftKey, commandKey, onSend, device }: ComposerOpts): Composer {
  const [seed] = useState(() => parseDraft(draftKey))
  const [text, setText] = useState(() => seed.text)
  // Caret offset: tells which line is being edited, so the /command bubbles
  // surface when the current line starts with `/` (T350).
  const [caret, setCaret] = useState(() => seed.selStart)
  const [attachedCmd, setAttachedCmd] = useState<CommandSuggestion | null>(() =>
    readAttachedCmd(commandKey),
  )
  const textareaRef = useRef<HTMLTextAreaElement>(null)

  const persistCmd = (c: CommandSuggestion | null) => {
    persist(commandKey, c)
  }
  // An empty draft removes its key, so sent/cleared drafts never linger.
  const persistDraft = (t: string, s: number, e: number) => {
    persist(draftKey, t ? { text: t, selStart: s, selEnd: e } : null)
  }

  // Restore the saved caret/selection once the textarea mounts (T304).
  useEffect(() => {
    const el = textareaRef.current
    if (!el || !seed.text) return
    if (device === "desktop") el.focus()
    el.setSelectionRange(seed.selStart, seed.selEnd)
  }, [seed, device])

  // JS-driven auto-grow (not experimental `field-sizing`). Reading
  // `scrollHeight` forces a reflow, so it is instrumented.
  const autoResize = () => {
    const el = textareaRef.current
    if (!el) return
    measure("composer:autosize", () => {
      el.style.height = "auto"
      el.style.height = `${Math.min(el.scrollHeight, MAX_H)}px`
    })
  }
  useEffect(autoResize, [text])

  // Text typed after `/` on the current line, or null when not on a slash
  // line. `""` = bare `/`, `"bo"` = `/bo` (T556).
  const slashPrefix = useMemo((): string | null => {
    const { start, end } = lineBounds(text, caret)
    const line = text.slice(start, end)
    return line.startsWith("/") ? line.slice(1) : null
  }, [text, caret])

  // A command-only message is sendable (handleSubmit sends its body alone).
  const canSend = (pendingFiles: number) =>
    text.trim().length > 0 || pendingFiles > 0 || attachedCmd !== null

  // Splice value + caret into state and the textarea in one shot. The caret
  // is restored after the controlled re-render via rAF (React resets it).
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

  // Attach a `/command` (T654): replaces any current one, persists it, and
  // strips the slash trigger line so the command rides as a chip, not text.
  const attachCmd = (s: CommandSuggestion) => {
    const { start, end } = lineBounds(text, caret)
    if (text.slice(start, end).startsWith("/")) {
      applyEdit(text.slice(0, start) + text.slice(end), start)
    }
    setAttachedCmd(s)
    persistCmd(s)
  }

  const detachCmd = () => {
    setAttachedCmd(null)
    persistCmd(null)
  }

  const handleSubmit = () => {
    if (!onSend) return
    // The attached command's body rides as a prefix, separated from the
    // message by a blank line; it detaches after send (T654).
    const base = attachedCmd?.body?.trim() ? attachedCmd.body.trimEnd() : ""
    const msg = base && text.trim() ? `${base}\n\n${text}` : base || text
    if (msg.trim().length === 0) return
    const sentText = text
    const sentCmd = attachedCmd
    // Failed send (no retry): put the text and command back. Anything typed
    // meanwhile is kept after the restored text.
    void onSend(msg).then((ok) => {
      if (ok) return
      setText((cur) => {
        const restored = cur ? `${sentText}\n${cur}` : sentText
        persistDraft(restored, restored.length, restored.length)
        return restored
      })
      setCaret(sentText.length)
      if (sentCmd) {
        setAttachedCmd(sentCmd)
        persistCmd(sentCmd)
      }
    })
    setText("")
    setCaret(0)
    persistDraft("", 0, 0)
    if (attachedCmd) detachCmd()
    // Collapse back to one row; refocus on desktop, blur on mobile.
    requestAnimationFrame(() => {
      const el = textareaRef.current
      if (!el) return
      el.style.height = "auto"
      if (device === "desktop") el.focus()
      else el.blur()
    })
  }

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    const { value, selectionStart: s, selectionEnd } = e.currentTarget
    const composing = e.nativeEvent.isComposing
    // Tab / Shift+Tab indent/outdent a list item (T359); elsewhere the
    // browser default stands.
    if (e.key === "Tab" && !composing) {
      const edit = resolveTab(value, s, selectionEnd, e.shiftKey)
      if (!edit) return
      e.preventDefault()
      applyEdit(edit.value, edit.caret)
      return
    }
    // TUI input-area port (T359): Shift+Enter = newline (default); a plain
    // Enter is hijacked and resolveEnter picks send vs list splice.
    if (e.key !== "Enter" || e.shiftKey || composing) return
    e.preventDefault()
    const action = resolveEnter(value, s, selectionEnd)
    if (action.kind === "send") handleSubmit()
    else applyEdit(action.value, action.caret)
  }

  const onChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
    const { value, selectionStart, selectionEnd } = e.target
    setText(value)
    setCaret(selectionStart)
    persistDraft(value, selectionStart, selectionEnd)
  }

  // Caret/selection moved without a text change: persist the range (T304).
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
