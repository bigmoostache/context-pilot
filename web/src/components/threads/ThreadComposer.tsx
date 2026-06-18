import { useEffect, useRef, useState } from "react"
import { ArrowUp, Paperclip, Loader2, Clock } from "lucide-react"
import type { ThreadStatus } from "@/lib/types"

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
}: {
  status: ThreadStatus
  /** true when this is the single thread the agent is currently focused on */
  focused?: boolean
  onSend?: (text: string) => void
}) {
  const [text, setText] = useState("")
  const textareaRef = useRef<HTMLTextAreaElement>(null)

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

  const canSend = text.trim().length > 0

  const handleSubmit = () => {
    if (!canSend || !onSend) return
    onSend(text)
    setText("")
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
      <div className="flex items-end gap-2 rounded-2xl border border-border bg-card px-3 py-2.5 card-shadow focus-within:border-[var(--signal)]/60">
        <button className="mb-0.5 text-muted-foreground/60 transition-colors hover:text-[var(--interactive)]">
          <Paperclip className="size-4" />
        </button>
        <textarea
          ref={textareaRef}
          autoFocus
          value={text}
          onChange={(e) => setText(e.target.value)}
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
