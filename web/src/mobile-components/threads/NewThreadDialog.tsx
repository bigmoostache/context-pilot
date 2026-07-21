import { useState } from "react"
import { MessagesSquare } from "lucide-react"
import { Dialog, DialogContent, DialogTitle, DialogDescription } from "@/mobile-components/ui/dialog"

/**
 * Mobile New Thread dialog — the divergent twin of `components/threads/
 * NewThreadDialog`. Same single-field "give it a title" flow and shadcn Dialog
 * primitive (focus-trap, scroll-lock, Esc), forked only for touch:
 *
 *   • **16px title input** — iOS Safari auto-zooms the viewport when a focused
 *     input's font is under 16px; the desktop 13px would jank the layout on tap.
 *   • **Taller controls** — the input and both buttons grow for comfortable
 *     thumb use.
 *
 * The real bottom-sheet presentation lands when `ui/dialog` itself is recoded
 * for mobile (mirror token already resolves to it here).
 */
export function NewThreadDialog({
  open,
  onClose,
  onCreate,
}: {
  open: boolean
  onClose: () => void
  onCreate: (title: string) => void
}) {
  const [title, setTitle] = useState("")
  const canCreate = title.trim().length > 0

  const close = () => {
    setTitle("")
    onClose()
  }

  const submit = (e: React.SyntheticEvent) => {
    e.preventDefault()
    if (!canCreate) return
    onCreate(title)
    setTitle("")
  }

  return (
    <Dialog open={open} onOpenChange={(o) => !o && close()}>
      <DialogContent className="w-[440px] max-w-[94vw] p-5">
        <form onSubmit={submit} className="flex flex-col gap-4">
          <div className="flex items-center gap-3">
            <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-(--signal)/15 text-(--signal)">
              <MessagesSquare className="size-[18px]" />
            </span>
            <div className="flex flex-col">
              <DialogTitle>New Thread</DialogTitle>
              <DialogDescription>
                Give it a title — you can put the agent to work once it's open.
              </DialogDescription>
            </div>
          </div>

          <input
            autoFocus
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            placeholder="e.g. Refactor the cache engine"
            className="w-full rounded-lg border border-border bg-card px-3 py-2.5 text-[16px] text-foreground/90 outline-none placeholder:text-muted-foreground/50 focus:border-(--signal)/60"
          />

          <div className="flex items-center justify-end gap-2">
            <button
              type="button"
              onClick={close}
              className="rounded-lg px-4 py-2.5 text-[13px] font-medium text-muted-foreground transition-colors active:bg-muted/60"
            >
              Cancel
            </button>
            <button
              type="submit"
              disabled={!canCreate}
              className="rounded-lg bg-(--signal) px-4 py-2.5 text-[13px] font-medium text-(--primary-foreground) transition-[filter] active:brightness-105 disabled:cursor-not-allowed disabled:opacity-50"
            >
              Create thread
            </button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  )
}
