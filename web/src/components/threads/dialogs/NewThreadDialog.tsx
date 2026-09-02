import { useState, useRef, useEffect, useCallback } from "react"
import { Dialog as DialogPrimitive } from "@base-ui/react/dialog"
import { Paperclip, X, CornerDownLeft, Loader2 } from "lucide-react"
import { uploadUnique } from "@/lib/api"
import type { CreateThreadOpts } from "@/lib/live/threadView"
import type { UploadedFile } from "@/lib/live/threadUpload"
import { cn } from "@/lib/utils"

/** Persisted draft shape — everything the composer holds, so a close/reopen (or
 *  a full reload) restores the exact in-progress thread. Files are stored as
 *  ALREADY-uploaded {@link UploadedFile} records (the dialog uploads on attach),
 *  so the draft is JSON-serialisable and the attachments survive too (T679). */
interface Draft {
  title: string
  firstMessage: string
  files: UploadedFile[]
  paused: boolean
}

const EMPTY_DRAFT: Draft = { title: "", firstMessage: "", files: [], paused: false }

/** Per-agent localStorage key for the in-progress new-thread draft. */
function draftKey(agentId: string): string {
  return `cp-newthread-${agentId}`
}

/** Hydrate the persisted draft for an agent, falling back to an empty draft on a
 *  missing / malformed entry (never throws into render). */
function loadDraft(agentId: string): Draft {
  try {
    const raw = localStorage.getItem(draftKey(agentId))
    if (!raw) return EMPTY_DRAFT
    const d = JSON.parse(raw) as Partial<Draft>
    return {
      title: typeof d.title === "string" ? d.title : "",
      firstMessage: typeof d.firstMessage === "string" ? d.firstMessage : "",
      files: Array.isArray(d.files) ? d.files : [],
      paused: d.paused === true,
    }
  } catch {
    return EMPTY_DRAFT
  }
}

/** All composer state + handlers for the new-thread dialog, extracted from the
 *  component so its render fn stays within the max-lines budget. Owns the draft
 *  hydrate/persist, the auto-grow ref, on-attach uploads, and submit/clear. */
function useNewThreadDraft(
  agentId: string,
  open: boolean,
  onCreate: (o: CreateThreadOpts) => void,
) {
  // Seed all composer state from the persisted per-agent draft (render-phase
  // initializer, runs once per mount). The dialog body unmounts on close (Base
  // UI only renders the Popup while open), so a reopen re-hydrates from here.
  const initial = () => loadDraft(agentId)
  const [title, setTitle] = useState(() => initial().title)
  const [firstMessage, setFirstMessage] = useState(() => initial().firstMessage)
  const [files, setFiles] = useState<UploadedFile[]>(() => initial().files)
  const [paused, setPaused] = useState(() => initial().paused)
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState<string | null>(null)
  const canCreate = title.trim().length > 0

  // Mirror the composer to localStorage on every change so the draft survives a
  // close/reopen or a reload. Persist regardless of `open` — the whole point is
  // that a dismissed-but-not-submitted draft comes back intact.
  useEffect(() => {
    const draft: Draft = { title, firstMessage, files, paused }
    localStorage.setItem(draftKey(agentId), JSON.stringify(draft))
  }, [agentId, title, firstMessage, files, paused])

  // Auto-grow the message editor to fit its content (like the thread composer):
  // reset to `auto`, then set to `scrollHeight`. The CSS `max-h-[44vh]` caps the
  // element and `overflow-y-auto` scrolls past that cap, so a very long draft
  // never pushes the dialog off screen.
  const msgRef = useRef<HTMLTextAreaElement>(null)
  const resizeMsg = useCallback(() => {
    const el = msgRef.current
    if (!el) return
    el.style.height = "auto"
    el.style.height = `${el.scrollHeight}px`
  }, [])

  // Re-grow synchronously on every keystroke (the responsive typing path).
  useEffect(() => {
    resizeMsg()
  }, [firstMessage, resizeMsg])

  // Grow on OPEN too. The textarea (re)mounts inside Base UI's portaled,
  // animated Popup, so a measurement taken during that commit reads the
  // collapsed height and a PRE-FILLED draft would stay one row tall until the
  // first keystroke re-ran the effect. Measuring a frame later — once layout
  // is settled and the popup visible — grows it immediately on open.
  useEffect(() => {
    if (!open) return
    const id = requestAnimationFrame(resizeMsg)
    return () => cancelAnimationFrame(id)
  }, [open, resizeMsg])

  // Wipe the draft (state + persisted key) — ONLY on a successful create.
  const clearDraft = () => {
    setTitle("")
    setFirstMessage("")
    setFiles([])
    setPaused(false)
    setErr(null)
    localStorage.removeItem(draftKey(agentId))
  }

  const submit = (e: React.SyntheticEvent) => {
    e.preventDefault()
    if (!canCreate || busy) return
    onCreate({ title, firstMessage, files, paused })
    clearDraft()
  }

  // ⌘/Ctrl+Enter submits from anywhere in the form (Linear parity).
  const onKeyDown = (e: React.KeyboardEvent) => {
    if ((e.metaKey || e.ctrlKey) && e.key === "Enter") submit(e)
  }

  // Upload attachments to `.uploads/` immediately so the draft holds serialisable
  // paths (survives close/reopen), not raw File objects (T679).
  const addFiles = async (list: FileList | null) => {
    const picked = [...(list ?? [])]
    if (picked.length === 0) return
    setBusy(true)
    setErr(null)
    try {
      const added: UploadedFile[] = []
      for (const f of picked) {
        const r = await uploadUnique(agentId, ".uploads", f)
        added.push({
          path: r.path,
          name: r.name,
          size: r.size,
          note: `uploaded by user at ${new Date().toISOString()}`,
        })
      }
      setFiles((prev) => [...prev, ...added])
    } catch (e) {
      setErr(e instanceof Error ? e.message : "Upload failed")
    } finally {
      setBusy(false)
    }
  }

  return {
    title,
    setTitle,
    firstMessage,
    setFirstMessage,
    files,
    setFiles,
    paused,
    setPaused,
    busy,
    err,
    canCreate,
    msgRef,
    submit,
    onKeyDown,
    addFiles,
  }
}

/**
 * New Thread dialog (T674, reworked T678/T679) — a Linear-"new issue"-style
 * composer.
 *
 * Layout mirrors Linear's fast-create popup: a large borderless **title** line,
 * a seamless **auto-growing** first-message editor beneath it (grows with the
 * text like the thread composer, capped at 44vh so the surface always stays on
 * screen), then a single footer action row — attach (icon button, left) + a
 * "start paused" toggle switch, with the primary Create action on the right.
 * There is no Cancel button: Esc and clicking the backdrop already dismiss.
 *
 * **Draft persistence (T679):** the whole composer state (title, first message,
 * attachments, paused) is mirrored to a per-agent localStorage key on every
 * change, so leaving the dialog and coming back — or a full reload — restores
 * the in-progress thread. Attachments upload to `.uploads/` *on attach* (not on
 * submit), so what is persisted is a list of realm-relative paths, not
 * un-serialisable `File` objects. The draft is cleared only on a successful
 * create, never on close.
 *
 * **Submit (T679):** the first message is NOT handed to the backend as an
 * atomic `initial_message` (that silently dropped against an N-1 agent).
 * `onCreate` creates the thread name-only and the parent sends the composed
 * first message once the new thread's server-assigned id is observed — so the
 * message always lands on the right thread and needs no orchestrator change.
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
  agentId,
}: {
  open: boolean
  onClose: () => void
  onCreate: (opts: CreateThreadOpts) => void
  /** owning agent — scopes the draft key and receives on-attach uploads */
  agentId: string
}) {
  const {
    title,
    setTitle,
    firstMessage,
    setFirstMessage,
    files,
    setFiles,
    paused,
    setPaused,
    busy,
    err,
    canCreate,
    msgRef,
    submit,
    onKeyDown,
    addFiles,
  } = useNewThreadDraft(agentId, open, onCreate)

  return (
    <DialogPrimitive.Root open={open} onOpenChange={(o) => !o && onClose()}>
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
                onClick={onClose}
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
            {err && <span className="px-4 pb-1 text-[11px] text-(--danger)">{err}</span>}

            {/* footer — attach + start-paused toggle (left), Create (right) */}
            <div className="flex items-center gap-3 border-t border-border px-3.5 py-2.5">
              {/* Colour-only hover, matching the composer's attach button —
                  the same control must not behave two ways. */}
              <label
                title="Attach files"
                className="flex size-8 cursor-pointer items-center justify-center rounded-md text-muted-foreground/70 transition-colors hover:text-foreground"
              >
                {busy ? (
                  <Loader2 className="size-4 animate-spin" />
                ) : (
                  <Paperclip className="size-4" />
                )}
                <input
                  type="file"
                  multiple
                  className="hidden"
                  disabled={busy}
                  onChange={(e) => {
                    void addFiles(e.target.files)
                    e.target.value = ""
                  }}
                />
              </label>

              <span className="flex items-center gap-2 text-[12px] font-medium text-muted-foreground select-none">
                <PausedToggle on={paused} onToggle={() => setPaused((p) => !p)} />
                Start paused
              </span>

              <button
                type="submit"
                disabled={!canCreate || busy}
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

/** Staged-attachment chips (removable). Files are already uploaded (T679). */
function FileChips({
  files,
  onRemove,
}: {
  files: UploadedFile[]
  onRemove: (index: number) => void
}) {
  if (files.length === 0) return null
  return (
    <div className="flex flex-wrap gap-1.5 px-4 pb-1">
      {files.map((f, i) => (
        <span
          key={`${f.path}-${i}`}
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
