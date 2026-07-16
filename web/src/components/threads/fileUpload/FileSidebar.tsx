import { useMemo } from "react"
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
 * Clicking a chip opens the shared Quick Look drawer (same `onOpenFile` path).
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
    <aside className="flex w-[200px] shrink-0 flex-col border-l border-border">
      <div className="flex-1 space-y-0.5 overflow-y-auto p-1.5">
        {unique.map((f) => (
          <button
            key={f.file.path}
            type="button"
            onClick={() => onOpen(f.file)}
            className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left transition-colors hover:bg-muted/50"
          >
            <FileIcon kind={kindOf(f.file.name)} size={20} />
            <div className="min-w-0 flex-1">
              <div className="truncate text-[11.5px] font-medium text-foreground/80">
                {f.file.name}
              </div>
              <div className="text-[9.5px] text-muted-foreground/50">
                {f.role === "user" ? "You" : "Assistant"}
              </div>
            </div>
          </button>
        ))}
      </div>
    </aside>
  )
}
