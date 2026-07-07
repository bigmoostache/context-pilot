import { useState } from "react"
import { Pin, X } from "lucide-react"
import type { LucideIcon } from "lucide-react"
import type { FinderNode } from "@/lib/types"
import { collectStarred } from "@/lib/support/finderFs"
import { extOf, kindMeta } from "./kind"
import { FileIcon } from "./macIcons"
import { FOLDER_DRAG_MIME } from "../views/helpers"
import type { PinnedFolder } from "../Finder"
import { cn } from "@/lib/utils"

// ── Left sidebar: favorites / locations / pinned ──────────────────
/** True when a drag carries a Finder folder payload (pin-drop target). */
function acceptsFolder(e: React.DragEvent): boolean {
  return e.dataTransfer.types.includes(FOLDER_DRAG_MIME)
}

export function FinderSidebar({
  root,
  cwd,
  pins,
  onNavigate,
  onOpen,
  onPin,
  onUnpin,
}: {
  root: FinderNode
  cwd: string
  pins: PinnedFolder[]
  onNavigate: (path: string) => void
  onOpen: (node: FinderNode) => void
  /** pin a folder dropped onto / right-clicked into the sidebar */
  onPin: (p: PinnedFolder) => void
  /** remove a pinned folder */
  onUnpin: (path: string) => void
}) {
  const topFolders = (root.children ?? []).filter((c) => c.kind === "folder")
  const starred = collectStarred(root)
  const [dropActive, setDropActive] = useState(false)

  return (
    <aside className="flex w-[var(--sidebar-w)] shrink-0 flex-col gap-3.5 overflow-y-auto border-r border-border bg-surface px-2.5 py-3">
      {starred.length > 0 && (
        <Group label="Favorites">
          {starred.map((n) => (
            <Place
              key={n.path}
              leading={<FileIcon kind={n.kind} ext={extOf(n.name)} size={16} />}
              label={n.name}
              accent={n.kind === "folder" ? "var(--warn)" : kindMeta[n.kind].accent}
              onClick={() => (n.kind === "folder" ? onNavigate(n.path) : onOpen(n))}
            />
          ))}
        </Group>
      )}

      <Group label="Locations">
        <Place
          leading={<FileIcon kind="folder" size={16} />}
          label={root.name}
          active={cwd === root.path}
          accent="var(--signal)"
          onClick={() => onNavigate(root.path)}
        />
        {topFolders.map((f) => (
          <Place
            key={f.path}
            leading={<FileIcon kind="folder" size={16} />}
            label={f.name}
            active={cwd === f.path}
            accent="var(--warn)"
            indent
            onClick={() => onNavigate(f.path)}
          />
        ))}
      </Group>

      {/* Pinned — a drag-and-drop target (drop a folder, or right-click → Pin). */}
      <div
        onDragOver={(e) => {
          if (!acceptsFolder(e)) {
            return
          }

          e.preventDefault()
          if (!dropActive) setDropActive(true)
        }}
        onDragLeave={(e) => {
          if (e.currentTarget === e.target) setDropActive(false)
        }}
        onDrop={(e) => {
          if (!acceptsFolder(e)) return
          e.preventDefault()
          setDropActive(false)
          const raw = e.dataTransfer.getData(FOLDER_DRAG_MIME)
          if (!raw) return
          try {
            const p = JSON.parse(raw) as PinnedFolder
            if (p.path) onPin({ name: p.name, path: p.path })
          } catch {
            /* malformed drag payload — ignore */
          }
        }}
        className={cn(
          "flex flex-col gap-0.5 rounded-lg transition-colors",
          dropActive && "bg-[var(--signal)]/10 ring-1 ring-inset ring-[var(--signal)]/45",
        )}
      >
        <span className="px-2 pb-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground/50">
          Pinned
        </span>
        {pins.map((p) => (
          // `flex flex-col` so the single Place child STRETCHES to full width
          // (flex-col's default align-items:stretch), exactly like a Locations
          // row — those stretch only because their parent Group is also a
          // flex-col. A plain block/`relative` parent leaves the <button> at
          // its intrinsic (text) width, which was the real "pinned looks
          // different" mismatch. `relative` is kept (alongside flex) so the
          // absolute Unpin button still anchors to the row; it self-centers via
          // top-1/2 + -translate-y-1/2.
          <div key={p.path} className="group/pin relative flex flex-col">
            <Place
              leading={<FileIcon kind="folder" size={16} />}
              label={p.name}
              active={cwd === p.path}
              accent="var(--warn)"
              onClick={() => onNavigate(p.path)}
            />
            <button
              title="Unpin"
              onClick={() => onUnpin(p.path)}
              className="absolute right-1.5 top-1/2 flex size-4 -translate-y-1/2 items-center justify-center rounded text-muted-foreground/50 opacity-0 transition-opacity hover:text-foreground group-hover/pin:opacity-100"
            >
              <X className="size-3" />
            </button>
          </div>
        ))}
        {pins.length === 0 && (
          <div className="mx-1 flex items-center gap-1.5 rounded-md border border-dashed border-border px-2.5 py-2 text-[10.5px] leading-relaxed text-muted-foreground/55">
            <Pin className="size-3 shrink-0" />
            Drag a folder here — or right-click a folder → Pin — to keep it handy.
          </div>
        )}
      </div>
    </aside>
  )
}

function Group({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-0.5">
      <span className="px-2 pb-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground/50">
        {label}
      </span>
      {children}
    </div>
  )
}

function Place({
  icon: Icon,
  leading,
  label,
  active,
  accent,
  indent,
  muted,
  onClick,
}: {
  icon?: LucideIcon
  /** custom leading element (e.g. a macOS FileIcon); overrides `icon` */
  leading?: React.ReactNode
  label: string
  active?: boolean
  accent: string
  indent?: boolean
  muted?: boolean
  onClick?: () => void
}) {
  return (
    <button
      onClick={onClick}
      className={cn(
        "flex items-center gap-2 rounded-md py-1.5 pr-2 text-left text-[12.5px] transition-colors",
        indent ? "pl-5" : "pl-2",
        active
          ? "bg-card font-medium text-foreground card-shadow"
          : "text-foreground/75 hover:bg-muted/60",
        muted && "cursor-default opacity-70",
      )}
    >
      {leading ?? (Icon && <Icon className="size-4 shrink-0" style={{ color: accent }} />)}
      <span className="truncate">{label}</span>
    </button>
  )
}
