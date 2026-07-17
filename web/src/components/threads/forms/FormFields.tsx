// ── Form field input renderers (one per v1 type, docs/forms.md §3) ──────
//
// Each renderer is a controlled input over a single field's answer value:
// scalar (single/text/number/date/toggle/confirm) or string list (multi/files).
// They are presentational — the owning FormWidget holds the value map, the
// draft persistence, and the submit gate. `files` is the sole async one: it
// uploads on pick via the existing `.uploads/` path and answers with paths.

import { useState } from "react"
import { Check, Upload, Loader2, AlertTriangle, X, CalendarIcon } from "lucide-react"
import { format, parse } from "date-fns"
import { uploadUnique } from "@/lib/api"
import { Calendar } from "@/components/ui/calendar"
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover"
import type { AnswerValue, FormField } from "./helpers"

/** Shared props for every field input. `value` is the current answer (scalar or
 *  list); `onChange` writes it back into the FormWidget value map. */
interface FieldProps {
  field: FormField
  value: AnswerValue
  onChange: (v: AnswerValue) => void
  disabled: boolean
  /** owning agent — only `files` needs it (immediate upload). */
  agentId: string
}

/** Normalise a value to a scalar string (empty when it's a list). */
function asScalar(v: AnswerValue): string {
  return Array.isArray(v) ? "" : v
}

/** Normalise a value to a string list (single-wraps a non-empty scalar). */
function asList(v: AnswerValue): string[] {
  if (Array.isArray(v)) return v
  return v ? [v] : []
}

/** One selectable option row (radio/checkbox). The control is a custom glyph so
 *  the checked state reads crisply against the signal-tinted selected surface. */
function OptionRow({
  kind,
  on,
  disabled,
  onPick,
  label,
  detail,
}: {
  kind: "radio" | "checkbox"
  on: boolean
  disabled: boolean
  onPick: () => void
  label: string
  detail?: string | undefined
}) {
  return (
    <label
      className={`group flex cursor-pointer items-start gap-2.5 rounded-xl border px-3 py-1.5 text-[12.5px] transition-all ${
        on
          ? "border-(--signal)/70 bg-(--signal)/8 shadow-[inset_0_0_0_1px_var(--signal)]"
          : "border-border/70 hover:border-border hover:bg-muted/40"
      } ${disabled ? "pointer-events-none opacity-60" : ""}`}
    >
      <input
        type={kind}
        checked={on}
        disabled={disabled}
        onChange={onPick}
        className="sr-only"
      />
      <span
        className={`mt-0.5 flex size-4 shrink-0 items-center justify-center border transition-colors ${
          kind === "radio" ? "rounded-full" : "rounded-sm"
        } ${on ? "border-(--signal) bg-(--signal) text-(--primary-foreground)" : "border-border/80 bg-background"}`}
      >
        {on &&
          (kind === "radio" ? (
            <span className="size-1.5 rounded-full bg-current" />
          ) : (
            <Check className="size-3" strokeWidth={3} />
          ))}
      </span>
      <span className="min-w-0">
        <span className="block font-medium text-foreground/90">{label}</span>
        {detail && <span className="block text-[11.5px] text-muted-foreground/70">{detail}</span>}
      </span>
    </label>
  )
}

/** single — radio over `{label, detail}` options, plus an optional free-text
 *  "Other…" choice (`allow-other`). The answer is the chosen label or the typed
 *  string. */
function SingleField({ field, value, onChange, disabled }: FieldProps) {
  const sel = asScalar(value)
  const labels = (field.options ?? []).map((o) => o.label)
  const isOther = field.allowOther === true && sel !== "" && !labels.includes(sel)
  return (
    <div className="flex flex-col gap-1">
      {(field.options ?? []).map((o) => (
        <OptionRow
          key={o.label}
          kind="radio"
          on={sel === o.label}
          disabled={disabled}
          onPick={() => onChange(o.label)}
          label={o.label}
          detail={o.detail}
        />
      ))}
      {field.allowOther === true && (
        <label
          className={`group flex cursor-pointer items-start gap-2.5 rounded-xl border px-3 py-1.5 text-[12.5px] transition-all ${
            isOther
              ? "border-(--signal)/70 bg-(--signal)/8 shadow-[inset_0_0_0_1px_var(--signal)]"
              : "border-border/70 hover:border-border hover:bg-muted/40"
          } ${disabled ? "pointer-events-none opacity-60" : ""}`}
        >
          <input
            type="radio"
            checked={isOther}
            disabled={disabled}
            onChange={() => onChange(" ")}
            className="sr-only"
          />
          <span
            className={`mt-0.5 flex size-4 shrink-0 items-center justify-center rounded-full border transition-colors ${isOther ? "border-(--signal) bg-(--signal) text-(--primary-foreground)" : "border-border/80 bg-background"}`}
          >
            {isOther && <span className="size-1.5 rounded-full bg-current" />}
          </span>
          <span className="min-w-0 flex-1">
            <span className="block font-medium text-foreground/90">Other…</span>
            {isOther && (
              <input
                autoFocus
                type="text"
                value={sel.trim()}
                disabled={disabled}
                onChange={(e) => onChange(e.target.value || " ")}
                placeholder="Type your answer"
                className="mt-1.5 w-full rounded-lg border border-border bg-background px-2.5 py-1.5 text-[12.5px] transition-shadow outline-none focus:border-(--signal) focus:shadow-[0_0_0_3px_var(--signal)]/15"
              />
            )}
          </span>
        </label>
      )}
    </div>
  )
}

/** multi — checkbox group; the answer is the list of chosen labels. */
function MultiField({ field, value, onChange, disabled }: FieldProps) {
  const sel = asList(value)
  const toggle = (label: string) => {
    onChange(sel.includes(label) ? sel.filter((l) => l !== label) : [...sel, label])
  }
  return (
    <div className="flex flex-col gap-1">
      {(field.options ?? []).map((o) => (
        <OptionRow
          key={o.label}
          kind="checkbox"
          on={sel.includes(o.label)}
          disabled={disabled}
          onPick={() => toggle(o.label)}
          label={o.label}
          detail={o.detail}
        />
      ))}
    </div>
  )
}

const SCALAR_INPUT =
  "w-full rounded-lg border border-border/80 bg-background px-3 py-1.5 text-[12.5px] text-foreground/90 outline-none transition-shadow focus:border-(--signal) focus:shadow-[0_0_0_3px_var(--signal)]/15 disabled:opacity-60"

/** text / number — a single controlled input keyed off the field type (`date`
 *  is handled separately by {@link DateField}). */
function ScalarField({ field, value, onChange, disabled }: FieldProps) {
  const type = field.type === "number" ? "number" : "text"
  return (
    <input
      type={type}
      value={asScalar(value)}
      disabled={disabled}
      onChange={(e) => onChange(e.target.value)}
      placeholder={field.type === "text" ? "Type your answer" : undefined}
      className={SCALAR_INPUT}
    />
  )
}

/** ISO storage format for a `date` answer — the form-answer contract (docs/
 *  forms.md §3). */
const ISO_DATE = "yyyy-MM-dd"

/** Parse a stored ISO date string into a Date, or `undefined` when empty/invalid
 *  (so a blank field opens the calendar on today with nothing selected). */
function parseISODate(v: string): Date | undefined {
  if (v.trim().length === 0) return undefined
  const d = parse(v, ISO_DATE, new Date())
  return Number.isNaN(d.getTime()) ? undefined : d
}

/** date — a shadcn Calendar in a Popover. The trigger shows the selected date
 *  (long form) or a placeholder; picking a day writes the ISO string back and
 *  closes the popover. The answer is always ISO `yyyy-MM-dd`. */
function DateField({ value, onChange, disabled }: FieldProps) {
  const [open, setOpen] = useState(false)
  const iso = asScalar(value)
  const selected = parseISODate(iso)
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger
        disabled={disabled}
        className={`flex w-full items-center gap-2 rounded-lg border border-border/80 bg-background px-3 py-1.5 text-left text-[12.5px] transition-shadow outline-none focus:border-(--signal) focus:shadow-[0_0_0_3px_var(--signal)]/15 disabled:opacity-60 ${
          selected ? "text-foreground/90" : "text-muted-foreground/70"
        }`}
      >
        <CalendarIcon className="size-3.5 shrink-0 text-muted-foreground/60" />
        {selected ? format(selected, "PPP") : "Pick a date"}
      </PopoverTrigger>
      <PopoverContent className="w-auto p-0" align="start">
        <Calendar
          mode="single"
          selected={selected}
          {...(selected ? { defaultMonth: selected } : {})}
          onSelect={(d) => {
            onChange(d ? format(d, ISO_DATE) : "")
            setOpen(false)
          }}
          autoFocus
        />
      </PopoverContent>
    </Popover>
  )
}

/** toggle — a neutral on/off switch with a labelled state; the answer is
 *  `"true"` / `"false"`. */
function ToggleField({ value, onChange, disabled }: FieldProps) {
  const on = asScalar(value) === "true"
  return (
    <div className="flex items-center gap-2.5">
      <button
        type="button"
        role="switch"
        aria-checked={on}
        disabled={disabled}
        onClick={() => onChange(on ? "false" : "true")}
        className={`relative inline-flex h-6 w-11 items-center rounded-full transition-colors ${on ? "bg-(--signal)" : "bg-muted"} disabled:opacity-60`}
      >
        <span
          className={`inline-block size-5 transform rounded-full bg-white shadow-sm transition-transform ${on ? "translate-x-5" : "translate-x-0.5"}`}
        />
      </button>
      <span className="text-[11.5px] font-medium text-muted-foreground/70">{on ? "On" : "Off"}</span>
    </div>
  )
}

/** confirm — a danger-accented button, optionally gated by a `confirm-word` the
 *  user must type to arm it. The answer is `"true"` once confirmed. */
function ConfirmField({ field, value, onChange, disabled }: FieldProps) {
  const [typed, setTyped] = useState("")
  const confirmed = asScalar(value) === "true"
  if (confirmed) {
    return (
      <span className="inline-flex items-center gap-1.5 rounded-lg bg-(--danger)/10 px-3 py-1.5 text-[12.5px] font-medium text-(--danger)">
        <Check className="size-3.5" strokeWidth={3} /> Confirmed
      </span>
    )
  }
  const armed = !field.confirmWord || typed === field.confirmWord
  return (
    <div className="flex flex-col gap-2">
      {field.confirmWord && (
        <input
          type="text"
          value={typed}
          disabled={disabled}
          onChange={(e) => setTyped(e.target.value)}
          placeholder={`Type ${field.confirmWord} to confirm`}
          className={SCALAR_INPUT}
        />
      )}
      <button
        type="button"
        disabled={disabled || !armed}
        onClick={() => onChange("true")}
        className="inline-flex w-fit items-center gap-1.5 rounded-lg bg-(--danger) px-3.5 py-1.5 text-[12.5px] font-semibold text-white shadow-sm transition-[filter,opacity] hover:brightness-105 disabled:opacity-40 disabled:hover:brightness-100"
      >
        <AlertTriangle className="size-3.5" /> Confirm
      </button>
    </div>
  )
}

/** files — the user answers by uploading; each pick uploads IMMEDIATELY via the
 *  existing `.uploads/` path and the answer accrues the realm-relative paths. */
function FilesField({ value, onChange, disabled, agentId }: FieldProps) {
  const paths = asList(value)
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState<string | null>(null)
  const pick = async (files: File[]) => {
    if (files.length === 0) return
    setBusy(true)
    setErr(null)
    try {
      const added: string[] = []
      for (const f of files) {
        const r = await uploadUnique(agentId, ".uploads", f)
        added.push(r.path)
      }
      onChange([...paths, ...added])
    } catch (e) {
      setErr(e instanceof Error ? e.message : "Upload failed")
    } finally {
      setBusy(false)
    }
  }
  const remove = (p: string) => onChange(paths.filter((x) => x !== p))
  return (
    <div className="flex flex-col gap-1.5">
      {paths.map((p) => (
        <span
          key={p}
          className="inline-flex w-fit items-center gap-1.5 rounded-lg border border-border/70 bg-card px-2.5 py-1 text-[11.5px] text-(--interactive)"
        >
          📎 <span className="font-mono">{p}</span>
          {!disabled && (
            <button
              type="button"
              onClick={() => remove(p)}
              className="text-muted-foreground/60 transition-colors hover:text-(--danger)"
              aria-label={`Remove ${p}`}
            >
              <X className="size-3" />
            </button>
          )}
        </span>
      ))}
      <label
        className={`inline-flex w-fit cursor-pointer items-center gap-1.5 rounded-lg border border-dashed border-border/80 px-3 py-1.5 text-[12.5px] text-muted-foreground/80 transition-colors hover:border-(--signal)/60 hover:text-(--signal) ${disabled ? "pointer-events-none opacity-60" : ""}`}
      >
        {busy ? <Loader2 className="size-3.5 animate-spin" /> : <Upload className="size-3.5" />}
        {busy ? "Uploading…" : "Upload files"}
        <input
          type="file"
          multiple
          className="hidden"
          disabled={disabled || busy}
          onChange={(e) => {
            const files = [...(e.target.files ?? [])]
            e.target.value = ""
            void pick(files)
          }}
        />
      </label>
      {err && <span className="text-[11px] text-(--danger)">{err}</span>}
    </div>
  )
}

/**
 * Dispatch a field to its input renderer by `type`. The `files` uploading state
 * lives inside {@link FilesField}; every other renderer is a controlled input
 * over the shared value map.
 */
export function FieldInput(props: FieldProps) {
  switch (props.field.type) {
    case "single": {
      return <SingleField {...props} />
    }
    case "multi": {
      return <MultiField {...props} />
    }
    case "toggle": {
      return <ToggleField {...props} />
    }
    case "confirm": {
      return <ConfirmField {...props} />
    }
    case "files": {
      return <FilesField {...props} />
    }
    case "date": {
      return <DateField {...props} />
    }
    case "text":
    case "number": {
      return <ScalarField {...props} />
    }
    default: {
      return null
    }
  }
}
