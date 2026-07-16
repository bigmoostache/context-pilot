// ── Form field input renderers (one per v1 type, docs/forms.md §3) ──────
//
// Each renderer is a controlled input over a single field's answer value:
// scalar (single/text/number/date/toggle/confirm) or string list (multi/files).
// They are presentational — the owning FormWidget holds the value map, the
// draft persistence, and the submit gate. `files` is the sole async one: it
// uploads on pick via the existing `.uploads/` path and answers with paths.

import { useState } from "react"
import { Check, Upload, Loader2, AlertTriangle } from "lucide-react"
import { uploadUnique } from "@/lib/api"
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

const OPTION_ROW =
  "flex cursor-pointer items-start gap-2.5 rounded-lg border px-3 py-2 text-[12.5px] transition-colors"

/** single — radio over `{label, detail}` options, plus an optional free-text
 *  "Other…" choice (`allow-other`). The answer is the chosen label or the typed
 *  string. */
function SingleField({ field, value, onChange, disabled }: FieldProps) {
  const sel = asScalar(value)
  const labels = (field.options ?? []).map((o) => o.label)
  const isOther = field.allowOther === true && sel !== "" && !labels.includes(sel)
  return (
    <div className="flex flex-col gap-1.5">
      {(field.options ?? []).map((o) => {
        const on = sel === o.label
        return (
          <label
            key={o.label}
            className={`${OPTION_ROW} ${on ? "border-(--signal) bg-(--signal)/10" : "border-border hover:bg-muted/40"}`}
          >
            <input
              type="radio"
              checked={on}
              disabled={disabled}
              onChange={() => onChange(o.label)}
              className="mt-0.5 accent-(--signal)"
            />
            <span className="min-w-0">
              <span className="block font-medium text-foreground/90">{o.label}</span>
              {o.detail && <span className="block text-muted-foreground/70">{o.detail}</span>}
            </span>
          </label>
        )
      })}
      {field.allowOther === true && (
        <label
          className={`${OPTION_ROW} ${isOther ? "border-(--signal) bg-(--signal)/10" : "border-border hover:bg-muted/40"}`}
        >
          <input
            type="radio"
            checked={isOther}
            disabled={disabled}
            onChange={() => onChange(" ")}
            className="mt-0.5 accent-(--signal)"
          />
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
                className="mt-1 w-full rounded-md border border-border bg-background px-2 py-1 text-[12.5px] outline-none focus:border-(--signal)"
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
    <div className="flex flex-col gap-1.5">
      {(field.options ?? []).map((o) => {
        const on = sel.includes(o.label)
        return (
          <label
            key={o.label}
            className={`${OPTION_ROW} ${on ? "border-(--signal) bg-(--signal)/10" : "border-border hover:bg-muted/40"}`}
          >
            <input
              type="checkbox"
              checked={on}
              disabled={disabled}
              onChange={() => toggle(o.label)}
              className="mt-0.5 accent-(--signal)"
            />
            <span className="min-w-0">
              <span className="block font-medium text-foreground/90">{o.label}</span>
              {o.detail && <span className="block text-muted-foreground/70">{o.detail}</span>}
            </span>
          </label>
        )
      })}
    </div>
  )
}

const SCALAR_INPUT =
  "w-full rounded-lg border border-border bg-background px-3 py-2 text-[12.5px] text-foreground/90 outline-none focus:border-(--signal) disabled:opacity-60"

/** text / number / date — a single controlled input keyed off the field type. */
function ScalarField({ field, value, onChange, disabled }: FieldProps) {
  const type = field.type === "number" ? "number" : field.type === "date" ? "date" : "text"
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

/** toggle — a neutral on/off switch; the answer is `"true"` / `"false"`. */
function ToggleField({ value, onChange, disabled }: FieldProps) {
  const on = asScalar(value) === "true"
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      disabled={disabled}
      onClick={() => onChange(on ? "false" : "true")}
      className={`relative inline-flex h-6 w-11 items-center rounded-full transition-colors ${on ? "bg-(--signal)" : "bg-muted"} disabled:opacity-60`}
    >
      <span
        className={`inline-block size-5 transform rounded-full bg-white transition-transform ${on ? "translate-x-5" : "translate-x-0.5"}`}
      />
    </button>
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
        <Check className="size-3.5" /> Confirmed
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
        className="inline-flex w-fit items-center gap-1.5 rounded-lg bg-(--danger) px-3 py-1.5 text-[12.5px] font-medium text-white transition-[filter] hover:brightness-105 disabled:opacity-40"
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
  return (
    <div className="flex flex-col gap-1.5">
      {paths.map((p) => (
        <span
          key={p}
          className="inline-flex w-fit items-center gap-1.5 rounded-md border border-border bg-card px-2 py-1 text-[11.5px] text-(--interactive)"
        >
          📎 {p}
        </span>
      ))}
      <label
        className={`inline-flex w-fit cursor-pointer items-center gap-1.5 rounded-lg border border-dashed border-border px-3 py-1.5 text-[12.5px] text-muted-foreground/80 transition-colors hover:border-(--signal)/60 hover:text-(--signal) ${disabled ? "pointer-events-none opacity-60" : ""}`}
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
    case "text":
    case "number":
    case "date": {
      return <ScalarField {...props} />
    }
    default: {
      return null
    }
  }
}
