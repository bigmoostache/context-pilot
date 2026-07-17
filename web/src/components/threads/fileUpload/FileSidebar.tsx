import { useMemo } from "react"
import { Paperclip } from "lucide-react"
import { FileIcon } from "@/components/finder/support/macIcons"
import { kindOf } from "@/components/finder/support/kind"
import type { UploadedFile } from "./helpers"

/** A file attachment extracted from a thread message, tagged with the sender's role. */
export interface ThreadFile {
  file: UploadedFile
  role: string
}

/**
 * Right-rail sidebar listing every file attachment in the thread. Stays visible
 * while the conversation scrolls so the user can re-open any document without
 * hunting for it in the message history.
 *
 * Only rendered when the thread contains at least one `file-upload` block.
 * A sticky header labels the rail ("Attachments · N"); each row is an icon-well
 * button that opens the shared Quick Look drawer (same `onOpenFile` path).
 */
export function FileSidebar({
  files,
  onOpen,
}: {
  files: ThreadFile[]
  onOpen: (file: UploadedFile) => void
}) {
  // Dedupe by file path — same file attached multiple times shows once.
  const unique = useMemo(() => {
    const seen = new Set<string>()
    return files.filter((f) => {
      if (seen.has(f.file.path)) return false
      seen.add(f.file.path)
      return true
    })
  }, [files])

  return (
    <aside className="flex w-[210px] shrink-0 flex-col border-l border-border/70 bg-muted/10">
      <div className="flex items-center gap-1.5 border-b border-border/60 px-3 py-2">
        <Paperclip className="size-3 text-muted-foreground/55" />
        <span className="text-[10.5px] font-semibold tracking-wide text-muted-foreground/65 uppercase">
          Attachments
        </span>
        <span className="ml-auto rounded-full bg-muted/60 px-1.5 py-px text-[10px] font-medium text-muted-foreground/70 tabular-nums">
          {unique.length}
        </span>
      </div>
      <div className="flex-1 space-y-0.5 overflow-y-auto p-1.5">
        {unique.map((f) => (
          <button
            key={f.file.path}
            type="button"
            onClick={() => onOpen(f.file)}
            className="group hover:card-shadow flex w-full items-center gap-2.5 rounded-lg px-2 py-1.5 text-left transition-colors hover:bg-card"
          >
            <span className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-muted/50 transition-colors group-hover:bg-muted/70">
              <FileIcon kind={kindOf(f.file.name)} size={20} />
            </span>
            <span className="flex min-w-0 flex-1 flex-col leading-tight">
              <span className="truncate text-[11.5px] font-medium text-foreground/85 group-hover:text-foreground">
                {f.file.name}
              </span>
              <span className="text-[9.5px] text-muted-foreground/50">
                {f.role === "user" ? "You" : "Assistant"}
              </span>
            </span>
          </button>
        ))}
      </div>
    </aside>
  )
}
