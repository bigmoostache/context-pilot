import { useRef, useState } from "react"
import { Bot, Download, Lock, Pencil, Plus, Trash2, Upload } from "lucide-react"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { useLibrary, sendCommand, useDeleteLibraryAgent } from "@/lib/live"
import { fetchLibraryAgent } from "@/lib/api"
import type { LibraryItem } from "@/lib/types"
import { AgentEditorDialog, type AgentEditorMode } from "./AgentEditorDialog"

/** The editor dialog's open state: closed, or open in one of its three flows. */
type EditorState =
  | { open: false }
  | {
      open: true
      mode: AgentEditorMode
      initial?: { name: string; description: string; body: string }
    }

// ── Behaviour-agent `.md` frontmatter parser (Import preview) ─────────
//
// A light client-side mirror of the orchestrator's frontmatter parser (which
// re-validates authoritatively on save, M141). Previews a picked `.md`'s
// title/description before opening the editor. String-op based (no regex) so it
// carries no catastrophic-backtracking or replaceAll lint debt. Kept private to
// this component (its only consumer) so lib/support stays within its entry cap.

/** Strip a leading/trailing single- or double-quote pair from a scalar. */
function stripQuotes(value: string): string {
  const q = value.at(0)
  if ((q === '"' || q === "'") && value.at(-1) === q) {
    return value.slice(1, -1)
  }
  return value
}

/** Drop a leading UTF-8 BOM (U+FEFF), then any leading CR/LF/space run. */
function stripLeading(text: string): string {
  const noBom = text.codePointAt(0) === 0xFE_FF ? text.slice(1) : text
  return noBom.trimStart()
}

/**
 * Parse a `.md` file's text into `{ name, description, body }` for the Import
 * preview. Returns `null` when the frontmatter block is missing or has no
 * `name:` (so the caller can reject the file before opening the editor).
 */
function parseAgentMd(text: string): { name: string; description: string; body: string } | null {
  const trimmed = stripLeading(text)
  if (!trimmed.startsWith("---")) return null

  // Skip the opening `---` and any immediate CR/LF before the YAML block.
  const rest = trimmed.slice(3).trimStart()
  const end = rest.indexOf("\n---")
  if (end === -1) return null

  const front = rest.slice(0, end)
  let name = ""
  let description = ""
  for (const line of front.split("\n")) {
    if (line.startsWith("name:")) {
      name = stripQuotes(line.slice("name:".length).trim())
    } else if (line.startsWith("description:")) {
      description = stripQuotes(line.slice("description:".length).trim())
    }
  }
  if (name.length === 0) return null

  // Body = everything after the closing fence line.
  const afterFence = rest.slice(end + 1)
  const nl = afterFence.indexOf("\n")
  const body = nl === -1 ? "" : afterFence.slice(nl + 1).trim()
  return { name, description, body }
}

/**
 * Active-behaviour-agent chip + selector — right of the footer "Ready" indicator
 * (T581). Shows the loaded system-prompt agent's name and, on click, a dropdown
 * to switch it, edit/export/delete each entry, and create/import new ones.
 *
 * Switching issues a `load_behaviour` command down the SAME live path threads
 * use (`sendCommand → POST /command → apply_command → set_active_agent`); the
 * active flag comes back through the library query's `behaviour_changed` fold.
 * Editing/creating/importing all converge on the backend's authoritative
 * `PUT …/library/agent/{itemId}` upsert (M141) — this component only renders +
 * calls. Delete is hidden for pure built-ins (no on-disk `.md` to remove).
 */
export function BehaviourChip({ agentId }: { agentId: string }) {
  const { data: library = [] } = useLibrary(agentId)
  const agentBehaviours = library.filter((item) => item.kind === "agent")
  const active = agentBehaviours.find((item) => item.active)
  const activeName = active?.name ?? "default"

  const [editor, setEditor] = useState<EditorState>({ open: false })
  const del = useDeleteLibraryAgent(agentId)
  const fileInputRef = useRef<HTMLInputElement>(null)

  const select = (id: string) => {
    if (id === active?.id) return
    void sendCommand(agentId, { kind: "load_behaviour", id }).catch(() => {
      // Fire-and-forget: a failed switch keeps the current behaviour; the
      // library query re-reports ground truth on its next fold/poll.
    })
  }

  // Export = fetch the raw `.md` (disk file or built-in seed) and download it.
  const exportAgent = (item: LibraryItem) => {
    void fetchLibraryAgent(agentId, item.id)
      .then((raw) => {
        const md = `---\nname: ${raw.name}\ndescription: ${raw.description}\n---\n${raw.body}\n`
        const url = URL.createObjectURL(new Blob([md], { type: "text/markdown" }))
        const a = document.createElement("a")
        a.href = url
        a.download = `${item.id}.md`
        a.click()
        URL.revokeObjectURL(url)
      })
      .catch(() => {
        /* export is best-effort; a failed fetch simply downloads nothing */
      })
  }

  // Import = read a picked `.md`, parse it client-side for a preview, and open
  // the editor prefilled. The backend re-validates authoritatively on save.
  const onImportPicked = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0]
    e.target.value = "" // allow re-picking the same file
    if (!file) return
    void file.text().then((text) => {
      const parsed = parseAgentMd(text)
      if (!parsed) {
        window.alert("That .md has no valid frontmatter (needs a `name:` field).")
        return
      }
      setEditor({ open: true, mode: { kind: "create" }, initial: parsed })
    })
  }

  return (
    <>
      <span className="h-3.5 w-px bg-border" />
      <DropdownMenu>
        <DropdownMenuTrigger className="flex cursor-pointer items-center gap-1.5 rounded-sm px-1.5 py-0.5 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground/85 focus:outline-none">
          <Bot className="size-3.5" />
          <span className="max-w-[120px] truncate font-medium text-foreground/80">{activeName}</span>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" side="top" className="min-w-64">
          <DropdownMenuGroup>
            <DropdownMenuLabel>System prompt</DropdownMenuLabel>
            <DropdownMenuSeparator />
            <DropdownMenuItem onClick={() => setEditor({ open: true, mode: { kind: "create" } })}>
              <span className="flex items-center gap-2 text-foreground/80">
                <Plus className="size-3.5" /> Create agent
              </span>
            </DropdownMenuItem>
            <DropdownMenuItem onClick={() => fileInputRef.current?.click()}>
              <span className="flex items-center gap-2 text-foreground/80">
                <Upload className="size-3.5" /> Import agent
              </span>
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            {agentBehaviours.length === 0 ? (
              <DropdownMenuItem disabled>No behaviours</DropdownMenuItem>
            ) : (
              agentBehaviours.map((item) => (
                <BehaviourRow
                  key={item.id}
                  item={item}
                  onSelect={() => select(item.id)}
                  onEdit={() =>
                    setEditor({
                      open: true,
                      mode: { kind: "edit", itemId: item.id, builtin: item.builtin === true },
                    })
                  }
                  onExport={() => exportAgent(item)}
                  onDelete={() => del.mutate(item.id)}
                />
              ))
            )}
          </DropdownMenuGroup>
        </DropdownMenuContent>
      </DropdownMenu>

      <input
        ref={fileInputRef}
        type="file"
        accept=".md,text/markdown"
        className="hidden"
        onChange={onImportPicked}
      />

      {editor.open && (
        <AgentEditorDialog
          open
          onClose={() => setEditor({ open: false })}
          agentId={agentId}
          mode={editor.mode}
          initial={editor.initial}
        />
      )}
    </>
  )
}

/** Wrap a row-action handler so its click doesn't also trigger the row's
 *  select (which switches the active behaviour). Hoisted to module scope —
 *  it closes over nothing, so redefining it per render is needless. */
const stopAnd = (fn: () => void) => (e: React.MouseEvent) => {
  e.preventDefault()
  e.stopPropagation()
  fn()
}

/**
 * One selectable behaviour row with hover-revealed Edit / Export / Delete
 * actions. Selecting the row body switches the active behaviour; the action
 * buttons `stopPropagation` so they don't also trigger a switch. Delete is
 * omitted for a pure built-in (`builtin && !active` is not the test — a built-in
 * with a local override still has a file: the backend 404s a fileless delete,
 * but we hide the button when there is provably nothing to delete, i.e. a
 * built-in the user has never overridden). Since the list payload can't tell an
 * overridden built-in from a pure one, we show Delete for every NON-built-in and
 * for built-ins too (the backend is the authoritative backstop, 404 → no-op).
 */
function BehaviourRow({
  item,
  onSelect,
  onEdit,
  onExport,
  onDelete,
}: {
  item: LibraryItem
  onSelect: () => void
  onEdit: () => void
  onExport: () => void
  onDelete: () => void
}) {
  // A pure built-in (compiled-in, never overridden) has no file to delete, so
  // hide Delete. The list marks `builtin: true` for BOTH a pure built-in and a
  // local override of one — but an override also carries a real on-disk name, so
  // it is indistinguishable here. We therefore hide Delete only for built-ins;
  // user agents always show it. (Backstop: the backend 404s a fileless delete.)
  const showDelete = item.builtin !== true

  return (
    <DropdownMenuItem
      onClick={onSelect}
      className={`group/row justify-between focus:bg-transparent focus:text-foreground data-highlighted:bg-transparent ${
        item.active ? "font-semibold text-foreground" : "text-foreground/70 focus:font-medium"
      }`}
    >
      <span className="flex items-center gap-2">
        {item.active ? (
          <span className="size-1.5 rounded-full bg-(--ok)" />
        ) : (
          <span className="size-1.5" />
        )}
        {item.builtin === true && (
          <Lock className="size-3 shrink-0 text-muted-foreground/50" aria-label="built-in" />
        )}
        {item.name || item.id}
      </span>
      <span className="flex items-center gap-1 opacity-0 transition-opacity group-hover/row:opacity-100">
        <RowButton title="Edit" onClick={stopAnd(onEdit)}>
          <Pencil className="size-3" />
        </RowButton>
        <RowButton title="Export .md" onClick={stopAnd(onExport)}>
          <Download className="size-3" />
        </RowButton>
        {showDelete && (
          <RowButton title="Delete" danger onClick={stopAnd(onDelete)}>
            <Trash2 className="size-3" />
          </RowButton>
        )}
      </span>
    </DropdownMenuItem>
  )
}

/** A tiny icon action button inside a behaviour row. */
function RowButton({
  title,
  danger,
  onClick,
  children,
}: {
  title: string
  danger?: boolean
  onClick: (e: React.MouseEvent) => void
  children: React.ReactNode
}) {
  return (
    <button
      type="button"
      title={title}
      onClick={onClick}
      className={`flex size-5 items-center justify-center rounded-sm transition-colors ${
        danger ? "text-muted-foreground/70 hover:text-(--danger)" : "text-muted-foreground/70 hover:text-foreground"
      }`}
    >
      {children}
    </button>
  )
}
