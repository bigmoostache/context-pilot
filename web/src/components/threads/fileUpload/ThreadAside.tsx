import { useMemo } from "react"
import { Paperclip, ListChecks, ChevronLeft, Download } from "lucide-react"
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs"
import { TooltipProvider } from "@/components/ui/tooltip"
import { FileIcon } from "@/components/finder/support/macIcons"
import { kindOf } from "@/components/finder/support/kind"
import { FinderPreview } from "@/components/finder/preview/FinderPreview"
import { downloadFile } from "@/lib/api"
import { uploadToNode, type UploadedFile } from "./helpers"
import type { ThreadFile } from "./FileSidebar"
import { TaskList } from "./ThreadAsideTasks"
import type { ThreadTask } from "@/lib/types"

/**
 * Rail width strategy (dynamic): in LIST mode the rail sizes to its content
 * (`w-fit`) — as wide as its longest task/file line — floored at a sensible
 * minimum and capped at 2/5 of the viewport (`max-w-[40vw]`), beyond which the
 * content wraps. In PREVIEW mode (a file open) the rail takes the full 40vw cap
 * so the embedded {@link FinderPreview} has room.
 */
const RAIL_MAX = "max-w-[40vw]"

/**
 * The unified right-rail aside for a thread conversation (T662) — a single
 * always-visible rail with two tabs, **Tasks** and **Files**, replacing the two
 * separate {@link FileSidebar} + {@link TodoSidebar} rails.
 *
 * In the Files tab, clicking a file swaps the list for an inline
 * {@link FinderPreview} and widens the rail to the {@link RAIL_MAX} cap — so the
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

  return (
    <div
      className={
        "mt-2 mr-2 flex shrink-0 flex-col overflow-hidden border-l border-border/70 transition-[width,max-width] duration-300 ease-[cubic-bezier(.16,1,.3,1)] motion-reduce:transition-none " +
        (previewing ? `w-[40vw] ${RAIL_MAX}` : `w-fit min-w-60 ${RAIL_MAX}`)
      }
    >
      <TooltipProvider>
          <Tabs
            value={activeTab}
            onValueChange={(v) => onTabChange(v as "files" | "tasks")}
            className="flex min-h-0 flex-1 flex-col gap-0"
          >
            {/* Header: the single always-visible Tasks/Files tab bar. While a
                file preview is open it is ENRICHED with right-aligned Download +
                Back(return-to-files) controls — so the aside keeps exactly ONE
                header and the preview's own Quick Look bar is suppressed (its
                FinderPreview renders with variant="full"). */}
            <AsideTabBar
              hasTasks={hasTasks}
              hasFiles={hasFiles}
              previewing={previewing}
              agentId={agentId}
              file={selectedFile}
              onBack={() => onSelectFile(null)}
            />

            {/* Tasks tab */}
            {hasTasks && (
              <TabsContent value="tasks" className="min-h-0 flex-1 overflow-y-auto">
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
  )
}

/** The single header row: the left-aligned Tasks/Files tab selector, plus —
 *  while a file preview is open — a right-aligned Download + Back(return to
 *  files) control group, styled to match the tab triggers (icon+text, same
 *  colour, hover changes text colour only, no background). Extracted so its
 *  per-tab presence branches live outside {@link ThreadAside} (keeping that
 *  function under the cyclomatic-complexity budget). The controls replace
 *  {@link FinderPreview}'s own Quick Look header, which is suppressed by
 *  rendering the preview with variant="full". */
function AsideTabBar({
  hasTasks,
  hasFiles,
  previewing,
  agentId,
  file,
  onBack,
}: {
  hasTasks: boolean
  hasFiles: boolean
  previewing: boolean
  agentId: string
  file: UploadedFile | null
  onBack: () => void
}) {
  return (
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
      {previewing && (
        <div className="ml-auto flex items-center gap-0.5 pr-1.5">
          {file && (
            <button
              type="button"
              onClick={() => void downloadFile(agentId, file.path)}
              className="inline-flex items-center gap-1.5 px-2 text-[11px] font-medium text-foreground/60 transition-colors hover:text-foreground"
            >
              <Download className="size-3" />
              Download
            </button>
          )}
          <button
            type="button"
            onClick={onBack}
            className="inline-flex items-center gap-1.5 px-2 text-[11px] font-medium text-foreground/60 transition-colors hover:text-foreground"
          >
            <ChevronLeft className="size-3" />
            Back
          </button>
        </div>
      )}
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
          className="group flex w-full items-center gap-2.5 rounded-md px-2 py-1.5 text-left transition-colors hover:bg-muted/40"
        >
          <span className="shrink-0">
            <FileIcon kind={kindOf(f.file.name)} size={28} />
          </span>
          <span className="flex min-w-0 flex-1 flex-col leading-tight">
            <span className="truncate text-[13.5px] text-foreground/85 group-hover:text-foreground">
              {f.file.name}
            </span>
            <span className="text-[11px] text-muted-foreground/55">
              {f.role === "user" ? "You" : "Assistant"}
            </span>
          </span>
        </button>
      ))}
    </div>
  )
}

/** Inline file preview in the Files tab — the shared {@link FinderPreview} pane
 *  rendered with variant="full" so it draws NO Quick Look header of its own. The
 *  aside's single header is the enriched {@link AsideTabBar} (which carries the
 *  Download + Close controls while previewing); Close there returns to the file
 *  list. `onClose` is wired to `onBack` as a harmless fallback. */
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
    <div className="min-h-0 flex-1 overflow-hidden">
      <FinderPreview node={uploadToNode(file)} agentId={agentId} variant="full" onClose={onBack} />
    </div>
  )
}
