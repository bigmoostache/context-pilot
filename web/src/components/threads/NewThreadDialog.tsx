import { useState } from "react"
import { MessagesSquare } from "lucide-react"
import { Dialog, DialogContent, DialogTitle, DialogDescription } from "@/components/ui/dialog"

/**
 * New Thread dialog — the maquette for starting a thread. A focused little
 * sheet that asks only for a **title**; on submit the parent prepends a fresh
 * MY_TURN thread and selects it. Built on the portaled Base UI Dialog primitive
 * (focus-trap, scroll-lock, Esc-to-close, backdrop fade + spring-in).
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

  const submit = (e: React.FormEvent) => {
    e.preventDefault()
    if (!canCreate) return
    onCreate(title)
    setTitle("")
  }

  return (
    <Dialog open={open} onOpenChange={(o) => !o && close()}>
      <DialogContent className="w-[440px] max-w-[92vw] p-5">
        <form onSubmit={submit} className="flex flex-col gap-4">
          <div className="flex items-center gap-3">
            <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-[var(--signal)]/15 text-[var(--signal)]">
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
            className="w-full rounded-lg border border-border bg-card px-3 py-2 text-[13px] text-foreground/90 placeholder:text-muted-foreground/50 outline-none focus:border-[var(--signal)]/60"
          />

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
              className="rounded-lg bg-[var(--signal)] px-3.5 py-1.5 text-[12.5px] font-medium text-[var(--primary-foreground)] transition-[filter] hover:brightness-105 disabled:cursor-not-allowed disabled:opacity-50"
            >
              Create thread
            </button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  )
}
