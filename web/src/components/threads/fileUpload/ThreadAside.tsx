import { useMemo } from "react"
import { Paperclip, ListChecks, ChevronLeft, ChevronRight } from "lucide-react"
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs"
import { TooltipProvider } from "@/components/ui/tooltip"
import { FileIcon } from "@/components/finder/support/macIcons"
import { kindOf } from "@/components/finder/support/kind"
import { FinderPreview } from "@/components/finder/preview/FinderPreview"
import { uploadToNode, type UploadedFile } from "./helpers"
import type { ThreadFile } from "./FileSidebar"
import { TaskList } from "./ThreadAsideTasks"
import type { ThreadTask } from "@/lib/types"

/** Rail width (px) while browsing the list; widens when a file preview is open. */
const LIST_WIDTH = 440
const PREVIEW_WIDTH = 680

/**
 * The unified right-rail aside for a thread conversation (T662) — a single
 * always-visible rail with two tabs, **Tasks** and **Files**, replacing the two
 * separate {@link FileSidebar} + {@link TodoSidebar} rails.
 *
 * In the Files tab, clicking a file swaps the list for an inline
 * {@link FinderPreview} and widens the rail to {@link PREVIEW_WIDTH} — so the
 * drawer that previously popped over the whole conversation is gone (the Finder
 * still uses that drawer; threads no longer do).
 *
 * Selection + tab state is **controlled** by the parent so an in-message file
 * chip can drive the same rail (switch to Files, show the preview) instead of a
 * separate drawer.
 */
export function ThreadAside({
  files,
  tasks,
  agentId,
  tab,
  onTabChange,
  selectedFile,
  onSelectFile,
}: {
  files: ThreadFile[]
  tasks: ThreadTask[]
  agentId: string
  tab: "files" | "tasks"
  onTabChange: (tab: "files" | "tasks") => void
  selectedFile: UploadedFile | null
  onSelectFile: (file: UploadedFile | null) => void
}) {
  // Per-section presence gates (T666): a tab is shown only when its section has
  // content, and the whole rail disappears when BOTH are empty (the conversation
  // then takes the full width).
  const hasFiles = files.length > 0
  const hasTasks = tasks.length > 0
  if (!hasFiles && !hasTasks) return null

  // Clamp the active tab to a VISIBLE one: keep the requested tab when its
  // section still has content, else fall back to whichever tab remains. Prevents
  // landing on a now-hidden tab after switching to a thread that lacks it.
  const activeTab: "files" | "tasks" =
    (tab === "files" && hasFiles) || (tab === "tasks" && hasTasks)
      ? tab
      : hasTasks
        ? "tasks"
        : "files"
  const previewing = activeTab === "files" && selectedFile !== null
  const width = previewing ? PREVIEW_WIDTH : LIST_WIDTH

  return (
    <div
      className="mx-2 mt-2 shrink-0 overflow-hidden border-l border-border/70 transition-[width] duration-300 ease-[cubic-bezier(.16,1,.3,1)] motion-reduce:transition-none"
      style={{ width }}
    >
      <div className="flex h-full flex-col" style={{ width }}>
        <TooltipProvider>
          <Tabs
            value={activeTab}
            onValueChange={(v) => onTabChange(v as "files" | "tasks")}
            className="flex min-h-0 flex-1 flex-col gap-0"
          >
            {/* Header: left-aligned tab bar. */}
            <div className="flex items-center gap-1.5 border-b border-border/60">
              <TabsList variant="line" className="h-7 gap-0.5">
                {hasTasks && (
                  <TabsTrigger value="tasks" className="px-2 text-[11px]">
                    <ListChecks className="size-3" />
                    Tasks
                  </TabsTrigger>
                )}
                {hasFiles && (
                  <TabsTrigger value="files" className="px-2 text-[11px]">
                    <Paperclip className="size-3" />
                    Files
                  </TabsTrigger>
                )}
              </TabsList>
            </div>

            {/* Tasks tab */}
            {hasTasks && (
              <TabsContent value="tasks" className="min-h-0 flex-1 overflow-y-auto p-1.5">
                <TaskList tasks={tasks} />
              </TabsContent>
            )}

            {/* Files tab — list, or inline preview when a file is selected */}
            {hasFiles && (
              <TabsContent value="files" className="flex min-h-0 flex-1 flex-col">
                {selectedFile ? (
                  <InlineFilePreview
                    file={selectedFile}
                    agentId={agentId}
                    onBack={() => onSelectFile(null)}
                  />
                ) : (
                  <FileList files={files} onSelect={onSelectFile} />
                )}
              </TabsContent>
            )}
          </Tabs>
        </TooltipProvider>
      </div>
    </div>
  )
}

/** The de-duplicated attachment list (rows open the inline preview). */
function FileList({
  files,
  onSelect,
}: {
  files: ThreadFile[]
  onSelect: (file: UploadedFile) => void
}) {
  const unique = useMemo(() => {
    const seen = new Set<string>()
    return files.filter((f) => {
      if (seen.has(f.file.path)) return false
      seen.add(f.file.path)
      return true
    })
  }, [files])

  if (unique.length === 0) {
    return (
      <div className="flex flex-1 items-center justify-center p-4 text-center text-[11px] text-muted-foreground/45">
        No attachments in this thread yet.
      </div>
    )
  }

  return (
    <div className="flex-1 space-y-0.5 overflow-y-auto p-1.5">
      {unique.map((f) => (
        <button
          key={f.file.path}
          type="button"
          onClick={() => onSelect(f.file)}
          className="group flex w-full items-center gap-2.5 rounded-lg px-2 py-1.5 text-left"
        >
          <span className="flex size-8 shrink-0 items-center justify-center rounded-lg bg-muted/50">
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
          <ChevronRight className="size-3.5 shrink-0 text-muted-foreground/30 group-hover:text-muted-foreground/60" />
        </button>
      ))}
    </div>
  )
}

/** Inline file preview in the Files tab — a slim back bar over the shared
 *  {@link FinderPreview} pane (the same renderer the Finder uses). */
function InlineFilePreview({
  file,
  agentId,
  onBack,
}: {
  file: UploadedFile
  agentId: string
  onBack: () => void
}) {
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <button
        type="button"
        onClick={onBack}
        className="flex shrink-0 items-center gap-1 border-b border-border/60 px-2 py-1.5 text-[11px] font-medium text-muted-foreground/70 transition-colors hover:text-foreground"
      >
        <ChevronLeft className="size-3.5" />
        Back to files
      </button>
      <div className="min-h-0 flex-1 overflow-hidden">
        <FinderPreview node={uploadToNode(file)} agentId={agentId} variant="pane" onClose={onBack} />
      </div>
    </div>
  )
}
