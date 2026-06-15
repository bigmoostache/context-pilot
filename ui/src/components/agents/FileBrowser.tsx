import { useState } from "react"
import {
  ChevronRight,
  File as FileIcon,
  FileCode,
  Folder,
  FolderGit2,
  FolderOpen,
} from "lucide-react"
import { ScrollArea } from "@/components/ui/scroll-area"
import { agents } from "@/lib/mock"
import { accentVar } from "@/lib/panelMeta"
import type { FsNode } from "@/lib/types"
import { cn } from "@/lib/utils"

/**
 * Left pane of the Agents launcher: a navigable filesystem tree. Folders that
 * host an agent are badged with the agent's accent + a small dot; selecting any
 * node drives the detail pane (open agent, or initialize one in a plain folder).
 */
export function FileBrowser({
  root,
  selectedPath,
  onSelect,
}: {
  root: FsNode
  selectedPath: string
  onSelect: (node: FsNode) => void
}) {
  return (
    <aside className="flex w-[320px] shrink-0 flex-col border-r border-border bg-surface">
      <div className="flex h-11 shrink-0 items-center gap-2 px-4">
        <FolderOpen className="size-4 text-muted-foreground" />
        <span className="text-[12.5px] font-semibold text-foreground/85">Filesystem</span>
        <span className="ml-auto font-mono text-[11px] text-muted-foreground/60">{root.path}</span>
      </div>
      <div className="h-px bg-border/60" />
      <ScrollArea className="min-h-0 flex-1">
        <div className="px-2 py-2">
          {(root.children ?? []).map((child) => (
            <TreeRow
              key={child.path}
              node={child}
              depth={0}
              selectedPath={selectedPath}
              onSelect={onSelect}
            />
          ))}
        </div>
      </ScrollArea>
    </aside>
  )
}

function TreeRow({
  node,
  depth,
  selectedPath,
  onSelect,
}: {
  node: FsNode
  depth: number
  selectedPath: string
  onSelect: (node: FsNode) => void
}) {
  const isDir = node.kind === "dir"
  const [open, setOpen] = useState(depth === 0)
  const selected = node.path === selectedPath
  const agent = node.agentId ? agents.find((a) => a.id === node.agentId) : undefined

  const Icon = isDir ? (open ? FolderOpen : Folder) : node.name.endsWith(".lean") || node.name.endsWith(".rs") || node.name.endsWith(".ts") ? FileCode : FileIcon

  return (
    <div>
      <button
        type="button"
        onClick={() => {
          onSelect(node)
          if (isDir) setOpen((o) => !o)
        }}
        className={cn(
          "group flex w-full items-center gap-1.5 rounded-md py-1 pr-2 text-left transition-colors",
          selected ? "bg-card card-shadow" : "hover:bg-muted/60",
        )}
        style={{ paddingLeft: `${depth * 14 + 6}px` }}
      >
        {isDir ? (
          <ChevronRight
            className={cn(
              "size-3.5 shrink-0 text-muted-foreground/50 transition-transform",
              open && "rotate-90",
            )}
          />
        ) : (
          <span className="w-3.5 shrink-0" />
        )}

        {agent ? (
          <FolderGit2 className="size-4 shrink-0" style={{ color: accentVar[agent.accent] }} />
        ) : (
          <Icon
            className={cn("size-4 shrink-0", isDir ? "text-[var(--warn)]/80" : "text-muted-foreground/70")}
          />
        )}

        <span
          className={cn(
            "min-w-0 flex-1 truncate text-[12.5px]",
            selected ? "text-foreground" : "text-foreground/80",
          )}
        >
          {node.name}
        </span>

        {agent && (
          <span
            className="shrink-0 rounded-full px-1.5 py-px text-[9.5px] font-semibold"
            style={{
              background: `color-mix(in oklab, ${accentVar[agent.accent]} 16%, transparent)`,
              color: accentVar[agent.accent],
            }}
          >
            agent
          </span>
        )}
      </button>

      {isDir && open && node.children && node.children.length > 0 && (
        <div>
          {node.children.map((child) => (
            <TreeRow
              key={child.path}
              node={child}
              depth={depth + 1}
              selectedPath={selectedPath}
              onSelect={onSelect}
            />
          ))}
        </div>
      )}
    </div>
  )
}
