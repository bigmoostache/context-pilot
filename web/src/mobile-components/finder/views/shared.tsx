import { useEffect, useRef } from "react"
import type { FinderNode, FinderTag } from "@/lib/types"
import { TAG_META } from "../support/kind"
import { cn } from "@/lib/utils"

/**
 * Inline rename field — mobile twin of `components/finder/views/shared`
 * RenameInput. Same commit-on-Enter/blur, cancel-on-Esc, basename-preselect,
 * and key-stop behaviour; the only fork is a **16px** text size so iOS Safari
 * doesn't zoom the viewport when the field takes focus (the desktop 12px
 * triggers focus-zoom on a phone).
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
          if (e.target instanceof HTMLInputElement) onCommit(e.target.value)
        } else if (e.key === "Escape") {
          e.preventDefault()
          onCancel()
        }
      }}
      onBlur={(e) => onCommit(e.target.value)}
      className="min-w-0 flex-1 rounded-[4px] border border-(--signal) bg-background px-1 py-px text-[16px] text-foreground ring-2 ring-(--signal)/40 outline-none"
    />
  )
}

/** Colored macOS finder tag dots — identical to desktop (pure indicator). */
export function TagDots({
  tags,
  className,
}: {
  tags?: FinderTag[] | undefined
  className?: string | undefined
}) {
  if (!tags || tags.length === 0) return null
  return (
    <span className={cn("flex items-center gap-0.5", className)}>
      {tags.map((t) => (
        <span
          key={t}
          title={TAG_META[t].label}
          className="size-2 rounded-full ring-1 ring-black/10 ring-inset"
          style={{ background: TAG_META[t].color }}
        />
      ))}
    </span>
  )
}
