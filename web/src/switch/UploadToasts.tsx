import { dismissUpload, useUploads, type UploadJob } from "@/lib/api/client/upload"

/** Compact byte formatter for the toast's "12.3 / 80 MB" line. */
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

function UploadToast({ job }: { job: UploadJob }) {
  const pct = job.total > 0 ? Math.min(100, Math.round((job.loaded / job.total) * 100)) : 0
  const bar =
    job.status === "error"
      ? "bg-destructive"
      : job.status === "done"
        ? "bg-emerald-500"
        : "bg-primary"
  const label =
    job.status === "error"
      ? (job.error ?? "failed")
      : job.status === "done"
        ? `Uploaded · ${fmtBytes(job.total)}`
        : `${fmtBytes(job.loaded)} / ${fmtBytes(job.total)} · ${pct}%`
  return (
    <div className="pointer-events-auto w-72 rounded-lg bg-popover p-3 text-popover-foreground shadow-lg">
      <div className="flex items-center gap-2">
        <span className="min-w-0 flex-1 truncate text-sm font-medium" title={job.name}>
          {job.name}
        </span>
        {job.status !== "uploading" && (
          <button
            type="button"
            aria-label="Dismiss"
            className="text-xs text-muted-foreground hover:text-foreground"
            onClick={() => dismissUpload(job.id)}
          >
            ✕
          </button>
        )}
      </div>
      <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-muted">
        <div
          className={`h-full transition-[width] duration-200 ${bar}`}
          style={{ width: `${job.status === "uploading" ? pct : 100}%` }}
        />
      </div>
      <div
        className={`mt-1.5 text-xs ${job.status === "error" ? "text-destructive" : "text-muted-foreground"}`}
      >
        {label}
      </div>
    </div>
  )
}

/**
 * Bottom-right stack of upload progress toasts — one per file sent through the
 * single upload channel (`lib/api/client/upload.ts`). Mounted once in `App`, above the
 * desktop/mobile switch, so every surface on both trees shares it.
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
