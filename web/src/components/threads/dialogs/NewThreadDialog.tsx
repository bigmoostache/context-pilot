import { useState, useRef, useEffect } from "react"
import { Dialog as DialogPrimitive } from "@base-ui/react/dialog"
import { Paperclip, X, CornerDownLeft } from "lucide-react"
import type { CreateThreadOpts } from "@/lib/live/threadView"
import { cn } from "@/lib/utils"

/**
 * New Thread dialog (T674, reworked T678) — a Linear-"new issue"-style composer.
 *
 * Layout mirrors Linear's fast-create popup: a large borderless **title** line,
 * a seamless **auto-growing** first-message editor beneath it (grows with the
 * text like the thread composer, capped at 44vh so the surface always stays on
 * screen), then a single footer action row — attach (icon button, left) + a
 * "start paused" toggle switch, with the primary Create action on the right.
 * There is no Cancel button: Esc and clicking the backdrop already dismiss.
 *
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

  // Auto-grow the message editor to fit its content (like the thread composer):
  // reset to `auto`, then set to `scrollHeight`. The CSS `max-h-[44vh]` caps the
  // element and `overflow-y-auto` scrolls past that cap, so a very long draft
  // never pushes the dialog off screen.
  const msgRef = useRef<HTMLTextAreaElement>(null)
  useEffect(() => {
    const el = msgRef.current
    if (!el) return
    el.style.height = "auto"
    el.style.height = `${el.scrollHeight}px`
  }, [firstMessage, open])

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
            "fixed top-[11vh] left-1/2 z-50 w-[740px] max-w-[94vw] -translate-x-1/2",
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

            {/* first message — seamless auto-growing editor under the title */}
            <textarea
              ref={msgRef}
              value={firstMessage}
              onChange={(e) => setFirstMessage(e.target.value)}
              placeholder="Add a first message… (sent automatically)"
              rows={2}
              className="max-h-[44vh] w-full resize-none overflow-y-auto bg-transparent px-4 pt-2 pb-3 text-[13.5px] leading-relaxed text-foreground/90 outline-none placeholder:text-muted-foreground/40"
            />

            <FileChips
              files={files}
              onRemove={(i) => setFiles((p) => p.filter((_, idx) => idx !== i))}
            />

            {/* footer — attach + start-paused toggle (left), Create (right) */}
            <div className="flex items-center gap-3 border-t border-border px-3.5 py-2.5">
              <label
                title="Attach files"
                className="flex size-8 cursor-pointer items-center justify-center rounded-md text-muted-foreground/70 transition-colors hover:bg-muted hover:text-foreground"
              >
                <Paperclip className="size-4" />
                <input
                  type="file"
                  multiple
                  className="hidden"
                  onChange={(e) => addFiles(e.target.files)}
                />
              </label>

              <span className="flex items-center gap-2 text-[12px] font-medium text-muted-foreground select-none">
                <PausedToggle on={paused} onToggle={() => setPaused((p) => !p)} />
                Start paused
              </span>

              <button
                type="submit"
                disabled={!canCreate}
                className="ml-auto flex items-center gap-1.5 rounded-md bg-(--signal) px-3 py-1.5 text-[12.5px] font-medium text-(--primary-foreground) transition-[filter] hover:brightness-105 disabled:cursor-not-allowed disabled:opacity-50"
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

/** Neutral on/off switch for the "start paused" option — same switch markup as
 *  the form `toggle` field, in the paused-amber accent. */
function PausedToggle({ on, onToggle }: { on: boolean; onToggle: () => void }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      onClick={onToggle}
      className={cn(
        "relative inline-flex h-5 w-9 shrink-0 items-center rounded-full transition-colors",
        on ? "bg-(--warn)" : "bg-muted",
      )}
    >
      <span
        className={cn(
          "inline-block size-4 transform rounded-full bg-white shadow-sm transition-transform",
          on ? "translate-x-4" : "translate-x-0.5",
        )}
      />
    </button>
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
