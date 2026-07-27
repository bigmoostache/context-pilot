import { useState } from "react"
import { MessagesSquare, Paperclip, X } from "lucide-react"
import { Dialog, DialogContent, DialogTitle, DialogDescription } from "@/components/ui/dialog"
import type { CreateThreadOpts } from "@/lib/live/threadView"

/**
 * New Thread dialog (T674) — the rich thread-starter. Collects a **title**, an
 * optional **first message** (auto-sent right after creation), **file
 * attachments** (uploaded + folded into that first message), and a **create
 * paused** toggle (queues the seeded message without waking the agent). On
 * submit the parent's {@link CreateThreadOpts} handler uploads + dispatches a
 * single `create_thread` command; the new thread is auto-selected once its
 * server id arrives. Built on the portaled Base UI Dialog primitive (focus-trap,
 * scroll-lock, Esc-to-close, backdrop fade + spring-in).
 */
export function NewThreadDialog({
  open,
  onClose,
  onCreate,
}: {
  open: boolean
  onClose: () => void
  onCreate: (opts: CreateThreadOpts) => void
}) {
  const [title, setTitle] = useState("")
  const [firstMessage, setFirstMessage] = useState("")
  const [files, setFiles] = useState<File[]>([])
  const [paused, setPaused] = useState(false)
  const canCreate = title.trim().length > 0

  const reset = () => {
    setTitle("")
    setFirstMessage("")
    setFiles([])
    setPaused(false)
  }
  const close = () => {
    reset()
    onClose()
  }

  const submit = (e: React.SyntheticEvent) => {
    e.preventDefault()
    if (!canCreate) return
    onCreate({ title, firstMessage, files, paused })
    reset()
  }

  const addFiles = (list: FileList | null) => {
    if (!list || list.length === 0) return
    setFiles((prev) => [...prev, ...list])
  }

  return (
    <Dialog open={open} onOpenChange={(o) => !o && close()}>
      <DialogContent className="w-[460px] max-w-[92vw] p-5">
        <form onSubmit={submit} className="flex flex-col gap-4">
          <div className="flex items-center gap-3">
            <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-(--signal)/15 text-(--signal)">
              <MessagesSquare className="size-[18px]" />
            </span>
            <div className="flex flex-col">
              <DialogTitle>New Thread</DialogTitle>
              <DialogDescription>
                Give it a title and, if you like, a first message to send right away.
              </DialogDescription>
            </div>
          </div>

          <input
            autoFocus
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            placeholder="e.g. Refactor the cache engine"
            className="w-full rounded-lg border border-border bg-card px-3 py-2 text-[13px] text-foreground/90 outline-none placeholder:text-muted-foreground/50 focus:border-(--signal)/60"
          />

          <textarea
            value={firstMessage}
            onChange={(e) => setFirstMessage(e.target.value)}
            placeholder="First message (optional) — sent automatically once the thread opens"
            rows={4}
            className="w-full resize-none rounded-lg border border-border bg-card px-3 py-2 text-[13px] text-foreground/90 outline-none placeholder:text-muted-foreground/50 focus:border-(--signal)/60"
          />

          <FileRow
            files={files}
            onAdd={addFiles}
            onRemove={(i) => setFiles((p) => p.filter((_, idx) => idx !== i))}
          />

          <label className="flex items-center gap-2.5 text-[12.5px] text-foreground/80 select-none">
            <input
              type="checkbox"
              checked={paused}
              onChange={(e) => setPaused(e.target.checked)}
              className="size-3.5 accent-(--signal)"
            />
            Create paused — queue the first message without waking the agent
          </label>

          <div className="flex items-center justify-end gap-2">
            <button
              type="button"
              onClick={close}
              className="rounded-lg px-3 py-1.5 text-[12.5px] font-medium text-muted-foreground transition-colors hover:bg-muted/60 hover:text-foreground"
            >
              Cancel
            </button>
            <button
              type="submit"
              disabled={!canCreate}
              className="rounded-lg bg-(--signal) px-3.5 py-1.5 text-[12.5px] font-medium text-(--primary-foreground) transition-[filter] hover:brightness-105 disabled:cursor-not-allowed disabled:opacity-50"
            >
              Create thread
            </button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  )
}

/** Attachment row — an "attach" button plus a chip per staged file (removable).
 *  Files upload only on submit, so this is pure local staging. */
function FileRow({
  files,
  onAdd,
  onRemove,
}: {
  files: File[]
  onAdd: (list: FileList | null) => void
  onRemove: (index: number) => void
}) {
  return (
    <div className="flex flex-col gap-2">
      <label className="flex w-fit cursor-pointer items-center gap-1.5 rounded-lg border border-border bg-card px-2.5 py-1.5 text-[12px] font-medium text-muted-foreground transition-colors hover:border-(--signal)/50 hover:text-foreground">
        <Paperclip className="size-3.5" />
        Attach files
        <input type="file" multiple className="hidden" onChange={(e) => onAdd(e.target.files)} />
      </label>
      {files.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {files.map((f, i) => (
            <span
              key={`${f.name}-${i}`}
              className="flex items-center gap-1.5 rounded-md border border-border bg-muted/50 px-2 py-1 text-[11px] text-foreground/80"
            >
              <span className="max-w-[160px] truncate">{f.name}</span>
              <button
                type="button"
                onClick={() => onRemove(i)}
                className="text-muted-foreground/60 transition-colors hover:text-(--danger)"
                aria-label={`Remove ${f.name}`}
              >
                <X className="size-3" />
              </button>
            </span>
          ))}
        </div>
      )}
    </div>
  )
}
