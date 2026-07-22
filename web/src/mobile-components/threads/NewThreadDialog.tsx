import { useState } from "react"
import { Dialog, DialogContent, DialogTitle } from "@/mobile-components/ui/dialog"

/**
 * Mobile New Thread sheet — the divergent twin of `components/threads/
 * NewThreadDialog`, rebuilt to feel like an iOS "new message" flow (T624)
 * rather than a transcribed desktop modal card.
 *
 * The presentation is an iOS **bottom sheet** (the mobile `ui/dialog` primitive
 * already anchors bottom, rounds its top, and pads the home-indicator safe
 * area — this consumer just embraces it instead of overriding a fixed centred
 * width). Inside:
 *
 *   • a **grabber** pill at the top edge — the universal iOS "drag me down /
 *     dismissable sheet" affordance;
 *   • an iOS **nav-bar header**: a `Cancel` text link (left), the `New Thread`
 *     title (centre), and a bold `Create` action (right) that stays disabled
 *     until a title is typed — the Cancel|Title|Done convention every iOS modal
 *     uses, replacing the desktop two-button footer row;
 *   • a single large **16px rounded input** (16px defeats iOS focus-zoom),
 *     autofocused so the keyboard is ready the instant the sheet opens — here
 *     autofocus is wanted: the user tapped compose deliberately, exactly like
 *     iMessage focusing the To: field on compose.
 *
 * Submitting (the Create action or the keyboard return) hands the title up; the
 * parent prepends a fresh MY_TURN thread and closes the sheet. Dismiss is the
 * native sheet gesture (tap the dimmed backdrop / swipe down) or the Cancel
 * link.
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
      {/* No width override — let the primitive's full-width bottom-sheet base
          stand. px-0 so the header hairline can span edge-to-edge; children
          re-inset themselves. */}
      <DialogContent className="px-0 pt-2 pb-[max(1.25rem,env(safe-area-inset-bottom))]">
        {/* grabber — the iOS "this sheet is dismissable" pill */}
        <div className="mx-auto mb-1 h-1 w-9 rounded-full bg-muted-foreground/25" />

        {/* iOS nav-bar header: Cancel · title · Create */}
        <div className="grid grid-cols-[1fr_auto_1fr] items-center border-b border-border/70 px-4 py-2">
          <button
            type="button"
            onClick={close}
            className="justify-self-start text-[16px] text-(--signal) active:opacity-60"
          >
            Cancel
          </button>
          <DialogTitle className="justify-self-center text-[16px]">New Thread</DialogTitle>
          <button
            type="submit"
            form="new-thread-form"
            disabled={!canCreate}
            className="justify-self-end text-[16px] font-semibold text-(--signal) active:opacity-60 disabled:text-muted-foreground/40"
          >
            Create
          </button>
        </div>

        {/* the single input — big, rounded, autofocused */}
        <form id="new-thread-form" onSubmit={submit} className="px-4 pt-4">
          <input
            autoFocus
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            placeholder="What should this thread be about?"
            className="w-full rounded-xl bg-muted/60 px-4 py-3 text-[16px] text-foreground/90 outline-none placeholder:text-muted-foreground/50"
          />
          <p className="mt-2 px-1 text-[12.5px] text-muted-foreground/70">
            Give it a title — you can put the agent to work once it's open.
          </p>
        </form>
      </DialogContent>
    </Dialog>
  )
}
