// ── FormWidget — the interactive form rendered in place of a ```form``` block ─
//
// The mobile twin of `components/threads/forms/FormWidget`. One widget per
// ```form``` block (docs/forms.md §2/§4). Unanswered → an interactive form
// (draft persisted per form-id in localStorage). Answered → a locked read-only
// receipt derived from the matching ```form-answer``` message (§5). On submit it
// hands the composed {id, answer} entries to the parent, which sends the
// ```form-answer``` message via the EXISTING send path — no backend form state.
//
// DELIBERATELY EFFECT-LIGHT (T643), byte-for-byte the desktop twin's logic minus
// the removed machinery (two-click arm, 3s timer, scrollIntoView + CSS.escape,
// auto-appended comment field). One state field (the value map), no layout
// effects. The only fork from desktop is that `./FormFields` resolves to the
// MOBILE field renderers (16px inputs / touch option rows) within this tree.

import { useMemo, useState } from "react"
import { Check, CheckCircle2, ClipboardList } from "lucide-react"
import { formatTs } from "@/lib/support/threadMessages"
import { FieldInput } from "./FormFields"
import type { AnswerValue, FormAnswer, FormAnswerEntry, FormField, FormSpec } from "./helpers"

/** A form field that answers with a list (multi/files) — needs ≥1 to be valid. */
function isListField(f: FormField): boolean {
  return f.type === "multi" || f.type === "files"
}

/** Whether one field's current value satisfies "mandatory". A list field needs
 *  ≥1 entry; `confirm` must be armed (`"true"`); `toggle` is always satisfied
 *  (a switch always carries a boolean); every other scalar needs a value. */
function fieldFilled(f: FormField, v: AnswerValue | undefined): boolean {
  if (v === undefined) return f.type === "toggle"
  if (isListField(f)) return Array.isArray(v) && v.length > 0
  if (f.type === "confirm") return v === "true"
  if (f.type === "toggle") return true
  return typeof v === "string" && v.trim().length > 0
}

/** Seed the value map: a scalar field defaults to "" (toggle to "false"), a list
 *  field to []. */
function seedValues(spec: FormSpec): Record<string, AnswerValue> {
  const out: Record<string, AnswerValue> = {}
  for (const f of spec.fields) {
    out[f.id] = isListField(f) ? [] : f.type === "toggle" ? "false" : ""
  }
  return out
}

/** Read the persisted draft for this form, merged onto the seed (so a new field
 *  added since the draft was written still gets its default). */
function loadDraft(key: string, spec: FormSpec): Record<string, AnswerValue> {
  const base = seedValues(spec)
  try {
    const raw = localStorage.getItem(key)
    if (raw == null) return base
    const parsed: unknown = JSON.parse(raw)
    if (parsed && typeof parsed === "object") {
      for (const f of spec.fields) {
        const v = (parsed as Record<string, unknown>)[f.id]
        if (typeof v === "string" || Array.isArray(v)) base[f.id] = v as AnswerValue
      }
    }
  } catch {
    // malformed draft — fall back to the seed
  }
  return base
}

/** A single labelled field row (plain label above the type's input — an iOS form
 *  labels fields plainly, it doesn't number them). */
function FieldRow({
  field,
  value,
  onChange,
  disabled,
  agentId,
}: {
  field: FormField
  value: AnswerValue
  onChange: (v: AnswerValue) => void
  disabled: boolean
  agentId: string
}) {
  return (
    <div className="flex flex-col gap-2">
      <label className="px-0.5 text-[13px] font-medium text-foreground/80">{field.label}</label>
      <FieldInput
        field={field}
        value={value}
        onChange={onChange}
        disabled={disabled}
        agentId={agentId}
      />
    </div>
  )
}

/** Render one answer value as a display string (a list joins with commas). */
function showAnswer(v: AnswerValue | undefined): string {
  if (v === undefined) return "—"
  if (Array.isArray(v)) return v.length > 0 ? v.join(", ") : "—"
  return v.trim().length > 0 ? v : "—"
}

/** Locked read-only receipt once the form is answered (§4): a sealed header plus
 *  a definition list of each field's label and submitted value, no inputs. */
function LockedForm({ spec, answer }: { spec: FormSpec; answer: FormAnswer }) {
  const byId = new Map(answer.answers.map((a) => [a.id, a.answer]))
  return (
    <div className="my-1.5 overflow-hidden rounded-2xl border border-(--signal)/25 bg-linear-to-b from-(--signal)/10 to-(--signal)/2 shadow-(--shadow-pop) backdrop-blur-xl backdrop-saturate-150">
      <div className="flex items-center gap-2 border-b border-signal/15 px-3 py-2">
        <span className="flex size-5 items-center justify-center rounded-full bg-(--signal) text-(--primary-foreground)">
          <Check className="size-3" strokeWidth={3} />
        </span>
        <div className="flex min-w-0 flex-col">
          <span className="truncate text-[12px] font-semibold text-foreground/90">
            {spec.title ?? "Form"}
          </span>
          <span className="text-[10px] font-medium tracking-wide text-(--signal)/80 uppercase">
            {answer.submittedAt === undefined
              ? "Submitted"
              : `Submitted · ${formatTs(answer.submittedAt)}`}
          </span>
        </div>
      </div>
      <dl className="divide-y divide-(--signal)/10">
        {spec.fields.map((f) => (
          <div key={f.id} className="flex flex-col gap-0.5 px-3 py-1.5">
            <dt className="text-[10px] font-semibold tracking-wide text-muted-foreground/70 uppercase">
              {f.label}
            </dt>
            <dd className="min-w-0 text-[12.5px] wrap-break-word text-foreground/90">
              {showAnswer(byId.get(f.id))}
            </dd>
          </div>
        ))}
      </dl>
    </div>
  )
}

/** The card header: an icon chip, the title, and the field count. */
function FormHeader({ title, count }: { title: string | undefined; count: number }) {
  return (
    <div className="flex items-center gap-2 border-b border-border/40 px-3.5 py-2.5">
      <span className="flex size-5 items-center justify-center rounded-full bg-(--signal)/12 text-(--signal) ring-1 ring-(--signal)/20">
        <ClipboardList className="size-3" />
      </span>
      <div className="flex min-w-0 flex-col">
        <span className="truncate text-[12px] font-semibold text-foreground/90">
          {title ?? "Form"}
        </span>
        <span className="text-[10px] font-medium tracking-wide text-muted-foreground/60 uppercase">
          {count} {count === 1 ? "field" : "fields"}
        </span>
      </div>
    </div>
  )
}

/** The card footer: a live progress hint on the left, the submit on the right. */
function FormFooter({
  filled,
  total,
  label,
  disabled,
  onSubmit,
}: {
  filled: number
  total: number
  label: string
  disabled: boolean
  onSubmit: () => void
}) {
  const complete = filled >= total
  return (
    <div className="flex items-center justify-between gap-3 border-t border-border/40 px-3.5 py-2.5">
      <span className="flex items-center gap-1.5 text-[11px] font-medium">
        {complete ? (
          <span className="flex items-center gap-1.5 text-muted-foreground/70">
            <CheckCircle2 className="size-3.5 text-(--signal)" /> All set
          </span>
        ) : (
          <span className="text-muted-foreground/70 tabular-nums">
            {filled} of {total} answered
          </span>
        )}
      </span>
      <button
        type="button"
        onClick={onSubmit}
        disabled={disabled}
        className="flex items-center gap-1.5 rounded-lg bg-(--signal) px-3.5 py-2 text-[13px] font-semibold text-(--primary-foreground) shadow-sm transition-[filter,opacity] active:brightness-105 disabled:opacity-35 disabled:active:brightness-100"
      >
        {label}
      </button>
    </div>
  )
}

/**
 * Interactive or locked form widget for one ```form``` block.
 *
 * `answer` is the derived matching ```form-answer``` (or null): when present the
 * form renders a locked receipt; otherwise it is an editable form. `onSubmit`
 * hands the composed entries to the parent, which sends the answer message
 * through the existing send path. Submit is a single click — a blank mandatory
 * field sends its empty seed (the backend holds no form state, docs/forms.md §7).
 */
export function FormWidget({
  spec,
  agentId,
  answer,
  draftKey,
  onSubmit,
}: {
  spec: FormSpec
  agentId: string
  answer: FormAnswer | null
  /** localStorage key for this form's unsent draft (per agent/thread/form-id). */
  draftKey: string
  onSubmit: (formId: string, entries: FormAnswerEntry[]) => void
}) {
  const [values, setValues] = useState<Record<string, AnswerValue>>(() =>
    loadDraft(draftKey, spec),
  )
  const [sent, setSent] = useState(false)

  const filled = useMemo(
    () => spec.fields.filter((f) => fieldFilled(f, values[f.id])).length,
    [spec.fields, values],
  )

  if (answer) return <LockedForm spec={spec} answer={answer} />

  const setValue = (id: string, v: AnswerValue) => {
    setValues((prev) => {
      const next = { ...prev, [id]: v }
      try {
        localStorage.setItem(draftKey, JSON.stringify(next))
      } catch {
        // storage full / disabled — the draft is best-effort, submit still works
      }
      return next
    })
  }

  const submit = () => {
    if (sent) return
    const entries: FormAnswerEntry[] = spec.fields.map((f) => {
      // An un-armed confirm sends a clean "false" rather than its empty seed.
      if (f.type === "confirm" && values[f.id] !== "true") return { id: f.id, answer: "false" }
      return { id: f.id, answer: values[f.id] ?? (isListField(f) ? [] : "") }
    })
    setSent(true)
    try {
      localStorage.removeItem(draftKey)
    } catch {
      // ignore — draft cleanup is best-effort
    }
    onSubmit(spec.formId, entries)
  }

  return (
    <div className="my-1.5 overflow-hidden rounded-2xl border border-border/40 bg-card/55 shadow-(--shadow-pop) backdrop-blur-xl backdrop-saturate-150">
      <FormHeader title={spec.title} count={spec.fields.length} />
      <div className="flex flex-col gap-4 p-3.5">
        {spec.fields.map((f) => (
          <FieldRow
            key={f.id}
            field={f}
            value={values[f.id] ?? ""}
            onChange={(v) => setValue(f.id, v)}
            disabled={sent}
            agentId={agentId}
          />
        ))}
      </div>
      <FormFooter
        filled={filled}
        total={spec.fields.length}
        label={spec.submit ?? "Submit"}
        disabled={sent}
        onSubmit={submit}
      />
    </div>
  )
}
