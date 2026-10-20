import { useState } from "react"
import { cn } from "@/lib/utils"

/**
 * Inline name editor that replaces an agent row's title while renaming.
 * Opens focused with the current name selected. Enter commits a trimmed,
 * non-blank, changed name; Escape or blur cancels. Clicks and keys stop at the
 * input so the row behind it is not selected.
 */
export function RenameInput({
  name,
  onCommit,
  onCancel,
  className,
}: {
  name: string
  onCommit: (name: string) => void
  onCancel: () => void
  className?: string | undefined
}) {
  const [draft, setDraft] = useState(name)
  const commit = () => {
    const next = draft.trim()
    if (next === "" || next === name) onCancel()
    else onCommit(next)
  }
  return (
    <input
      autoFocus
      value={draft}
      aria-label="Agent name"
      onFocus={(e) => e.currentTarget.select()}
      onChange={(e) => setDraft(e.target.value)}
      onClick={(e) => e.stopPropagation()}
      onKeyDown={(e) => {
        e.stopPropagation()
        if (e.key === "Enter") {
          e.preventDefault()
          commit()
        } else if (e.key === "Escape") {
          e.preventDefault()
          onCancel()
        }
      }}
      onBlur={onCancel}
      className={cn(
        "min-w-0 flex-1 rounded-sm bg-background px-1 text-foreground ring-1 ring-(--interactive) outline-none",
        className,
      )}
    />
  )
}
