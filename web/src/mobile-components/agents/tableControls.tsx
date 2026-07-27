import { useEffect, useRef, useState } from "react"
import { Trash2 } from "lucide-react"
import type { ReactNode } from "react"
import type { useEditor } from "@tiptap/react"
import { cn } from "@/lib/utils"

// ── Grid picker ────────────────────────────────────────────────────
// Tap-to-select grid shown on Table button tap — pick rows × cols, then insert.
// Mobile twin: the desktop hover-to-preview interaction doesn't exist on touch,
// so the grid cells are larger (24px) tap targets and the selection is driven by
// the last touched cell rather than hover.

const GRID_COLS = 8
const GRID_ROWS = 6

export function TableGridPicker({
  onSelect,
  onClose,
}: {
  onSelect: (rows: number, cols: number) => void
  onClose: () => void
}) {
  const [hover, setHover] = useState<{ r: number; c: number } | null>(null)
  const ref = useRef<HTMLDivElement>(null)

  // Close on outside pointer.
  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (ref.current && e.target instanceof Node && !ref.current.contains(e.target)) onClose()
    }
    document.addEventListener("mousedown", handler)
    return () => document.removeEventListener("mousedown", handler)
  }, [onClose])

  // Close on Escape (hardware keyboard, if any).
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose()
    }
    document.addEventListener("keydown", handler)
    return () => document.removeEventListener("keydown", handler)
  }, [onClose])

  return (
    <div
      ref={ref}
      className="absolute top-full left-0 z-50 mt-1 rounded-lg border border-border bg-card p-3 shadow-lg"
    >
      <div
        className="grid gap-1"
        style={{ gridTemplateColumns: `repeat(${GRID_COLS}, 1fr)` }}
        onMouseLeave={() => setHover(null)}
      >
        {Array.from({ length: GRID_ROWS }, (_, r) =>
          Array.from({ length: GRID_COLS }, (_, c) => {
            const highlighted = hover != null && r <= hover.r && c <= hover.c
            return (
              <button
                type="button"
                key={`${r}-${c}`}
                className={cn(
                  "size-6 cursor-pointer rounded-[3px] border transition-colors duration-75",
                  highlighted
                    ? "border-(--signal)/60 bg-(--signal)/25"
                    : "border-border/40 bg-muted/20",
                )}
                // On touch, hover fires just before click (pointer coalesce) so
                // the preview + insert still read correctly; explicit setHover on
                // tap keeps the size label live.
                onMouseEnter={() => setHover({ r, c })}
                onClick={() => {
                  onSelect(r + 1, c + 1)
                  onClose()
                }}
              />
            )
          }),
        )}
      </div>
      <div className="mt-2 text-center text-[11px] font-medium text-muted-foreground">
        {hover ? `${hover.c + 1} × ${hover.r + 1}` : "Choose size"}
      </div>
    </div>
  )
}

// ── Table context bar ──────────────────────────────────────────────
// Appears between the toolbar and editor when the cursor is inside a table.
// Mobile twin: horizontally scrollable (a phone can't fit every op across) with
// larger tap padding and active: press feedback.

export function TableContextBar({ editor }: { editor: NonNullable<ReturnType<typeof useEditor>> }) {
  return (
    <div className="no-scrollbar flex h-9 shrink-0 items-center gap-0.5 overflow-x-auto border-b border-border bg-muted/25 px-2">
      <span className="mr-1 shrink-0 text-[10px] font-semibold tracking-wide text-muted-foreground/60 uppercase">
        Table
      </span>
      <Sep />

      {/* ── Column operations ── */}
      <TinyBtn
        label="+ Col before"
        onClick={() => editor.chain().focus().addColumnBefore().run()}
        disabled={!editor.can().addColumnBefore()}
      />
      <TinyBtn
        label="+ Col after"
        onClick={() => editor.chain().focus().addColumnAfter().run()}
        disabled={!editor.can().addColumnAfter()}
      />
      <TinyBtn
        label="− Col"
        danger
        onClick={() => editor.chain().focus().deleteColumn().run()}
        disabled={!editor.can().deleteColumn()}
      />
      <Sep />

      {/* ── Row operations ── */}
      <TinyBtn
        label="+ Row above"
        onClick={() => editor.chain().focus().addRowBefore().run()}
        disabled={!editor.can().addRowBefore()}
      />
      <TinyBtn
        label="+ Row below"
        onClick={() => editor.chain().focus().addRowAfter().run()}
        disabled={!editor.can().addRowAfter()}
      />
      <TinyBtn
        label="− Row"
        danger
        onClick={() => editor.chain().focus().deleteRow().run()}
        disabled={!editor.can().deleteRow()}
      />
      <Sep />

      {/* ── Header toggle + delete table ── */}
      <TinyBtn
        label="Header row"
        active={editor.isActive("tableHeader")}
        onClick={() => editor.chain().focus().toggleHeaderRow().run()}
      />
      <TinyBtn
        label="Delete table"
        danger
        onClick={() => editor.chain().focus().deleteTable().run()}
        disabled={!editor.can().deleteTable()}
        icon={<Trash2 className="size-3" />}
      />
    </div>
  )
}

/** Compact text button for the table context bar — larger tap padding + active:
 *  press feedback vs the desktop hover variant. */
function TinyBtn({
  label,
  onClick,
  disabled,
  active,
  danger,
  icon,
}: {
  label: string
  onClick: () => void
  disabled?: boolean
  active?: boolean
  danger?: boolean
  icon?: ReactNode
}) {
  return (
    <button
      type="button"
      title={label}
      onMouseDown={(e) => e.preventDefault()}
      onClick={onClick}
      disabled={disabled}
      className={cn(
        "flex shrink-0 items-center gap-1 rounded-md px-2 py-1.5 text-[11px] font-medium transition-colors",
        disabled && "opacity-30",
        active && "bg-(--signal)/15 text-(--signal)",
        danger && !disabled && "active:text-(--danger)",
        !active && !danger && "text-muted-foreground/70 active:bg-muted/60 active:text-foreground",
        danger && !active && "text-muted-foreground/70 active:bg-(--danger)/10",
      )}
    >
      {icon}
      {label}
    </button>
  )
}

function Sep() {
  return <span className="mx-0.5 h-4 w-px shrink-0 bg-border" />
}
