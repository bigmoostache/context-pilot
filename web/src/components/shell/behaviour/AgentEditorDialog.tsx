import { useEffect, useState } from "react"
import { Dialog as DialogPrimitive } from "@base-ui/react/dialog"
import { Bot, Loader2, X, CornerDownLeft, TerminalSquare } from "lucide-react"
import { fetchLibraryAgent } from "@/lib/api"
import { useUpsertLibraryAgent, useCreateCommand } from "@/lib/live"
import { cn } from "@/lib/utils"

/**
 * Which library item this dialog authors. `agent` (default) drives the
 * behaviour-agent upsert (`PUT …/library/agent/{id}`); `command` reuses the
 * exact same UI but wires to the command create (`POST …/library/command`) and
 * swaps the agent-specific wording (system prompt → prompt, file id → `/slug`).
 * The command flow is create-only (no edit/import), so callers pass
 * `mode={{ kind: "create" }}`.
 */
export type AgentEditorVariant = "agent" | "command"

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
  { kind: "create" } | { kind: "edit"; itemId: string; builtin: boolean }

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
function useAgentEditorFields(
  open: boolean,
  mode: AgentEditorMode,
  agentId: string,
  initial?: Prefill,
) {
  const [name, setName] = useState("")
  const [description, setDescription] = useState("")
  const [body, setBody] = useState("")
  const [loading, setLoading] = useState(false)
  const [seededKey, setSeededKey] = useState<string | null>(null)

  const openKey = open
    ? `${mode.kind === "edit" ? `edit:${mode.itemId}` : "create"}:${initial ? "seed" : "empty"}`
    : null
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

/** The variant-derived wording + icon for the editor chrome. Extracted as a
 *  pure helper so the component function stays under its line + complexity
 *  budgets (the title/submit/id-line branches live here, not in the render). */
interface EditorCopy {
  icon: React.ReactNode
  namePlaceholder: string
  idLine: React.ReactNode
  sectionLabel: string
  bodyPlaceholder: string
  title: string
  submitLabel: string
}

/** Resolve the {@link EditorCopy} for a variant + mode. `command` is always a
 *  create flow; `agent` distinguishes create / edit / built-in-override. */
function editorCopy(
  variant: AgentEditorVariant,
  mode: AgentEditorMode,
  slug: string,
  isBuiltin: boolean,
): EditorCopy {
  if (variant === "command") {
    return {
      icon: <TerminalSquare className="size-2.5" />,
      namePlaceholder: "Command name",
      idLine: (
        <>
          Invoked as <span className="font-mono text-(--interactive)">/{slug}</span>
        </>
      ),
      sectionLabel: "Prompt",
      bodyPlaceholder: "The prompt this command expands to when clicked…",
      title: "New command",
      submitLabel: "Create command",
    }
  }
  const isEdit = mode.kind === "edit"
  return {
    icon: <Bot className="size-2.5" />,
    namePlaceholder: "Agent name",
    idLine: (
      <>
        File id <span className="font-mono text-(--interactive)">{slug}</span>
        {isEdit && " · rename only changes the display name"}
      </>
    ),
    sectionLabel: "System prompt",
    bodyPlaceholder: "The system prompt this behaviour agent loads…",
    title: isEdit ? (isBuiltin ? "Override built-in" : "Edit agent") : "New agent",
    submitLabel: isEdit ? "Save agent" : "Create agent",
  }
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
  variant = "agent",
}: {
  open: boolean
  onClose: () => void
  agentId: string
  mode: AgentEditorMode
  /** Prefill (Import path passes the parsed `.md`; Edit fetches on open). */
  initial?: Prefill | undefined
  /** Which library item to author — `agent` (default) or `command`. */
  variant?: AgentEditorVariant
}) {
  const { name, setName, description, setDescription, body, setBody, loading } =
    useAgentEditorFields(open, mode, agentId, initial)
  const upsert = useUpsertLibraryAgent(agentId)
  const createCmd = useCreateCommand(agentId)
  const isCommand = variant === "command"
  // The active mutation for the current variant — both hooks are always called
  // (Rules of Hooks); only the relevant one is fired on submit.
  const mut = isCommand ? createCmd : upsert

  const slug = mode.kind === "edit" ? mode.itemId : slugify(name)
  const canSave = name.trim().length > 0 && body.trim().length > 0
  const error = mut.error instanceof Error ? mut.error.message : null
  const isBuiltin = !isCommand && mode.kind === "edit" && mode.builtin

  const close = () => {
    upsert.reset()
    createCmd.reset()
    onClose()
  }

  const submit = (e: React.SyntheticEvent) => {
    e.preventDefault()
    if (!canSave || mut.isPending) return
    const fields = { name: name.trim(), description: description.trim(), body: body.trim() }
    if (isCommand) {
      createCmd.mutate(fields, { onSuccess: () => close() })
    } else {
      upsert.mutate({ itemId: slug, ...fields }, { onSuccess: () => close() })
    }
  }

  // ⌘/Ctrl+Enter submits from anywhere in the form (Linear parity).
  const onKeyDown = (e: React.KeyboardEvent) => {
    if ((e.metaKey || e.ctrlKey) && e.key === "Enter") submit(e)
  }

  const copy = editorCopy(variant, mode, slug, isBuiltin)

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
            "fixed top-[5vh] left-1/2 z-50 flex max-h-[90vh] w-[920px] max-w-[94vw] -translate-x-1/2 flex-col",
            "overflow-hidden rounded-xl border border-border bg-popover text-popover-foreground",
            "shadow-(--shadow-pop) outline-none",
            "animate-[sheet-pop-in_.22s_cubic-bezier(.16,1,.3,1)]",
            "data-ending-style:animate-[sheet-pop-out_.15s_ease-in_forwards]",
          )}
        >
          <form onSubmit={submit} className="flex min-h-0 flex-1 flex-col">
            {/* breadcrumb + close — mirrors the New Thread sheet chrome */}
            <div className="flex items-center gap-2 px-4 pt-3 pb-1">
              <span className="flex items-center gap-1.5 text-[12px] text-muted-foreground">
                <span className="flex size-[15px] items-center justify-center rounded-sm bg-(--signal)/15 text-(--signal)">
                  {copy.icon}
                </span>
                <span className="text-muted-foreground/50">›</span>
                <span className="text-foreground/70">{copy.title}</span>
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

            {loading ? (
              <div className="flex flex-1 items-center justify-center gap-2 px-5 py-16 text-muted-foreground">
                <Loader2 className="size-4 animate-spin" /> Loading…
              </div>
            ) : (
              <div className="flex min-h-0 flex-1 flex-col">
                {/* name — big borderless title line */}
                <input
                  autoFocus
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  placeholder={copy.namePlaceholder}
                  className="w-full bg-transparent px-4 pt-1 text-[19px] font-semibold tracking-tight text-foreground outline-none placeholder:text-muted-foreground/40"
                />
                <span className="px-4 pt-0.5 text-[11px] text-muted-foreground/70">
                  {copy.idLine}
                </span>

                {/* description — seamless secondary line */}
                <input
                  value={description}
                  onChange={(e) => setDescription(e.target.value)}
                  placeholder="One-line description (optional)"
                  className="w-full bg-transparent px-4 pt-2.5 text-[13.5px] text-foreground/90 outline-none placeholder:text-muted-foreground/40"
                />

                {/* system prompt — the primary field, its own scroll region */}
                <div className="mt-2 flex min-h-0 flex-1 flex-col border-t border-border/60">
                  <span className="px-4 pt-2.5 text-[11px] font-medium tracking-wide text-muted-foreground/70 uppercase">
                    {copy.sectionLabel}
                  </span>
                  <textarea
                    value={body}
                    onChange={(e) => setBody(e.target.value)}
                    placeholder={copy.bodyPlaceholder}
                    className="min-h-[340px] w-full flex-1 resize-none overflow-y-auto bg-transparent px-4 pt-1.5 pb-3 font-mono text-[12.5px] leading-relaxed text-foreground/90 outline-none placeholder:text-muted-foreground/40"
                  />
                </div>

                {error && <span className="px-4 pb-1 text-[11px] text-(--danger)">{error}</span>}
              </div>
            )}

            {/* footer — override note (left), submit (right); Esc/backdrop close */}
            <div className="flex items-center gap-3 border-t border-border px-3.5 py-2.5">
              {isBuiltin && (
                <span className="text-[11px] text-muted-foreground/70">
                  Saves a local copy — the built-in is never touched.
                </span>
              )}
              <button
                type="submit"
                disabled={!canSave || mut.isPending || loading}
                className="ml-auto flex items-center gap-1.5 rounded-md bg-(--signal) px-3 py-1.5 text-[12.5px] font-medium text-(--primary-foreground) transition-[filter] hover:brightness-105 disabled:cursor-not-allowed disabled:opacity-50"
              >
                {mut.isPending && <Loader2 className="size-3.5 animate-spin" />}
                {copy.submitLabel}
                <CornerDownLeft className="size-3.5 opacity-70" />
              </button>
            </div>
          </form>
        </DialogPrimitive.Popup>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  )
}
