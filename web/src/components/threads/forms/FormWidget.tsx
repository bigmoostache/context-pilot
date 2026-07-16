// ── FormWidget — the interactive form rendered in place of a ```form``` block ─
//
// One widget per ```form``` block (docs/forms.md §2/§4). Unanswered → an
// interactive form (draft persisted per form-id in localStorage, all fields
// mandatory, client-gated submit). Answered → a locked read-only summary
// derived from the matching ```form-answer``` message (§5). On submit it hands
// the composed {id, answer} entries to the parent, which sends the
// ```form-answer``` message via the EXISTING send path — no backend form state.

import { useMemo, useState } from "react"
import { CheckCircle2 } from "lucide-react"
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

/** A single labelled field row (label + the type's input). */
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
    <div className="flex flex-col gap-1.5">
      <span className="text-[12.5px] font-medium text-foreground/85">{field.label}</span>
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
  return Array.isArray(v) ? v.join(", ") : v
}

/** Locked read-only view once the form is answered (§4): the title + each
 *  field's label and submitted value, no inputs. */
function LockedForm({ spec, answer }: { spec: FormSpec; answer: FormAnswer }) {
  const byId = new Map(answer.answers.map((a) => [a.id, a.answer]))
  return (
    <div className="my-2 rounded-xl border border-(--signal)/40 bg-(--signal)/5 px-4 py-3">
      <div className="mb-2 flex items-center gap-1.5 text-[12px] font-semibold text-(--signal)">
        <CheckCircle2 className="size-3.5" />
        {spec.title ?? "Form"} · answered
      </div>
      <div className="flex flex-col gap-1.5">
        {spec.fields.map((f) => (
          <div key={f.id} className="flex gap-2 text-[12px]">
            <span className="shrink-0 font-medium text-foreground/70">{f.label}</span>
            <span className="min-w-0 wrap-break-word text-foreground/90">
              {showAnswer(byId.get(f.id))}
            </span>
          </div>
        ))}
      </div>
    </div>
  )
}

/**
 * Interactive or locked form widget for one ```form``` block.
 *
 * `answer` is the derived matching ```form-answer``` (or null): when present the
 * form renders locked; otherwise it is an editable form whose submit is disabled
 * until every field is filled (client-side gate — the backend holds no form
 * state, docs/forms.md §7). `onSubmit` hands the composed entries to the parent,
 * which sends the answer message through the existing send path.
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
  // Locked branch: an answer already exists in the thread → read-only summary.
  const [values, setValues] = useState<Record<string, AnswerValue>>(() => loadDraft(draftKey, spec))
  const [sent, setSent] = useState(false)

  const canSubmit = useMemo(
    () => spec.fields.every((f) => fieldFilled(f, values[f.id])),
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
    if (!canSubmit || sent) return
    const entries: FormAnswerEntry[] = spec.fields.map((f) => ({
      id: f.id,
      answer: values[f.id] ?? (isListField(f) ? [] : ""),
    }))
    setSent(true)
    try {
      localStorage.removeItem(draftKey)
    } catch {
      // ignore — draft cleanup is best-effort
    }
    onSubmit(spec.formId, entries)
  }

  return (
    <div className="card-shadow my-2 rounded-xl border border-border bg-card px-4 py-3">
      {spec.title && (
        <div className="mb-3 text-[13px] font-semibold text-foreground/90">{spec.title}</div>
      )}
      <div className="flex flex-col gap-3.5">
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
      <button
        type="button"
        onClick={submit}
        disabled={!canSubmit || sent}
        className="mt-4 flex items-center gap-1.5 rounded-lg bg-(--signal) px-3.5 py-2 text-[12.5px] font-medium text-(--primary-foreground) transition-[filter] hover:brightness-105 disabled:opacity-40 disabled:hover:brightness-100"
      >
        {spec.submit ?? "Submit"}
      </button>
    </div>
  )
}
