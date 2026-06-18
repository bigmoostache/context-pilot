import { useState, useRef } from "react"
import { ArrowUp, Paperclip, Loader2 } from "lucide-react"
import type { ThreadStatus } from "@/lib/types"

/**
 * Thread composer — always active, regardless of turn status. When the thread
 * isn't MY_TURN a subtle "working" hint appears above the input so the user
 * knows the agent is active, but can still send a message at any time.
 */
export function ThreadComposer({
  status,
  onSend,
}: {
  status: ThreadStatus
  onSend?: (text: string) => void
}) {
  const [text, setText] = useState("")
  const textareaRef = useRef<HTMLTextAreaElement>(null)

  const isWorking = status !== "MY_TURN"
  const isActive = status === "ACTIVE"

  const canSend = text.trim().length > 0

  const handleSubmit = () => {
    if (!canSend || !onSend) return
    onSend(text)
    setText("")
    textareaRef.current?.focus()
  }

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault()
      handleSubmit()
    }
  }

  return (
    <div className="shrink-0 px-5 pb-4 pt-2">
      {isWorking && (
        <div className="mb-2 flex items-center justify-center gap-2 rounded-xl bg-muted/40 px-3 py-1.5 text-[11.5px] text-muted-foreground">
          <Loader2
            className="size-3.5 animate-spin"
            style={{ color: isActive ? "var(--ok)" : "var(--signal)" }}
          />
          <span>
            {isActive
              ? "Agent is streaming…"
              : "Agent is working this thread…"}
          </span>
        </div>
      )}
      <div className="flex items-end gap-2 rounded-2xl border border-border bg-card px-3 py-2.5 card-shadow focus-within:border-[var(--signal)]/60">
        <button className="mb-0.5 text-muted-foreground/60 transition-colors hover:text-[var(--interactive)]">
          <Paperclip className="size-4" />
        </button>
        <textarea
          ref={textareaRef}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={handleKeyDown}
          placeholder="Reply to this thread…"
          rows={1}
          className="max-h-[200px] min-h-[24px] flex-1 resize-none bg-transparent text-[13.5px] leading-relaxed text-foreground/90 placeholder:text-muted-foreground/60 outline-none"
          style={{ fieldSizing: "content" } as React.CSSProperties}
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
