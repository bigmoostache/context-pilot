import { useEffect, useState } from "react"
import {
  Bot,
  Boxes,
  Check,
  Code2,
  Download,
  FileCode2,
  Sparkles,
  Terminal,
  TerminalSquare,
  Wind,
  X,
  Zap,
} from "lucide-react"
import type { LibraryItem, LibraryKind } from "@/lib/types"
import { cn } from "@/lib/utils"
import { MarkdownEditor } from "./MarkdownEditor"

// ── kind identity (mirrors PromptsPage) ───────────────────────────
const KIND: Record<
  LibraryKind,
  { label: string; icon: typeof Bot; accent: string; blurb: string }
> = {
  agent: {
    label: "System prompt",
    icon: Bot,
    accent: "var(--signal)",
    blurb: "A personality & operating contract.",
  },
  skill: {
    label: "Skill",
    icon: Zap,
    accent: "var(--interactive)",
    blurb: "Reference material loaded on demand.",
  },
  command: {
    label: "Command",
    icon: TerminalSquare,
    accent: "var(--ok)",
    blurb: "A slash-command that expands into a prompt.",
  },
}

const KIND_ORDER: LibraryKind[] = ["agent", "skill", "command"]

/** A plausible .md body for an existing library item (design-only). */
function mockBody(item: LibraryItem): string {
  const id = item.id
  if (item.kind === "command") {
    return `---\nname: ${item.name}\ndescription: ${item.description}\n---\n\nWhen the user types \`/${id}\`, expand to this prompt and act on it.`
  }
  const head =
    item.kind === "agent"
      ? `---\nname: ${item.name}\ndescription: ${item.description}\n---\n\nYou are ${item.name}.`
      : `---\nname: ${item.name}\ndescription: ${item.description}\n---\n\n# ${item.name}`
  return `${head}\n\n${item.description}\n\n- Be precise.\n- Prefer clarity over cleverness.\n- Cite sources when relevant.`
}

/**
 * Create / view / edit a library prompt. Design-only: every field is editable
 * but nothing is persisted — saving just closes. Mirrors the AgentModal chrome
 * (backdrop-fade + modal-pop). `item === "new"` opens an empty create form;
 * passing a {@link LibraryItem} opens it prefilled for view/edit.
 */
export function PromptModal({ item, onClose }: { item: LibraryItem | "new"; onClose: () => void }) {
  const isNew = item === "new"
  const [kind, setKind] = useState<LibraryKind>(isNew ? "agent" : item.kind)
  const [name, setName] = useState(isNew ? "" : item.name)
  const [description, setDescription] = useState(isNew ? "" : item.description)
  const [body] = useState(() => (isNew ? "" : mockBody(item)))
  const builtin = !isNew && item.builtin === true
  const M = KIND[kind]

  return (
    <Backdrop onClose={onClose}>
      <button
        type="button"
        aria-label="Close"
        onClick={onClose}
        className="backdrop-fade fixed inset-0 cursor-default bg-black/40"
      />
      <div className="modal-pop pop-shadow relative z-10 flex h-[90vh] w-[1180px] max-w-[96vw] flex-col overflow-hidden rounded-2xl border border-border bg-card">
        {/* hero header */}
        <header
          className="relative flex items-center gap-3 px-6 py-5"
          style={{
            background: `linear-gradient(135deg, color-mix(in oklab, ${M.accent} 16%, transparent), transparent)`,
          }}
        >
          <span
            className="flex size-11 shrink-0 items-center justify-center rounded-xl"
            style={{
              background: `color-mix(in oklab, ${M.accent} 18%, transparent)`,
              color: M.accent,
            }}
          >
            <M.icon className="size-[22px]" />
          </span>
          <div className="flex min-w-0 flex-1 flex-col">
            <span className="text-[16px] font-semibold tracking-tight text-foreground">
              {isNew ? "New prompt" : name || M.label}
            </span>
            <span className="text-[12px] text-muted-foreground">
              {isNew ? "Add a system prompt, skill or command to the library." : M.blurb}
            </span>
          </div>
          <button
            onClick={onClose}
            className="flex size-8 shrink-0 items-center justify-center rounded-md text-muted-foreground/60 transition-colors hover:bg-muted/70 hover:text-foreground"
          >
            <X className="size-4" />
          </button>
        </header>

        <div className="flex min-h-0 flex-1 flex-col gap-4 px-6 py-5">
          {/* kind */}
          <Field label="Kind">
            <KindField isNew={isNew} kind={kind} setKind={setKind} builtin={builtin} />
          </Field>

          {/* name + description */}
          <Field label="Name">
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              readOnly={builtin}
              placeholder={kind === "command" ? "deep-review" : "Senior Reviewer"}
              className="w-full rounded-lg border border-border bg-background/60 px-3 py-2 text-[13px] text-foreground transition-colors outline-none read-only:opacity-60 focus:border-(--interactive)/60"
            />
          </Field>
          <Field label="Description" hint="One line — shows on the card">
            <input
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder="What is this prompt for?"
              className="w-full rounded-lg border border-border bg-background/60 px-3 py-2 text-[13px] text-foreground transition-colors outline-none focus:border-(--interactive)/60"
            />
          </Field>

          {/* body — WYSIWYG markdown editor */}
          <div className="flex min-h-0 flex-1 flex-col gap-1.5">
            <div className="flex items-baseline gap-2">
              <span className="text-[10.5px] font-semibold tracking-[0.07em] text-muted-foreground/80 uppercase">
                {kind === "command" ? "Expansion" : "Body"}
              </span>
              {kind === "agent" && (
                <span className="text-[11px] text-muted-foreground/55">
                  Rich text — formatting is saved as markdown
                </span>
              )}
            </div>
            <MarkdownEditor
              key={isNew ? "new" : item.id}
              initialMarkdown={body}
              placeholder="Write the prompt here…"
            />
          </div>

          {!isNew && (
            <p className="flex items-center gap-1.5 text-[11px] text-muted-foreground/60">
              <FileCode2 className="size-3.5" />
              Edits open <code className="font-mono">{mdPath(kind, item.id)}</code>.
            </p>
          )}
        </div>

        {/* footer */}
        <PromptFooter
          isNew={isNew}
          builtin={builtin}
          canSubmit={name.trim().length > 0}
          onClose={onClose}
        />
      </div>
    </Backdrop>
  )
}

/** The `.context-pilot/<dir>/<id>.md` path a library item edits. */
function mdPath(kind: LibraryKind, id: string): string {
  const dir = kind === "agent" ? "agents" : kind === "skill" ? "skills" : "commands"
  return `.context-pilot/${dir}/${id}.md`
}

/** The Kind selector: a three-way picker in create mode, or a read-only badge
 *  (with a Built-in chip) in view/edit mode. Extracted so {@link PromptModal}
 *  stays within the P8 complexity budget. */
function KindField({
  isNew,
  kind,
  setKind,
  builtin,
}: {
  isNew: boolean
  kind: LibraryKind
  setKind: (k: LibraryKind) => void
  builtin: boolean
}) {
  const M = KIND[kind]
  if (!isNew) {
    return (
      <span className="inline-flex w-fit items-center gap-1.5 rounded-lg border border-border bg-muted/40 px-2.5 py-1.5 text-[12px] font-medium text-foreground/80">
        <M.icon className="size-3.5" style={{ color: M.accent }} />
        {M.label}
        {builtin && (
          <span className="ml-1 rounded-full bg-muted/70 px-1.5 py-px text-[9.5px] text-muted-foreground/70">
            Built-in
          </span>
        )}
      </span>
    )
  }
  return (
    <div className="flex gap-2">
      {KIND_ORDER.map((k) => {
        const on = k === kind
        const KM = KIND[k]
        return (
          <button
            key={k}
            onClick={() => setKind(k)}
            className={cn(
              "flex flex-1 items-center gap-2 rounded-lg border px-3 py-2 text-left transition-all",
              on
                ? "border-(--interactive) bg-(--interactive)/[0.07] ring-2 ring-(--interactive)/15"
                : "border-border bg-card hover:border-(--interactive)/40 hover:bg-muted/30",
            )}
          >
            <KM.icon className="size-4 shrink-0" style={{ color: KM.accent }} />
            <span className="text-[12.5px] font-medium text-foreground/85">{KM.label}</span>
          </button>
        )
      })}
    </div>
  )
}

/** The modal's footer actions (Delete / Cancel / Create-or-Save). Design-only:
 *  every button just closes. Extracted so {@link PromptModal} stays within the
 *  P8 budgets. */
function PromptFooter({
  isNew,
  builtin,
  canSubmit,
  onClose,
}: {
  isNew: boolean
  builtin: boolean
  canSubmit: boolean
  onClose: () => void
}) {
  return (
    <footer className="flex h-[60px] shrink-0 items-center gap-2 border-t border-border bg-muted/25 px-6">
      {!isNew && !builtin && (
        <button className="text-[12px] font-medium text-(--danger)/80 transition-colors hover:text-(--danger)">
          Delete
        </button>
      )}
      <button
        onClick={onClose}
        className="ml-auto rounded-lg border border-border px-3.5 py-2 text-[12.5px] font-medium text-foreground/75 transition-colors hover:bg-muted/50"
      >
        Cancel
      </button>
      <button
        onClick={onClose}
        disabled={!canSubmit}
        className="flex items-center gap-2 rounded-lg bg-(--interactive) px-4 py-2 text-[12.5px] font-medium text-(--primary-foreground) transition-all hover:brightness-105 active:scale-[0.98] disabled:cursor-not-allowed disabled:opacity-45"
      >
        <Check className="size-4" strokeWidth={2.5} />
        {isNew ? "Create" : "Save"}
      </button>
    </footer>
  )
}

// ── Import modal ──────────────────────────────────────────────────
interface Source {
  id: string
  name: string
  file: string
  icon: typeof Bot
  accent: string
}

const SOURCES: Source[] = [
  {
    id: "claude-code",
    name: "Claude Code",
    file: "CLAUDE.md · .claude/agents/",
    icon: Sparkles,
    accent: "var(--signal)",
  },
  {
    id: "codex",
    name: "Codex (OpenAI)",
    file: "AGENTS.md",
    icon: Bot,
    accent: "var(--interactive)",
  },
  {
    id: "cursor",
    name: "Cursor",
    file: ".cursor/rules · .cursorrules",
    icon: Code2,
    accent: "var(--ok)",
  },
  { id: "windsurf", name: "Windsurf", file: ".windsurfrules", icon: Wind, accent: "var(--warn)" },
  {
    id: "aider",
    name: "Aider",
    file: "CONVENTIONS.md",
    icon: Terminal,
    accent: "var(--interactive)",
  },
  {
    id: "continue",
    name: "Continue",
    file: ".continue/",
    icon: Boxes,
    accent: "var(--muted-foreground)",
  },
]

/**
 * Import prompts from other agentic systems — Claude Code, Codex, Cursor, …
 * Design-only: scanning each source flips the row to an "Imported ✓" state.
 */
export function ImportModal({ onClose }: { onClose: () => void }) {
  const [done, setDone] = useState<Set<string>>(() => new Set())

  return (
    <Backdrop onClose={onClose}>
      <button
        type="button"
        aria-label="Close"
        onClick={onClose}
        className="backdrop-fade fixed inset-0 cursor-default bg-black/40"
      />
      <div className="modal-pop pop-shadow relative z-10 flex max-h-[88vh] w-[520px] max-w-[94vw] flex-col overflow-hidden rounded-2xl border border-border bg-card">
        <header className="flex items-center gap-3 border-b border-border px-6 py-4">
          <span className="flex size-9 shrink-0 items-center justify-center rounded-xl bg-(--interactive)/14 text-(--interactive)">
            <Download className="size-[18px]" />
          </span>
          <div className="flex min-w-0 flex-1 flex-col">
            <span className="text-[15px] font-semibold tracking-tight text-foreground">
              Import prompts
            </span>
            <span className="text-[11.5px] text-muted-foreground">
              Bring rules & agents over from other tools into your global library.
            </span>
          </div>
          <button
            onClick={onClose}
            className="flex size-8 shrink-0 items-center justify-center rounded-md text-muted-foreground/60 transition-colors hover:bg-muted/70 hover:text-foreground"
          >
            <X className="size-4" />
          </button>
        </header>

        <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto p-4">
          {SOURCES.map((s, i) => {
            const imported = done.has(s.id)
            return (
              <div
                key={s.id}
                style={{ animationDelay: `${i * 35}ms` }}
                className="opt-rise card-shadow flex items-center gap-3 rounded-xl border border-border bg-card px-3.5 py-2.5"
              >
                <span
                  className="flex size-8 shrink-0 items-center justify-center rounded-lg"
                  style={{
                    background: `color-mix(in oklab, ${s.accent} 15%, transparent)`,
                    color: s.accent,
                  }}
                >
                  <s.icon className="size-[17px]" />
                </span>
                <div className="flex min-w-0 flex-1 flex-col leading-tight">
                  <span className="truncate text-[13px] font-medium text-foreground/90">
                    {s.name}
                  </span>
                  <span className="truncate font-mono text-[10.5px] text-muted-foreground/65">
                    {s.file}
                  </span>
                </div>
                <button
                  onClick={() => setDone((d) => new Set(d).add(s.id))}
                  disabled={imported}
                  className={cn(
                    "flex shrink-0 items-center gap-1.5 rounded-lg px-3 py-1.5 text-[11.5px] font-medium transition-all",
                    imported
                      ? "cursor-default bg-(--ok)/14 text-(--ok)"
                      : "border border-border text-foreground/75 hover:border-(--interactive)/50 hover:text-foreground active:scale-[0.97]",
                  )}
                >
                  {imported ? (
                    <Check className="size-3.5" strokeWidth={2.5} />
                  ) : (
                    <Download className="size-3.5" />
                  )}
                  {imported ? "Imported" : "Import"}
                </button>
              </div>
            )
          })}
        </div>

        <footer className="flex h-[56px] shrink-0 items-center border-t border-border bg-muted/25 px-6">
          <span className="text-[11px] text-muted-foreground/65">
            Import rules &amp; agents from your other tools.
          </span>
          <button
            onClick={onClose}
            className="ml-auto rounded-lg bg-(--interactive) px-4 py-2 text-[12.5px] font-medium text-(--primary-foreground) transition-all hover:brightness-105 active:scale-[0.98]"
          >
            Done
          </button>
        </footer>
      </div>
    </Backdrop>
  )
}

// ── shared chrome ─────────────────────────────────────────────────
function Backdrop({ children, onClose }: { children: React.ReactNode; onClose: () => void }) {
  // Escape-to-close as a document listener rather than an onKeyDown on the
  // dialog container: the container carries the (non-interactive) `dialog` role,
  // so a JSX key handler on it trips jsx-a11y — and a document listener closes
  // regardless of where focus sits inside the modal.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose()
    }
    window.addEventListener("keydown", onKey)
    return () => window.removeEventListener("keydown", onKey)
  }, [onClose])
  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center p-4"
      role="dialog"
      aria-modal
    >
      {children}
    </div>
  )
}

function Field({
  label,
  hint,
  children,
}: {
  label: string
  hint?: string
  children: React.ReactNode
}) {
  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex items-baseline gap-2">
        <span className="text-[10.5px] font-semibold tracking-[0.07em] text-muted-foreground/80 uppercase">
          {label}
        </span>
        {hint && <span className="text-[11px] text-muted-foreground/55">{hint}</span>}
      </div>
      {children}
    </div>
  )
}
