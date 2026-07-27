// ── FormWidget — the interactive form rendered in place of a ```form``` block ─
//
// The mobile twin of `components/threads/forms/FormWidget`. One widget per
// ```form``` block (docs/forms.md §2/§4). Unanswered → an interactive form
// (draft persisted per form-id in localStorage, all fields mandatory,
// client-gated submit). Answered → a locked read-only receipt derived from the
// matching ```form-answer``` message (§5). On submit it hands the composed
// {id, answer} entries to the parent, which sends the ```form-answer``` message
// via the EXISTING send path — no backend form state.
//
// All widget logic (draft round-trip, two-click incomplete-submit gate, soft
// confirm fields, locked receipt) is byte-identical to the desktop twin; the
// only fork is that `./FormFields` resolves to the MOBILE field renderers
// (16px inputs / touch option rows) within this tree. This real (marker-less)
// twin is what routes the mobile field renderers in — a stub would pull the
// desktop ones and bypass the touch sizing (design-mobile.md §3.3).

import { useEffect, useMemo, useRef, useState } from "react"
import { Check, CheckCircle2, ClipboardList, AlertTriangle } from "lucide-react"
import { formatTs } from "@/lib/support/threadMessages"
import { FieldInput } from "./FormFields"
import type { AnswerValue, FormAnswer, FormAnswerEntry, FormField, FormSpec } from "./helpers"

/** Reserved id for the always-appended optional free-text comment field. */
const COMMENT_ID = "__comment"
const COMMENT_FIELD: FormField = {
  id: COMMENT_ID,
  label: "Anything to add? (optional)",
  type: "text",
}

/** Append the optional comment field to a spec (unless the form already carries
 *  one under the reserved id). Every rendered form ends with a free-text box so
 *  the user can leave a note — it never blocks submit and is omitted from the
 *  locked receipt when left blank. */
function withComment(spec: FormSpec): FormSpec {
  if (spec.fields.some((f) => f.id === COMMENT_ID)) return spec
  return { ...spec, fields: [...spec.fields, COMMENT_FIELD] }
}

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

/** A single labelled field row (plain label above the type's input). When
 *  `highlight` is set (a blank mandatory field at the moment the incomplete
 *  submit arms) the row's label turns amber and the input rings amber, pointing
 *  the user straight at what's unfilled. `data-field-id` lets the widget scroll
 *  the first missing row into view. No numeric prefix / indent — an iOS form
 *  labels fields plainly, it doesn't number them (the "more mobile native" ask). */
function FieldRow({
  field,
  value,
  onChange,
  disabled,
  highlight,
  agentId,
}: {
  field: FormField
  value: AnswerValue
  onChange: (v: AnswerValue) => void
  disabled: boolean
  highlight: boolean
  agentId: string
}) {
  return (
    <div className="flex flex-col gap-2" data-field-id={field.id}>
      <label
        className={`px-0.5 text-[13px] font-medium ${highlight ? "text-(--warn)" : "text-foreground/80"}`}
      >
        {field.label}
      </label>
      <div className={highlight ? "rounded-xl ring-1 ring-(--warn)/50" : undefined}>
        <FieldInput
          field={field}
          value={value}
          onChange={onChange}
          disabled={disabled}
          agentId={agentId}
        />
      </div>
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
  // Skip the synthetic comment field when the user left it blank — an empty
  // "Anything to add?" row is pure noise in the receipt.
  const rows = spec.fields.filter((f) => {
    if (f.id !== COMMENT_ID) return true
    const v = byId.get(f.id)
    return typeof v === "string" && v.trim().length > 0
  })
  return (
    <div className="rise my-1.5 overflow-hidden rounded-2xl border border-(--signal)/25 bg-linear-to-b from-(--signal)/10 to-(--signal)/2 shadow-(--shadow-pop) backdrop-blur-xl backdrop-saturate-150">
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
        {rows.map((f) => (
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

/** The card footer: a live progress hint on the left, the submit on the right.
 *  Confirm fields are soft — when `unconfirmed` > 0 an amber warning shows but
 *  the submit stays enabled (the gate ignores confirm fields). Submitting an
 *  INCOMPLETE form is a two-click action: the first click arms (`armed`), the
 *  button turns amber and reads "Confirm incomplete form"; the second submits. */
function FormFooter({
  filled,
  total,
  label,
  disabled,
  unconfirmed,
  armed,
  onSubmit,
}: {
  filled: number
  total: number
  label: string
  disabled: boolean
  unconfirmed: number
  /** the incomplete-submit confirm is armed (button shows the confirm label). */
  armed: boolean
  onSubmit: () => void
}) {
  const complete = filled >= total
  const button = armed
    ? "bg-(--warn) text-(--primary-foreground) active:brightness-105"
    : "bg-(--signal) text-(--primary-foreground) active:brightness-105"
  return (
    <div className="flex items-center justify-between gap-3 border-t border-border/40 px-3.5 py-2.5">
      {/* Screen-reader announcement of the arm/disarm flip (the amber button
          change is otherwise visual-only). */}
      <span className="sr-only" role="status" aria-live="polite">
        {armed ? "Form incomplete — click submit again to confirm." : ""}
      </span>
      <span className="flex items-center gap-1.5 text-[11px] font-medium">
        {unconfirmed > 0 ? (
          <span className="flex items-center gap-1.5 text-(--warn)">
            <AlertTriangle className="size-3.5" />
            {unconfirmed} confirmation{unconfirmed === 1 ? "" : "s"} not armed
          </span>
        ) : complete ? (
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
        className={`flex items-center gap-1.5 rounded-lg px-3.5 py-2 text-[13px] font-semibold shadow-sm transition-[filter,opacity,background-color] disabled:opacity-35 disabled:active:brightness-100 ${button}`}
      >
        {armed && <AlertTriangle className="size-3.5" />}
        {label}
      </button>
    </div>
  )
}

/**
 * Interactive or locked form widget for one ```form``` block.
 *
 * `answer` is the derived matching ```form-answer``` (or null): when present the
 * form renders a locked receipt; otherwise it is an editable form whose submit
 * is disabled until every field is filled (client-side gate — the backend holds
 * no form state, docs/forms.md §7). `onSubmit` hands the composed entries to the
 * parent, which sends the answer message through the existing send path.
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
  // Every form gets a trailing optional free-text comment field appended, so
  // the user can always leave a note — see withComment (never blocks submit,
  // hidden from the receipt when blank).
  const fullSpec = useMemo(() => withComment(spec), [spec])
  const [values, setValues] = useState<Record<string, AnswerValue>>(() =>
    loadDraft(draftKey, fullSpec),
  )
  const [sent, setSent] = useState(false)
  // Two-click guard for submitting an INCOMPLETE form: the first click arms
  // (button turns amber, reads "Confirm incomplete form"), the second submits.
  const [armed, setArmed] = useState(false)

  const filled = useMemo(
    () => fullSpec.fields.filter((f) => fieldFilled(f, values[f.id])).length,
    [fullSpec.fields, values],
  )
  // The submit is ALWAYS clickable (only a post-submit `sent` guard applies):
  // no field blocks it. Unfilled fields answer their empty seed, and an un-armed
  // `confirm` answers "false" — the footer surfaces progress + an amber warning
  // for any un-armed confirm as information, never as a block (docs/forms.md §7).
  const unconfirmed = useMemo(
    () => fullSpec.fields.filter((f) => f.type === "confirm" && values[f.id] !== "true").length,
    [fullSpec.fields, values],
  )
  // A form is "incomplete" when a MANDATORY field is unfilled. The optional
  // trailing comment (__comment) and the soft `confirm` fields (un-armed answers
  // "false" by design) are NOT mandatory, so they never mark a form incomplete.
  // `missingIds` is the ordered list of those unfilled mandatory fields, used
  // both for the two-click gate (non-empty = incomplete) and to ring/scroll the
  // blank fields when the incomplete-submit confirm arms.
  const missingIds = useMemo(
    () =>
      fullSpec.fields
        .filter((f) => f.id !== COMMENT_ID && f.type !== "confirm" && !fieldFilled(f, values[f.id]))
        .map((f) => f.id),
    [fullSpec.fields, values],
  )
  const incomplete = missingIds.length > 0

  const cardRef = useRef<HTMLDivElement>(null)

  // Armed incomplete-submit confirm auto-reverts after 3s of inactivity — if the
  // user doesn't make the second (confirming) click in time, the button drops
  // back to its normal label so a stale "Confirm incomplete form" never lingers.
  useEffect(() => {
    if (!armed) return
    const t = setTimeout(() => setArmed(false), 3000)
    return () => clearTimeout(t)
  }, [armed])

  // On arm, scroll the FIRST missing mandatory field into view so the two-click
  // confirm points the user straight at what's unfilled (the rows also ring
  // amber via `highlight`).
  useEffect(() => {
    if (!armed) return
    const firstMissing = missingIds[0]
    if (firstMissing === undefined) return
    cardRef.current
      ?.querySelector(`[data-field-id="${CSS.escape(firstMissing)}"]`)
      ?.scrollIntoView({ block: "center", behavior: "smooth" })
  }, [armed, missingIds])

  if (answer) return <LockedForm spec={fullSpec} answer={answer} />

  const setValue = (id: string, v: AnswerValue) => {
    // Any edit disarms the incomplete-submit confirm — the user changed the
    // form, so the next submit re-evaluates completeness from scratch.
    setArmed(false)
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
    // Incomplete form: first click arms the confirm, second click submits.
    if (incomplete && !armed) {
      setArmed(true)
      return
    }
    const entries: FormAnswerEntry[] = fullSpec.fields.map((f) => {
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
    onSubmit(fullSpec.formId, entries)
  }

  return (
    <div
      ref={cardRef}
      className="rise my-1.5 overflow-hidden rounded-2xl border border-border/40 bg-card/55 shadow-(--shadow-pop) backdrop-blur-xl backdrop-saturate-150"
    >
      <FormHeader title={fullSpec.title} count={fullSpec.fields.length} />
      <div className="flex flex-col gap-4 p-3.5">
        {fullSpec.fields.map((f) => (
          <FieldRow
            key={f.id}
            field={f}
            value={values[f.id] ?? ""}
            onChange={(v) => setValue(f.id, v)}
            disabled={sent}
            highlight={armed && missingIds.includes(f.id)}
            agentId={agentId}
          />
        ))}
      </div>
      <FormFooter
        filled={filled}
        total={fullSpec.fields.length}
        label={armed && incomplete ? "Confirm incomplete form" : (fullSpec.submit ?? "Submit")}
        disabled={sent}
        unconfirmed={unconfirmed}
        armed={armed && incomplete}
        onSubmit={submit}
      />
    </div>
  )
}
