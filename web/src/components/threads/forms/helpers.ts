// ── Inline `form` blocks — parsing + answer helpers (see docs/forms.md) ──
//
// Forms ride the UNCHANGED message API as opaque markdown: the agent writes a
// ` ```form ` fenced block (YAML), the frontend parses it here and renders an
// interactive widget (mirrors the `file-upload` card mechanism). When the user
// submits, the frontend composes a ` ```form-answer ` block and sends it as an
// ordinary user message; the answered/locked state is DERIVED by scanning the
// thread for a matching `form-answer` (by `form-id`) — no backend form state.
//
// The YAML grammar is small and fixed (docs/forms.md §2/§5), so this parses it
// with a dedicated indent scanner rather than pulling a YAML dependency (which
// would churn the hash-locked config + lockfile for zero durable benefit).

/** The v1 field types (docs/forms.md §3). */
export type FieldType =
  "single" | "multi" | "text" | "number" | "date" | "toggle" | "confirm" | "files"

/** One `{label, detail}` option of a `single` / `multi` field. */
export interface FormOption {
  label: string
  detail: string
}

/** One field of a form. `options` present only for `single` / `multi`. */
export interface FormField {
  id: string
  label: string
  type: FieldType
  /** `single` only — adds a free-text "Other…" choice. */
  allowOther?: boolean
  /** `confirm` only — the word the user must type to arm the danger button. */
  confirmWord?: string
  options?: FormOption[]
}

/** A parsed ` ```form ` block. */
export interface FormSpec {
  formId: string
  title?: string
  submit?: string
  fields: FormField[]
}

/** A single answer: scalar for single/text/number/date/toggle/confirm, list for
 *  multi/files. */
export type AnswerValue = string | string[]

/** One `{id, answer}` entry of a `form-answer` block. */
export interface FormAnswerEntry {
  id: string
  answer: AnswerValue
}

/** A parsed ` ```form-answer ` block. */
export interface FormAnswer {
  formId: string
  answers: FormAnswerEntry[]
}

const FORM_RE = /```form\n([\s\S]*?)```/g
const FORM_ANSWER_RE = /```form-answer\n([\s\S]*?)```/

/** Whether a message body carries a ` ```form ` or ` ```form-answer ` block —
 *  i.e. it must render through the form row (fresh every render) rather than the
 *  memoized plain message row. Pure, so it lives here (not in the component
 *  file) to keep that file's exports component-only for Fast Refresh. */
export function isFormMessage(text: string): boolean {
  return text.includes("```form\n") || text.includes("```form-answer\n")
}

/** Leading-space count of a raw line. */
function indentOf(raw: string): number {
  return raw.length - raw.trimStart().length
}

/** Strip a single pair of matching surrounding quotes from a scalar. */
function unquote(s: string): string {
  const t = s.trim()
  if (
    t.length >= 2 &&
    ((t.startsWith('"') && t.endsWith('"')) || (t.startsWith("'") && t.endsWith("'")))
  ) {
    return t.slice(1, -1)
  }
  return t
}

/** Split a `key: value` line into its key and (possibly empty) value. */
function splitKv(line: string): { key: string; val: string } {
  const idx = line.indexOf(":")
  if (idx === -1) return { key: line.trim(), val: "" }
  return { key: line.slice(0, idx).trim(), val: line.slice(idx + 1).trim() }
}

/** Parse a YAML flow sequence `[a, b, c]` into trimmed, unquoted items. An empty
 *  `[]` yields `[]`. A non-bracketed value yields `null` (caller treats scalar). */
function parseFlowList(val: string): string[] | null {
  const t = val.trim()
  if (!t.startsWith("[") || !t.endsWith("]")) return null
  const inner = t.slice(1, -1).trim()
  if (inner === "") return []
  return inner.split(",").map((s) => unquote(s))
}

/** Apply a `key: value` pair onto the field currently being built. */
function applyFieldKv(field: FormField, key: string, val: string): void {
  switch (key) {
    case "id": {
      field.id = unquote(val)
      break
    }
    case "label": {
      field.label = unquote(val)
      break
    }
    case "type": {
      field.type = unquote(val) as FieldType
      break
    }
    case "allow-other": {
      field.allowOther = unquote(val) === "true"
      break
    }
    case "confirm-word": {
      field.confirmWord = unquote(val)
      break
    }
    // No default
  }
}

/** Mutable cursor threaded through the {@link parseFormBody} line handlers:
 *  whether we are inside the `fields:` list, plus the field/option currently
 *  being populated. */
interface FieldCtx {
  inFields: boolean
  curField: FormField | null
  curOpt: FormOption | null
}

/** Apply an indent-0 top-level line: `fields:` opens the list, otherwise a
 *  `form-id`/`title`/`submit` scalar. Resets the field/option cursor. */
function topLevelLine(spec: FormSpec, ctx: FieldCtx, line: string): void {
  ctx.inFields = line.startsWith("fields:")
  ctx.curField = null
  ctx.curOpt = null
  if (ctx.inFields) return
  const { key, val } = splitKv(line)
  switch (key) {
    case "form-id": {
      spec.formId = unquote(val)
      break
    }
    case "title": {
      spec.title = unquote(val)
      break
    }
    case "submit": {
      spec.submit = unquote(val)
      break
    }
    // No default
  }
}

/** Apply a `- ` dash line: a new field (indent ≤ 4) or a new option of the
 *  current field (indent ≥ 6). An inline `key: value` after the dash seeds it. */
function dashLine(spec: FormSpec, ctx: FieldCtx, indent: number, line: string): void {
  const rest = line.slice(2).trim()
  const kv = rest ? splitKv(rest) : null
  if (indent <= 4) {
    ctx.curField = { id: "", label: "", type: "text" }
    spec.fields.push(ctx.curField)
    ctx.curOpt = null
    if (kv) applyFieldKv(ctx.curField, kv.key, kv.val)
  } else if (ctx.curField) {
    ctx.curField.options ??= []
    ctx.curOpt = { label: "", detail: "" }
    ctx.curField.options.push(ctx.curOpt)
    if (kv) applyOptionKv(ctx.curOpt, kv.key, kv.val)
  }
}

/** Apply a `label`/`detail` pair onto the option currently being built. */
function applyOptionKv(opt: FormOption, key: string, val: string): void {
  if (key === "label") opt.label = unquote(val)
  else if (key === "detail") opt.detail = unquote(val)
}

/** Apply a plain `key: value` line: `options:` opens a field's option list, an
 *  indented pair fills the current option, else it fills the current field. */
function kvLine(ctx: FieldCtx, indent: number, line: string): void {
  const { key, val } = splitKv(line)
  if (key === "options" && ctx.curField) {
    ctx.curField.options = []
    ctx.curOpt = null
  } else if (ctx.curOpt && indent >= 6) {
    applyOptionKv(ctx.curOpt, key, val)
  } else if (ctx.curField) {
    applyFieldKv(ctx.curField, key, val)
  }
}

/**
 * Parse one ` ```form ` block body into a {@link FormSpec}, or `null` when it
 * lacks the two structural invariants (a `form-id` and a non-empty `fields`
 * list) — the same guard the backend applies at send time (docs/forms.md §7).
 *
 * Grammar (fixed, docs/forms.md §2): top-level `form-id`/`title`/`submit` keys
 * at indent 0, a `fields:` list whose items sit at indent 2 (`- id: …`) with
 * their keys at indent 4, and an optional `options:` list per field whose items
 * sit at indent 6 (`- label: …`) with keys at indent 8.
 */
function parseFormBody(body: string): FormSpec | null {
  const spec: FormSpec = { formId: "", fields: [] }
  const ctx: FieldCtx = { inFields: false, curField: null, curOpt: null }

  for (const raw of body.split("\n")) {
    if (raw.trim() === "") continue
    const indent = indentOf(raw)
    const line = raw.trim()
    if (indent === 0) {
      topLevelLine(spec, ctx, line)
      continue
    }
    if (!ctx.inFields) continue
    if (line.startsWith("- ")) dashLine(spec, ctx, indent, line)
    else kvLine(ctx, indent, line)
  }

  if (!spec.formId || spec.fields.length === 0) return null
  return spec
}

/** Parse every ` ```form ` block in a message body into {@link FormSpec}s (a
 *  malformed block is skipped, matching the send-time guard). */
export function parseFormBlocks(text: string): FormSpec[] {
  const out: FormSpec[] = []
  FORM_RE.lastIndex = 0
  let m: RegExpExecArray | null
  while ((m = FORM_RE.exec(text)) !== null) {
    const spec = parseFormBody(m[1] ?? "")
    if (spec) out.push(spec)
  }
  return out
}

/** Remove every ` ```form ` block from a message body so the prose can render
 *  without the raw YAML (the widget is drawn separately). */
export function stripFormBlocks(text: string): string {
  return text.replaceAll(FORM_RE, "").trim()
}

/**
 * Parse the FIRST ` ```form-answer ` block in a message body into a
 * {@link FormAnswer}, or `null` when absent/malformed. The `answers` list holds
 * one `{id, answer}` per field; a flow-list value (`[a, b]`) becomes a string
 * array (multi/files), a bare scalar stays a string.
 */
/** Mutable cursor threaded through the {@link parseFormAnswerBlock} handlers. */
interface AnswerCtx {
  formId: string
  answers: FormAnswerEntry[]
  cur: FormAnswerEntry | null
  inAnswers: boolean
}

/** Apply an `id`/`answer` pair onto the answer entry being built (a flow list
 *  `[a, b]` becomes an array, a bare scalar stays a string). */
function applyAnswerKv(cur: FormAnswerEntry, key: string, val: string): void {
  if (key === "id") cur.id = unquote(val)
  else if (key === "answer") cur.answer = parseFlowList(val) ?? unquote(val)
}

export function parseFormAnswerBlock(text: string): FormAnswer | null {
  const m = FORM_ANSWER_RE.exec(text)
  if (!m) return null
  const ctx: AnswerCtx = { formId: "", answers: [], cur: null, inAnswers: false }

  const bodyLines = (m[1] ?? "").split("\n")
  for (const raw of bodyLines) {
    if (raw.trim() === "") continue
    const indent = indentOf(raw)
    const line = raw.trim()
    if (indent === 0) {
      ctx.inAnswers = false
      ctx.cur = null
      const { key, val } = splitKv(line)
      if (key === "form-id") ctx.formId = unquote(val)
      else if (key === "answers") ctx.inAnswers = true
    } else if (ctx.inAnswers && line.startsWith("- ")) {
      ctx.cur = { id: "", answer: "" }
      ctx.answers.push(ctx.cur)
      const rest = line.slice(2).trim()
      if (rest) {
        const { key, val } = splitKv(rest)
        applyAnswerKv(ctx.cur, key, val)
      }
    } else if (ctx.inAnswers && ctx.cur) {
      const { key, val } = splitKv(line)
      applyAnswerKv(ctx.cur, key, val)
    }
  }

  if (!ctx.formId) return null
  return { formId: ctx.formId, answers: ctx.answers }
}

/** Whether a scalar needs YAML double-quoting (contains a colon-space, leading
 *  special char, or is empty). Kept conservative — quote when unsure. */
function needsQuote(s: string): boolean {
  return s === "" || /[:#]|^[-?[{&*!|>%@`"']/.test(s) || s !== s.trim()
}

/** Serialize one scalar for the `form-answer` YAML body. */
function scalar(s: string): string {
  return needsQuote(s) ? `"${s.replaceAll('"', String.raw`\"`)}"` : s
}

/**
 * Compose the ` ```form-answer ` message body the frontend sends when the user
 * submits a form (docs/forms.md §5). A scalar answer rides bare; a list answer
 * (`multi` / `files`) rides as a YAML flow list `[a, b]`.
 */
export function buildFormAnswerContent(formId: string, answers: FormAnswerEntry[]): string {
  const lines = ["```form-answer", `form-id: ${scalar(formId)}`, "answers:"]
  for (const a of answers) {
    lines.push(`  - id: ${scalar(a.id)}`)
    if (Array.isArray(a.answer)) {
      lines.push(`    answer: [${a.answer.map((v) => scalar(v)).join(", ")}]`)
    } else {
      lines.push(`    answer: ${scalar(a.answer)}`)
    }
  }
  lines.push("```")
  return lines.join("\n")
}
