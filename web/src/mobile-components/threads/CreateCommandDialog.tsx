import { useState } from "react"
import { TerminalSquare, Loader2 } from "lucide-react"
import {
  Dialog,
  DialogContent,
  DialogTitle,
  DialogDescription,
} from "@/mobile-components/ui/dialog"
import { Input } from "@/mobile-components/ui/input"
import { Textarea } from "@/mobile-components/ui/textarea"
import { Button } from "@/mobile-components/ui/button"
import { useCreateCommand } from "@/lib/live"

/**
 * Derive a command slug from its name — mirrors the orchestrator's `slugify`
 * (lowercase, non-alphanumerics → `-`, collapsed + trimmed, never empty) so the
 * `/invocation` preview shown here matches the file the backend will write.
 */
function slugify(name: string): string {
  const slug = name
    .trim()
    .toLowerCase()
    .replaceAll(/[^a-z0-9]+/g, "-")
    .replaceAll(/^-+|-+$/g, "")
  return slug || "untitled"
}

/**
 * Mobile Create Command dialog — the divergent twin of `components/threads/
 * CreateCommandDialog`. Same three-field authoring flow (name → live `/slug`
 * preview, optional description, prompt body) and {@link useCreateCommand}
 * mutation, forked only for touch: every text control carries a **16px font**
 * so iOS Safari doesn't auto-zoom on focus, and the field font-size is set via
 * `className` since the shared shadcn primitives default smaller.
 *
 * The real bottom-sheet presentation lands when `ui/dialog` itself is recoded
 * for mobile (the mirror token already resolves to it here).
 */
export function CreateCommandDialog({
  open,
  onClose,
  agentId,
}: {
  open: boolean
  onClose: () => void
  agentId: string
}) {
  const [name, setName] = useState("")
  const [description, setDescription] = useState("")
  const [body, setBody] = useState("")
  const create = useCreateCommand(agentId)

  const slug = slugify(name)
  const canCreate = name.trim().length > 0 && body.trim().length > 0
  const error = create.error instanceof Error ? create.error.message : null

  const close = () => {
    setName("")
    setDescription("")
    setBody("")
    create.reset()
    onClose()
  }

  const submit = (e: React.SyntheticEvent) => {
    e.preventDefault()
    if (!canCreate || create.isPending) return
    create.mutate(
      { name: name.trim(), description: description.trim(), body: body.trim() },
      { onSuccess: () => close() },
    )
  }

  return (
    <Dialog open={open} onOpenChange={(o) => !o && close()}>
      <DialogContent className="w-[540px] max-w-[94vw] p-0">
        <form onSubmit={submit} className="flex flex-col">
          {/* Header */}
          <div className="flex items-start gap-3 border-b border-border/70 px-5 py-4">
            <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-(--signal)/15 text-(--signal)">
              <TerminalSquare className="size-[18px]" />
            </span>
            <div className="flex flex-col gap-0.5">
              <DialogTitle>New command</DialogTitle>
              <DialogDescription>
                Author a <span className="font-mono text-foreground/70">/command</span> for this
                agent's library — it appears as a suggestion the moment you save it.
              </DialogDescription>
            </div>
          </div>

          {/* Body */}
          <div className="flex flex-col gap-4 px-5 py-4">
            <label htmlFor="cmd-name" className="flex flex-col gap-1.5">
              <span className="text-[12px] font-medium text-foreground/80">Name</span>
              <Input
                id="cmd-name"
                autoFocus
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="e.g. Boss Hunt"
                className="text-[16px]"
              />
              <span className="text-[11px] text-muted-foreground/70">
                Invoked as <span className="font-mono text-(--interactive)">/{slug}</span>
              </span>
            </label>

            <label htmlFor="cmd-desc" className="flex flex-col gap-1.5">
              <span className="text-[12px] font-medium text-foreground/80">
                Description <span className="text-muted-foreground/60">(optional)</span>
              </span>
              <Input
                id="cmd-desc"
                value={description}
                onChange={(e) => setDescription(e.target.value)}
                placeholder="Shown on the suggestion bubble"
                className="text-[16px]"
              />
            </label>

            <label htmlFor="cmd-body" className="flex flex-col gap-1.5">
              <span className="text-[12px] font-medium text-foreground/80">Prompt</span>
              <Textarea
                id="cmd-body"
                value={body}
                onChange={(e) => setBody(e.target.value)}
                placeholder="The prompt this command expands to when clicked…"
                className="min-h-[140px] font-mono text-[16px] leading-relaxed"
              />
            </label>

            {/* Live preview of the bubble this command will become */}
            <div className="flex flex-col gap-1.5">
              <span className="text-[11px] text-muted-foreground/70">Preview</span>
              <span className="inline-flex w-fit items-center gap-1.5 rounded-full border border-border bg-card px-2.5 py-1 text-[11.5px] text-foreground/75">
                <span className="font-mono font-medium text-(--interactive)">/{slug}</span>
                {description.trim() && (
                  <span className="max-w-[260px] truncate text-muted-foreground/70">
                    {description.trim()}
                  </span>
                )}
              </span>
            </div>

            {error && <p className="text-[12px] text-destructive">{error}</p>}
          </div>

          {/* Footer */}
          <div className="flex items-center justify-end gap-2 border-t border-border/70 px-5 py-3">
            <Button type="button" variant="ghost" size="sm" onClick={close}>
              Cancel
            </Button>
            <Button type="submit" size="sm" disabled={!canCreate || create.isPending}>
              {create.isPending && <Loader2 className="size-3.5 animate-spin" />}
              Create command
            </Button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  )
}
