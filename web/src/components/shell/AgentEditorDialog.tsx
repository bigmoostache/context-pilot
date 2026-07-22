import { useEffect, useState } from "react"
import { Bot, Loader2 } from "lucide-react"
import { Dialog, DialogContent, DialogTitle, DialogDescription } from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Textarea } from "@/components/ui/textarea"
import { Button } from "@/components/ui/button"
import { fetchLibraryAgent } from "@/lib/api"
import { useUpsertLibraryAgent } from "@/lib/live"

/**
 * Derive a behaviour-agent file id (slug) from its name — mirrors the
 * orchestrator's `slugify` (lowercase, non-alphanumerics → `-`, collapsed +
 * trimmed, never empty). Used only when CREATING (the file id is the slug of
 * the initial name); an EDIT keeps its original `itemId` so a display-name
 * change never orphans the `.md`.
 */
function slugify(name: string): string {
  const slug = name
    .trim()
    .toLowerCase()
    .replaceAll(/[^a-z0-9]+/g, "-")
    .replaceAll(/^-+|-+$/g, "")
  return slug || "untitled"
}

/** The editor's working mode — a fresh create, or editing an existing item. */
export type AgentEditorMode =
  | { kind: "create" }
  | { kind: "edit"; itemId: string; builtin: boolean }

/** The prefill an Import/Edit flow seeds the editor with. */
interface Prefill {
  name: string
  description: string
  body: string
}

/** The editor's field state + its open-driven seeding, extracted from the
 *  component so its several branches don't inflate the component's cyclomatic
 *  complexity budget. Seeds synchronously on open (render-phase adjust-state,
 *  NOT an effect) for Create/Import; the Edit path flags `loading` here and
 *  fetches the raw `.md` (or built-in seed) in the effect below, whose setState
 *  runs in async callbacks (after paint) so it never trips set-state-in-effect. */
function useAgentEditorFields(open: boolean, mode: AgentEditorMode, agentId: string, initial?: Prefill) {
  const [name, setName] = useState("")
  const [description, setDescription] = useState("")
  const [body, setBody] = useState("")
  const [loading, setLoading] = useState(false)
  const [seededKey, setSeededKey] = useState<string | null>(null)

  const openKey = open ? `${mode.kind === "edit" ? `edit:${mode.itemId}` : "create"}:${initial ? "seed" : "empty"}` : null
  if (openKey !== seededKey) {
    setSeededKey(openKey)
    if (initial) {
      setName(initial.name)
      setDescription(initial.description)
      setBody(initial.body)
    } else if (open && mode.kind === "edit") {
      setLoading(true)
    } else {
      setName("")
      setDescription("")
      setBody("")
    }
  }

  useEffect(() => {
    if (!open || mode.kind !== "edit" || initial) return
    let cancelled = false
    fetchLibraryAgent(agentId, mode.itemId)
      .then((raw) => {
        if (cancelled) return
        setName(raw.name)
        setDescription(raw.description)
        setBody(raw.body)
      })
      .catch(() => {
        /* leave fields blank; backend re-validates on save */
      })
      .finally(() => !cancelled && setLoading(false))
    return () => {
      cancelled = true
    }
  }, [open, mode, agentId, initial])

  return { name, setName, description, setDescription, body, setBody, loading }
}

/**
 * Behaviour-agent editor dialog (T581 footer selector). One dialog serves three
 * flows — **Create** (empty), **Edit** (prefilled from the on-disk `.md`, or the
 * compiled-in seed for a pure built-in), and **Import** (the parent parses a
 * dropped `.md` and opens this dialog prefilled). All three converge on the SAME
 * `PUT …/library/agent/{itemId}` upsert (via {@link useUpsertLibraryAgent}) so
 * editing a built-in writes a local override, exactly the tui loader's merge
 * rule. The backend re-validates authoritatively (M141) — this component only
 * renders + calls.
 *
 * Fields: **name** (its slug becomes the file id on create, previewed live),
 * optional one-line **description**, and the **system-prompt body**.
 */
export function AgentEditorDialog({
  open,
  onClose,
  agentId,
  mode,
  initial,
}: {
  open: boolean
  onClose: () => void
  agentId: string
  mode: AgentEditorMode
  /** Prefill (Import path passes the parsed `.md`; Edit fetches on open). */
  initial?: Prefill | undefined
}) {
  const { name, setName, description, setDescription, body, setBody, loading } = useAgentEditorFields(
    open,
    mode,
    agentId,
    initial,
  )
  const upsert = useUpsertLibraryAgent(agentId)

  const slug = mode.kind === "edit" ? mode.itemId : slugify(name)
  const canSave = name.trim().length > 0 && body.trim().length > 0
  const error = upsert.error instanceof Error ? upsert.error.message : null
  const isBuiltin = mode.kind === "edit" && mode.builtin

  const close = () => {
    upsert.reset()
    onClose()
  }

  const submit = (e: React.SyntheticEvent) => {
    e.preventDefault()
    if (!canSave || upsert.isPending) return
    upsert.mutate(
      { itemId: slug, name: name.trim(), description: description.trim(), body: body.trim() },
      { onSuccess: () => close() },
    )
  }

  const title = mode.kind === "create" ? "New behaviour agent" : isBuiltin ? "Override built-in agent" : "Edit agent"

  return (
    <Dialog open={open} onOpenChange={(o) => !o && close()}>
      <DialogContent className="flex h-[88vh] max-h-[900px] w-[92vw] max-w-[1080px] flex-col overflow-hidden p-0">
        <form onSubmit={submit} className="flex min-h-0 flex-1 flex-col">
          {/* Header */}
          <div className="flex items-start gap-3 border-b border-border/70 px-5 py-4">
            <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-(--signal)/15 text-(--signal)">
              <Bot className="size-[18px]" />
            </span>
            <div className="flex flex-col gap-0.5">
              <DialogTitle>{title}</DialogTitle>
              <DialogDescription>
                {isBuiltin
                  ? "Saving writes a local copy that overrides the built-in — the original is never touched."
                  : "A system-prompt agent for this realm's behaviour selector."}
              </DialogDescription>
            </div>
          </div>

          {/* Body */}
          {loading ? (
            <div className="flex flex-1 items-center justify-center gap-2 px-5 py-10 text-muted-foreground">
              <Loader2 className="size-4 animate-spin" /> Loading…
            </div>
          ) : (
            <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-5 py-4">
              <label htmlFor="agent-name" className="flex flex-col gap-1.5">
                <span className="text-[12px] font-medium text-foreground/80">Name</span>
                <Input
                  id="agent-name"
                  autoFocus
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  placeholder="e.g. Worker"
                />
                <span className="text-[11px] text-muted-foreground/70">
                  File id <span className="font-mono text-(--interactive)">{slug}</span>
                  {mode.kind === "edit" && " (fixed — rename only changes the display name)"}
                </span>
              </label>

              <label htmlFor="agent-desc" className="flex flex-col gap-1.5">
                <span className="text-[12px] font-medium text-foreground/80">
                  Description <span className="text-muted-foreground/60">(optional)</span>
                </span>
                <Input
                  id="agent-desc"
                  value={description}
                  onChange={(e) => setDescription(e.target.value)}
                  placeholder="One-line summary"
                />
              </label>

              <label htmlFor="agent-body" className="flex min-h-0 flex-1 flex-col gap-1.5">
                <span className="text-[12px] font-medium text-foreground/80">System prompt</span>
                <Textarea
                  id="agent-body"
                  value={body}
                  onChange={(e) => setBody(e.target.value)}
                  placeholder="The system prompt this behaviour agent loads…"
                  className="h-full min-h-[220px] resize-none font-mono text-[12.5px] leading-relaxed"
                />
              </label>

              {error && <p className="text-[12px] text-destructive">{error}</p>}
            </div>
          )}

          {/* Footer */}
          <div className="flex items-center justify-end gap-2 border-t border-border/70 px-5 py-3">
            <Button type="button" variant="ghost" size="sm" onClick={close}>
              Cancel
            </Button>
            <Button type="submit" size="sm" disabled={!canSave || upsert.isPending || loading}>
              {upsert.isPending && <Loader2 className="size-3.5 animate-spin" />}
              {mode.kind === "create" ? "Create" : "Save"}
            </Button>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  )
}

