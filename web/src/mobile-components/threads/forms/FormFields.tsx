// ── Form field input renderers (one per v1 type, docs/forms.md §3) ──────
//
// The mobile twin of `components/threads/forms/FormFields`. Each renderer is a
// controlled input over a single field's answer value: scalar (single/text/
// number/date/toggle/confirm) or string list (multi/files). They are
// presentational — the owning FormWidget holds the value map, the draft
// persistence, and the submit gate. `files` is the sole async one: it uploads
// on pick via the existing `.uploads/` path and answers with paths.
//
// Divergence from desktop is touch-only: every text control uses a **16px
// font** (below 16px iOS Safari auto-zooms the viewport on focus), and the
// shadcn Calendar/Popover resolve through the `@/mobile-components/ui` token so
// they can adopt a bottom-sheet presentation once `ui/` is recoded. All field
// logic (option toggling, ISO date storage, confirm-word arming, immediate
// upload) is byte-identical to the desktop twin.

import { useState } from "react"
import { Check, Upload, Loader2, AlertTriangle, X, CalendarIcon } from "lucide-react"
import { format, parse } from "date-fns"
import { uploadUnique } from "@/lib/api"
import { Calendar } from "@/mobile-components/ui/calendar"
import { Popover, PopoverContent, PopoverTrigger } from "@/mobile-components/ui/popover"
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

/** A grouped-list container for option rows — the iOS "grouped inset list"
 *  idiom: one rounded card with hairline row dividers, so a set of choices reads
 *  as a single discrete block instead of a stack of individually-bordered pills
 *  (the "more discrete boxing" the mobile form was asked for). */
function OptionGroup({ children }: { children: React.ReactNode }) {
  return (
    <div className="divide-y divide-border/50 overflow-hidden rounded-xl border border-border/60 bg-card">
      {children}
    </div>
  )
}

/** One selectable option row (radio/checkbox) inside an {@link OptionGroup}. The
 *  row is borderless (the group card supplies the boxing) and the whole row is
 *  the tap target; a custom glyph carries the checked state and the selected row
 *  gets a faint signal tint — the native iOS list-row selection look. `py-3`
 *  keeps a comfortable ≥44px touch target. */
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
      className={`group flex cursor-pointer items-start gap-3 px-3.5 py-3 text-[14px] transition-colors ${
        on ? "bg-(--signal)/8" : "active:bg-muted/40"
      } ${disabled ? "pointer-events-none opacity-60" : ""}`}
    >
      <input type={kind} checked={on} disabled={disabled} onChange={onPick} className="sr-only" />
      <span
        className={`mt-0.5 flex size-[18px] shrink-0 items-center justify-center border transition-colors ${
          kind === "radio" ? "rounded-full" : "rounded-md"
        } ${on ? "border-(--signal) bg-(--signal) text-(--primary-foreground)" : "border-border/80 bg-background"}`}
      >
        {on &&
          (kind === "radio" ? (
            <span className="size-2 rounded-full bg-current" />
          ) : (
            <Check className="size-3.5" strokeWidth={3} />
          ))}
      </span>
      <span className="min-w-0">
        <span className="block font-medium text-foreground/90">{label}</span>
        {detail && <span className="block text-[12px] text-muted-foreground/70">{detail}</span>}
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
    <OptionGroup>
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
          className={`group flex cursor-pointer items-start gap-3 px-3.5 py-3 text-[14px] transition-colors ${
            isOther ? "bg-(--signal)/8" : "active:bg-muted/40"
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
            className={`mt-0.5 flex size-[18px] shrink-0 items-center justify-center rounded-full border transition-colors ${isOther ? "border-(--signal) bg-(--signal) text-(--primary-foreground)" : "border-border/80 bg-background"}`}
          >
            {isOther && <span className="size-2 rounded-full bg-current" />}
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
                className="mt-2 w-full rounded-lg border border-transparent bg-muted/50 px-3 py-2.5 text-[16px] transition-colors outline-none focus:border-(--signal)/60 focus:bg-background"
              />
            )}
          </span>
        </label>
      )}
    </OptionGroup>
  )
}

/** multi — checkbox group; the answer is the list of chosen labels. */
function MultiField({ field, value, onChange, disabled }: FieldProps) {
  const sel = asList(value)
  const toggle = (label: string) => {
    onChange(sel.includes(label) ? sel.filter((l) => l !== label) : [...sel, label])
  }
  return (
    <OptionGroup>
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
    </OptionGroup>
  )
}

const SCALAR_INPUT =
  "w-full rounded-xl border border-transparent bg-muted/50 px-3.5 py-3 text-[16px] text-foreground/90 outline-none transition-colors focus:border-(--signal)/60 focus:bg-background disabled:opacity-60"

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
        className={`flex w-full items-center gap-2 rounded-xl border border-transparent bg-muted/50 px-3.5 py-3 text-left text-[16px] transition-colors outline-none focus:border-(--signal)/60 focus:bg-background disabled:opacity-60 ${
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
      <span className="text-[11.5px] font-medium text-muted-foreground/70">
        {on ? "On" : "Off"}
      </span>
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
      <span className="inline-flex items-center gap-1.5 rounded-lg bg-(--danger)/10 px-3 py-2 text-[13px] font-medium text-(--danger)">
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
        className="inline-flex w-fit items-center gap-1.5 rounded-lg bg-(--danger) px-3.5 py-2 text-[13px] font-semibold text-white shadow-sm transition-[filter,opacity] active:brightness-105 disabled:opacity-40 disabled:active:brightness-100"
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
          className="inline-flex w-fit items-center gap-1.5 rounded-lg border border-border/70 bg-card px-2.5 py-1.5 text-[12px] text-(--interactive)"
        >
          📎 <span className="font-mono">{p}</span>
          {!disabled && (
            <button
              type="button"
              onClick={() => remove(p)}
              className="text-muted-foreground/60 transition-colors active:text-(--danger)"
              aria-label={`Remove ${p}`}
            >
              <X className="size-3" />
            </button>
          )}
        </span>
      ))}
      <label
        className={`inline-flex w-fit cursor-pointer items-center gap-1.5 rounded-lg border border-dashed border-border/80 px-3 py-2 text-[13px] text-muted-foreground/80 transition-colors active:border-(--signal)/60 active:text-(--signal) ${disabled ? "pointer-events-none opacity-60" : ""}`}
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
