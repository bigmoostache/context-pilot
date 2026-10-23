import { Check, CircleAlert, LoaderCircle, X } from "lucide-react"
import { dismissUpload, useUploads, type UploadJob } from "@/lib/api/client/upload"

/** Compact byte formatter: "812 KB", "12.3 MB". */
function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`
  const units = ["KB", "MB", "GB", "TB"]
  let v = n / 1024
  let i = 0
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024
    i += 1
  }
  return `${v.toFixed(v < 10 ? 1 : 0)} ${units[i] ?? "TB"}`
}

/** Leading status glyph: spinner while sending, check when done, alert on error. */
const ICONS: Record<UploadJob["status"], React.ReactNode> = {
  uploading: <LoaderCircle className="size-4 animate-spin text-muted-foreground" />,
  done: <Check className="size-4 text-emerald-500" />,
  error: <CircleAlert className="size-4 text-destructive" />,
}

/** Thin bottom-edge bar colour per status. */
const BAR: Record<UploadJob["status"], string> = {
  uploading: "bg-primary",
  done: "bg-emerald-500",
  error: "bg-destructive",
}

function subtitle(job: UploadJob): string {
  if (job.status === "error") return job.error ?? "Upload failed"
  if (job.status === "done") return fmtBytes(job.total)
  return `${fmtBytes(job.loaded)} of ${fmtBytes(job.total)}`
}

function UploadToast({ job }: { job: UploadJob }) {
  const pct = job.total > 0 ? Math.min(100, Math.round((job.loaded / job.total) * 100)) : 0
  const width = job.status === "uploading" ? pct : 100
  return (
    <div className="group pointer-events-auto relative w-80 overflow-hidden rounded-xl bg-popover/95 text-popover-foreground shadow-lg ring-1 ring-foreground/5 backdrop-blur-sm">
      <div className="flex items-center gap-3 px-3.5 py-3">
        <span className="flex size-8 shrink-0 items-center justify-center rounded-full bg-muted">
          {ICONS[job.status]}
        </span>
        <div className="min-w-0 flex-1">
          <div className="truncate text-sm font-medium" title={job.name}>
            {job.name}
          </div>
          <div
            className={`truncate text-xs tabular-nums ${job.status === "error" ? "text-destructive" : "text-muted-foreground"}`}
          >
            {subtitle(job)}
          </div>
        </div>
        {job.status === "uploading" ? (
          <span className="text-xs font-medium text-muted-foreground tabular-nums">{pct}%</span>
        ) : (
          <button
            type="button"
            aria-label="Dismiss"
            className="rounded-md p-1 text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100 hover:bg-muted hover:text-foreground focus-visible:opacity-100"
            onClick={() => dismissUpload(job.id)}
          >
            <X className="size-3.5" />
          </button>
        )}
      </div>
      <div className="absolute inset-x-0 bottom-0 h-0.5 bg-muted">
        <div
          className={`h-full transition-[width] duration-300 ease-out ${BAR[job.status]}`}
          style={{ width: `${width}%` }}
        />
      </div>
    </div>
  )
}

/**
 * Bottom-right stack of upload progress toasts — one per file sent through the
 * single upload channel (`lib/api/client/upload.ts`). Mounted once in `App`,
 * above the desktop/mobile switch, so every surface on both trees shares it.
 */
export function UploadToasts() {
  const jobs = useUploads()
  if (jobs.length === 0) return null
  return (
    <div
      className="pointer-events-none fixed right-4 bottom-4 z-100 flex flex-col gap-2"
      role="status"
      aria-live="polite"
    >
      {jobs.map((job) => (
        <UploadToast key={job.id} job={job} />
      ))}
    </div>
  )
}
