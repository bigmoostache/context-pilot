import { FolderTree, Folder, FolderOpen, File } from "lucide-react"
import type { ContextPanel } from "@/lib/types"
import { useTree } from "@/lib/live"
import { PanelFrame } from "../../PanelFrame"

/**
 * Directory Tree panel — the cartographer's annotated project tree.
 * Each row shows depth-indented name, a folder/file glyph, an optional token
 * size chip, and the cartographer description. A `[!]` marker flags files that
 * changed since their description was written (stale-doc signal).
 */
export function TreePanel({ panel, agentId }: { panel: ContextPanel; agentId: string }) {
  const { data: treeRows = [] } = useTree(agentId)
  return (
    <PanelFrame
      icon={FolderTree}
      name="Directory Tree"
      subtitle="Annotated project map · cartographer descriptions"
      tokens={panel.tokens}
      cost={panel.costUsd}
    >
      <ul className="font-mono text-[12px]">
        {treeRows.map((r, i) => {
          const Glyph = r.kind === "dir" ? (r.open ? FolderOpen : Folder) : File
          return (
            <li
              key={i}
              className="group flex items-start gap-2 rounded-md px-1.5 py-1 hover:bg-muted/50"
              style={{ paddingLeft: `${r.depth * 16 + 6}px` }}
            >
              <Glyph
                className="mt-0.5 size-3.5 shrink-0"
                style={{ color: r.kind === "dir" ? "var(--signal)" : "var(--muted-foreground)" }}
              />
              <div className="flex min-w-0 flex-1 flex-col">
                <span className="flex items-center gap-1.5">
                  <span className="truncate text-foreground/90">{r.name}</span>
                  {r.changed && (
                    <span className="text-[10px] font-bold text-[var(--warn)]" title="changed since description">
                      [!]
                    </span>
                  )}
                  {r.size && (
                    <span className="text-[10px] tabular-nums text-muted-foreground/60">{r.size}</span>
                  )}
                </span>
                {r.desc && (
                  <span className="truncate font-sans text-[11px] text-muted-foreground/75">
                    {r.desc}
                  </span>
                )}
              </div>
            </li>
          )
        })}
      </ul>
    </PanelFrame>
  )
}
