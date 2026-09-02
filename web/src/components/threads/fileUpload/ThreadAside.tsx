import { useMemo } from "react"
import { Paperclip, ListChecks, ChevronLeft, Download, PanelRightClose } from "lucide-react"
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs"
import { TooltipProvider } from "@/components/ui/tooltip"
import { FileIcon } from "@/components/finder/support/macIcons"
import { kindOf } from "@/components/finder/support/kind"
import { FinderPreview } from "@/components/finder/preview/FinderPreview"
import { HintBadge } from "@/components/shell/chrome/HintBadge"
import { downloadFile } from "@/lib/api"
import { uploadToNode, type UploadedFile } from "./helpers"
import type { ThreadFile } from "./FileSidebar"
import { TaskList } from "./ThreadAsideTasks"
import type { ThreadTask } from "@/lib/types"

/**
 * Rail width strategy (dynamic): in LIST mode the rail is EXACTLY the width of
 * the thread sidebar (`w-(--sidebar-w)`, the shared `--sidebar-w` token every
 * top-level sidebar reads) so the two rails frame the conversation symmetrically
 * (T676). In PREVIEW mode (a file open) it widens so the embedded
 * {@link FinderPreview} has room — normally to 40vw, but to **half the viewport
 * (50vw)** when the left thread-list rail is hidden (T680), since the reclaimed
 * left space lets the preview breathe. The `transition-[width,max-width]`
 * animates between all three.
 */

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
  leftRailHidden,
  hintShown = false,
  onHide,
}: {
  files: ThreadFile[]
  tasks: ThreadTask[]
  agentId: string
  tab: "files" | "tasks"
  onTabChange: (tab: "files" | "tasks") => void
  selectedFile: UploadedFile | null
  onSelectFile: (file: UploadedFile | null) => void
  /** Whether the left thread-list rail is hidden. When it is AND a file is
   *  previewing, the aside widens to half the viewport (50vw) instead of 40vw,
   *  claiming the space the collapsed left rail freed up (T680). */
  leftRailHidden: boolean
  /** Whether ⌘/Ctrl is currently held (T688) — reveals the "H" hint badge on the
   *  tab bar's hide button, mirroring the header rail's shortcut affordance. */
  hintShown?: boolean
  /** Hide the whole rail for this thread (T677) — the tab bar's right-aligned
   *  hide button. Re-showing is driven by the parent's floating reopen button. */
  onHide: () => void
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
        "card-shadow my-2 mr-2 flex shrink-0 flex-col overflow-hidden border-l border-border/70 bg-surface-2 transition-[width,max-width] duration-300 ease-[cubic-bezier(.16,1,.3,1)] motion-reduce:transition-none " +
        (previewing
          ? leftRailHidden
            ? "w-[50vw] max-w-[50vw]"
            : "w-[40vw] max-w-[40vw]"
          : "w-(--sidebar-w)")
      }
    >
      <TooltipProvider>
        <Tabs
          value={activeTab}
          onValueChange={(v) => onTabChange(v as "files" | "tasks")}
          className="flex min-h-0 flex-1 flex-col gap-0 p-1"
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
            onHide={onHide}
            hintShown={hintShown}
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

/** The single header row: the left-aligned Tasks/Files tab selector, then a
 *  right-aligned control cluster. That cluster always ends with an icon-only
 *  Hide button (T677 — collapses the rail; the parent renders the floating
 *  reopen affordance), and while a file preview is open it is PREFIXED with the
 *  Download + Back(return to files) controls, styled to match the tab triggers
 *  (icon+text, colour-only hover, no background). Extracted so its per-tab
 *  presence branches live outside {@link ThreadAside} (keeping that function
 *  under the cyclomatic-complexity budget). The preview controls replace
 *  {@link FinderPreview}'s own Quick Look header, which is suppressed by
 *  rendering the preview with variant="full". */
function AsideTabBar({
  hasTasks,
  hasFiles,
  previewing,
  agentId,
  file,
  onBack,
  onHide,
  hintShown,
}: {
  hasTasks: boolean
  hasFiles: boolean
  previewing: boolean
  agentId: string
  file: UploadedFile | null
  onBack: () => void
  onHide: () => void
  hintShown: boolean
}) {
  return (
    <div className="flex items-center gap-1.5 border-b border-border/60">
      {/* p-0 kills the TabsList primitive's base p-[3px]; border-0 kills each
          trigger's 1px transparent border. Both are removed so the FIRST tab's
          icon starts at exactly the trigger's px-2 (8px) inset — the SAME 8px a
          depth-0 task row's status icon sits at — so the tab strip and the task
          list share one icon column (T685 alignment). */}
      <TabsList variant="line" className="h-8 gap-0.5 p-0">
        {hasTasks && (
          <TabsTrigger value="tasks" className="border-0 px-2 text-[13.5px]">
            <ListChecks className="size-3.5" />
            Tasks
          </TabsTrigger>
        )}
        {hasFiles && (
          <TabsTrigger value="files" className="border-0 px-2 text-[13.5px]">
            <Paperclip className="size-3.5" />
            Files
          </TabsTrigger>
        )}
      </TabsList>
      {/* Right cluster: the preview opt-in controls (Download + Back), then the
          always-present hide button pinned to the far right — so hiding stays
          to the right of the file controls exactly as asked (T677). */}
      <div className="ml-auto flex items-center gap-0.5 pr-1.5">
        {previewing && file && (
          <button
            type="button"
            onClick={() => void downloadFile(agentId, file.path)}
            className="inline-flex items-center gap-1.5 px-2 text-[13.5px] font-medium text-foreground/60 transition-colors hover:text-foreground"
          >
            <Download className="size-3.5" />
            Download
          </button>
        )}
        {previewing && (
          <button
            type="button"
            onClick={onBack}
            className="inline-flex items-center gap-1.5 px-2 text-[13.5px] font-medium text-foreground/60 transition-colors hover:text-foreground"
          >
            <ChevronLeft className="size-3.5" />
            Back
          </button>
        )}
        <button
          type="button"
          onClick={onHide}
          aria-label="Hide details rail"
          title="Hide"
          className="relative inline-flex items-center px-1 text-foreground/60 transition-colors hover:text-foreground"
        >
          <PanelRightClose className="size-3.5" />
          <HintBadge label="H" shown={hintShown} />
        </button>
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
          className="group flex w-full items-center gap-2.5 rounded-md px-2 py-1.5 text-left"
        >
          <span className="shrink-0">
            <FileIcon kind={kindOf(f.file.name)} size={28} />
          </span>
          <span className="flex min-w-0 flex-1 flex-col leading-tight">
            <span className="truncate text-[13.5px] text-foreground/85 transition-colors group-hover:text-foreground">
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
    // MUST be a flex COLUMN: the embedded FinderPreview (variant="full") sizes
    // itself with `flex-1`/`min-h-0`, which only resolves to a bounded height
    // inside a flex-column parent. A plain block here let the preview grow to
    // content height, so its inner `overflow-y-auto` scroll containers never
    // overflowed and the excess was silently clipped by `overflow-hidden` —
    // the "can't scroll the md file viewer" bug (T673).
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      <FinderPreview node={uploadToNode(file)} agentId={agentId} variant="full" onClose={onBack} />
    </div>
  )
}
