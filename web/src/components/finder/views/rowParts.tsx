import { useEffect, useRef } from "react"
import type { FinderNode, FinderTag } from "@/lib/types"
import { TAG_META } from "../support/kind"
import { cn } from "@/lib/utils"

// The two shared row sub-components, split out of `shared.tsx` so that file only
// exports helper functions — a file may not export both components and
// non-components under React Fast Refresh (react-refresh/only-export-components).

/**
 * Inline rename field — a macOS-style editable name cell. Mounts focused with the
 * basename (sans extension) pre-selected, commits on Enter or blur, cancels on
 * Esc. Keydown is stopped from bubbling so the Finder surface's own key handler
 * (arrows / type-ahead / Enter-to-rename) never fires while the user types.
 */
export function RenameInput({
  node,
  onCommit,
  onCancel,
}: {
  node: FinderNode
  onCommit: (newName: string) => void
  onCancel: () => void
}) {
  const ref = useRef<HTMLInputElement>(null)
  // Select the basename (everything before the last dot) on mount, like Finder —
  // so a quick retype keeps the extension. A dotfile / extensionless name selects
  // whole.
  useEffect(() => {
    const el = ref.current
    if (!el) return
    el.focus()
    const dot = node.name.lastIndexOf(".")
    el.setSelectionRange(0, dot > 0 ? dot : node.name.length)
  }, [node.name])

  return (
    <input
      ref={ref}
      type="text"
      defaultValue={node.name}
      spellCheck={false}
      onClick={(e) => e.stopPropagation()}
      onDoubleClick={(e) => e.stopPropagation()}
      onKeyDown={(e) => {
        e.stopPropagation()
        if (e.key === "Enter") {
          e.preventDefault()
          onCommit((e.target as HTMLInputElement).value)
        } else if (e.key === "Escape") {
          e.preventDefault()
          onCancel()
        }
      }}
      onBlur={(e) => onCommit(e.target.value)}
      className="min-w-0 flex-1 rounded-[4px] border border-[var(--signal)] bg-background px-1 py-px text-[12px] text-foreground outline-none ring-2 ring-[var(--signal)]/40"
    />
  )
}

/** Colored macOS finder tag dots. */
export function TagDots({ tags, className }: { tags?: FinderTag[]; className?: string }) {
  if (!tags || tags.length === 0) return null
  return (
    <span className={cn("flex items-center gap-0.5", className)}>
      {tags.map((t) => (
        <span
          key={t}
          title={TAG_META[t].label}
          className="size-2 rounded-full ring-1 ring-inset ring-black/10"
          style={{ background: TAG_META[t].color }}
        />
      ))}
    </span>
  )
}
