import { useEffect, useRef, useState } from "react"
import { Trash2 } from "lucide-react"
import type { ReactNode } from "react"
import type { useEditor } from "@tiptap/react"
import { cn } from "@/lib/utils"

// ── Grid picker ────────────────────────────────────────────────────
// Hoverable grid shown on Table button click — select rows × cols, then insert.

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

  // Close on outside click.
  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose()
    }
    document.addEventListener("mousedown", handler)
    return () => document.removeEventListener("mousedown", handler)
  }, [onClose])

  // Close on Escape.
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
      className="absolute top-full left-0 z-50 mt-1 rounded-lg border border-border bg-card p-2.5 shadow-lg"
    >
      <div
        className="grid gap-[3px]"
        style={{ gridTemplateColumns: `repeat(${GRID_COLS}, 1fr)` }}
        onMouseLeave={() => setHover(null)}
      >
        {Array.from({ length: GRID_ROWS }, (_, r) =>
          Array.from({ length: GRID_COLS }, (_, c) => {
            const highlighted = hover != null && r <= hover.r && c <= hover.c
            return (
              <div
                key={`${r}-${c}`}
                className={cn(
                  "size-[18px] rounded-[2px] border transition-colors duration-75 cursor-pointer",
                  highlighted
                    ? "border-(--signal)/60 bg-(--signal)/25"
                    : "border-border/40 bg-muted/20",
                )}
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
      <div className="mt-2 text-center text-[10.5px] font-medium text-muted-foreground">
        {hover ? `${hover.c + 1} × ${hover.r + 1}` : "Choose size"}
      </div>
    </div>
  )
}

// ── Table context bar ──────────────────────────────────────────────
// Appears between the main toolbar and the editor when cursor is inside a table.

export function TableContextBar({
  editor,
}: {
  editor: NonNullable<ReturnType<typeof useEditor>>
}) {
  return (
    <div className="flex h-7 shrink-0 items-center gap-0.5 border-b border-border bg-muted/25 px-2">
      <span className="mr-1 text-[10px] font-semibold tracking-wide text-muted-foreground/60 uppercase">
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

/** Compact text button for the table context bar. */
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
        "flex items-center gap-1 rounded-md px-1.5 py-0.5 text-[10px] font-medium transition-colors",
        disabled && "opacity-30",
        active && "bg-(--signal)/15 text-(--signal)",
        danger && !disabled && "hover:text-(--danger)",
        !active && !danger && "text-muted-foreground/70 hover:bg-muted/60 hover:text-foreground",
        danger && !active && "text-muted-foreground/70 hover:bg-(--danger)/10",
      )}
    >
      {icon}
      {label}
    </button>
  )
}

function Sep() {
  return <span className="mx-0.5 h-4 w-px bg-border" />
}
