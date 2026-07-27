import { useState } from "react"
import { Dialog as DialogPrimitive } from "@base-ui/react/dialog"
import { Paperclip, Pause, X, CornerDownLeft } from "lucide-react"
import type { CreateThreadOpts } from "@/lib/live/threadView"
import { cn } from "@/lib/utils"

/**
 * New Thread dialog (T674) — a Linear-"new issue"-style composer.
 *
 * Layout mirrors Linear's fast-create popup: a large borderless **title** line,
 * a seamless **first message** editor beneath it (auto-sent on create), then a
 * chip toolbar (attach + "start paused") and a footer with the primary action.
 * Attachments are staged locally and only uploaded on submit by the parent's
 * {@link CreateThreadOpts} handler, which folds them into the first message and
 * dispatches a single `create_thread` command.
 *
 * Built directly on Base UI's Dialog primitive (Portal escapes the vibrancy
 * containing-block, plus focus-trap / scroll-lock / Esc for free). Motion is
 * bespoke: the surface is anchored near the top (no vertical-centering
 * transform to fight) and springs in via `sheet-pop-in` / out via
 * `sheet-pop-out`, so open *and* close feel smooth. `⌘/Ctrl+Enter` submits.
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

  // ⌘/Ctrl+Enter submits from anywhere in the form (Linear parity).
  const onKeyDown = (e: React.KeyboardEvent) => {
    if ((e.metaKey || e.ctrlKey) && e.key === "Enter") submit(e)
  }

  const addFiles = (list: FileList | null) => {
    if (!list || list.length === 0) return
    setFiles((prev) => [...prev, ...list])
  }

  return (
    <DialogPrimitive.Root open={open} onOpenChange={(o) => !o && close()}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Backdrop
          className={cn(
            "fixed inset-0 z-50 bg-black/40 backdrop-blur-[3px] transition-opacity duration-200",
            "data-ending-style:opacity-0 data-starting-style:opacity-0",
          )}
        />
        <DialogPrimitive.Popup
          onKeyDown={onKeyDown}
          className={cn(
            "fixed top-[11vh] left-1/2 z-50 w-[560px] max-w-[92vw] -translate-x-1/2",
            "overflow-hidden rounded-xl border border-border bg-popover text-popover-foreground",
            "shadow-(--shadow-pop) outline-none",
            "animate-[sheet-pop-in_.22s_cubic-bezier(.16,1,.3,1)]",
            "data-ending-style:animate-[sheet-pop-out_.15s_ease-in_forwards]",
          )}
        >
          <form onSubmit={submit} className="flex flex-col">
            {/* breadcrumb + close, Linear's popup chrome */}
            <div className="flex items-center gap-2 px-4 pt-3 pb-1">
              <span className="flex items-center gap-1.5 text-[12px] text-muted-foreground">
                <span className="flex size-[15px] items-center justify-center rounded-sm bg-(--signal)/15 text-[9px] font-bold text-(--signal)">
                  T
                </span>
                <span className="text-muted-foreground/50">›</span>
                <span className="text-foreground/70">New thread</span>
              </span>
              <button
                type="button"
                onClick={close}
                aria-label="Close"
                className="ml-auto flex size-6 items-center justify-center rounded-md text-muted-foreground/60 transition-colors hover:bg-muted hover:text-foreground"
              >
                <X className="size-3.5" />
              </button>
            </div>

            {/* title — big, borderless, Linear issue-title feel */}
            <input
              autoFocus
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              placeholder="Thread title"
              className="w-full bg-transparent px-4 pt-1 text-[19px] font-semibold tracking-tight text-foreground outline-none placeholder:text-muted-foreground/40"
            />

            {/* first message — seamless editor under the title */}
            <textarea
              value={firstMessage}
              onChange={(e) => setFirstMessage(e.target.value)}
              placeholder="Add a first message… (sent automatically)"
              rows={4}
              className="w-full resize-none bg-transparent px-4 pt-2 pb-3 text-[13.5px] leading-relaxed text-foreground/90 outline-none placeholder:text-muted-foreground/40"
            />

            <FileChips
              files={files}
              onRemove={(i) => setFiles((p) => p.filter((_, idx) => idx !== i))}
            />

            {/* chip toolbar — Linear action-row: attach + paused */}
            <div className="flex items-center gap-1.5 px-3.5 py-2">
              <label className="flex cursor-pointer items-center gap-1.5 rounded-md border border-border px-2 py-1 text-[12px] font-medium text-muted-foreground transition-colors hover:border-(--signal)/50 hover:text-foreground">
                <Paperclip className="size-3.5" />
                Attach
                <input
                  type="file"
                  multiple
                  className="hidden"
                  onChange={(e) => addFiles(e.target.files)}
                />
              </label>
              <button
                type="button"
                onClick={() => setPaused((p) => !p)}
                aria-pressed={paused}
                className={cn(
                  "flex items-center gap-1.5 rounded-md border px-2 py-1 text-[12px] font-medium transition-colors",
                  paused
                    ? "border-(--warn)/40 bg-(--warn)/12 text-(--warn)"
                    : "border-border text-muted-foreground hover:border-(--warn)/40 hover:text-foreground",
                )}
              >
                <Pause className="size-3.5" />
                {paused ? "Starts paused" : "Start paused"}
              </button>
            </div>

            {/* footer — divider + primary action */}
            <div className="flex items-center justify-end gap-2 border-t border-border px-3.5 py-2.5">
              <button
                type="button"
                onClick={close}
                className="rounded-md px-3 py-1.5 text-[12.5px] font-medium text-muted-foreground transition-colors hover:bg-muted/60 hover:text-foreground"
              >
                Cancel
              </button>
              <button
                type="submit"
                disabled={!canCreate}
                className="flex items-center gap-1.5 rounded-md bg-(--signal) px-3 py-1.5 text-[12.5px] font-medium text-(--primary-foreground) transition-[filter] hover:brightness-105 disabled:cursor-not-allowed disabled:opacity-50"
              >
                Create thread
                <CornerDownLeft className="size-3.5 opacity-70" />
              </button>
            </div>
          </form>
        </DialogPrimitive.Popup>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  )
}

/** Staged-attachment chips (removable). Files upload only on submit. */
function FileChips({ files, onRemove }: { files: File[]; onRemove: (index: number) => void }) {
  if (files.length === 0) return null
  return (
    <div className="flex flex-wrap gap-1.5 px-4 pb-1">
      {files.map((f, i) => (
        <span
          key={`${f.name}-${i}`}
          className="flex items-center gap-1.5 rounded-md border border-border bg-muted/50 px-2 py-1 text-[11px] text-foreground/80"
        >
          <span className="max-w-[180px] truncate">{f.name}</span>
          <button
            type="button"
            onClick={() => onRemove(i)}
            aria-label={`Remove ${f.name}`}
            className="text-muted-foreground/60 transition-colors hover:text-(--danger)"
          >
            <X className="size-3" />
          </button>
        </span>
      ))}
    </div>
  )
}
