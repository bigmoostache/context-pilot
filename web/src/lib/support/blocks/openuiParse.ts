// ── OpenUI Lang — parser (see the ```openui``` block in the Send tool) ─────────
//
// OpenUI Lang is a compact, line-oriented language for LLMs to describe
// interactive UI far more cheaply than JSON (inspired by openui.com). One
// statement per line binds a node id to a component call; a `root = …` line
// names the entrypoint. Containers take a bracketed list of child ids, so the
// whole tree is expressed as a flat list of bindings plus references:
//
//   title = Heading("Quarterly report", level: 2)
//   note  = Text("Revenue up 18% QoQ", muted: true)
//   root  = Card([title, note], title: "Q3")
//
// The grammar is deliberately tiny and fixed, so this hand-rolls a scanner
// rather than pulling a parser dependency (which would churn the hash-locked
// web config + lockfile for zero durable benefit — same reasoning as the
// `form` block parser in components/agents/forms/helpers.ts).

/** A parsed component invocation bound to an id. */
export interface OpenUiNode {
  /** Component name, e.g. `Stack`, `Card`, `Button`. */
  comp: string
  /** Positional arguments: string literals, numbers, or child-id ref lists. */
  args: OpenUiArg[]
  /** Named properties (`key: value`). */
  props: Record<string, OpenUiValue>
}

/** A scalar value a prop or argument can hold. */
export type OpenUiValue = string | number | boolean

/** A positional argument: a scalar, a single ref, or a list of refs. */
export type OpenUiArg =
  | { kind: "value"; value: OpenUiValue }
  | { kind: "ref"; id: string }
  | { kind: "refs"; ids: string[] }
  | { kind: "numbers"; values: number[] }
  | { kind: "strings"; values: string[] }

/** The parse result: a resolved node table plus the root id, or an error. */
export type OpenUiProgram =
  { ok: true; root: string; nodes: Map<string, OpenUiNode> } | { ok: false; error: string }

/** True for an identifier char (binding names and refs). */
const IDENT_RE = /^[a-z_][\w-]*$/i
const isIdent = (s: string): boolean => IDENT_RE.test(s)

/** True for a quoted string literal (`"…"`). */
const isQuoted = (t: string): boolean => t.length >= 2 && t.startsWith('"') && t.endsWith('"')

/**
 * True for a decimal number token. Hand-rolled (sign, digits, at most one dot)
 * rather than a regex: a backtracking `-?\d+(\.\d+)?` trips the unsafe-regex
 * guard, and this scan is both O(n) and total.
 */
function isNumberToken(t: string): boolean {
  const body = t.startsWith("-") ? t.slice(1) : t
  if (body.length === 0) return false
  let dots = 0
  for (const c of body) {
    if (c === ".") {
      dots += 1
      if (dots > 1) return false
    } else if (c < "0" || c > "9") {
      return false
    }
  }
  return true
}

/**
 * Split a comma-separated argument string at top level only — commas inside
 * `"…"` strings or `[…]` lists are preserved. Returns the raw (trimmed) pieces.
 */
function splitArgs(src: string): string[] {
  const out: string[] = []
  let depth = 0
  let inStr = false
  let start = 0
  for (let i = 0; i < src.length; i++) {
    const c = src[i]
    if (inStr) {
      if (c === '"') inStr = false
      continue
    }
    if (c === '"') {
      inStr = true
      continue
    }
    if (c === "[") {
      depth += 1
      continue
    }
    if (c === "]") {
      depth = Math.max(0, depth - 1)
      continue
    }
    // A comma only separates at top level — inside `[…]` it belongs to the list.
    if (c === "," && depth === 0) {
      out.push(src.slice(start, i).trim())
      start = i + 1
    }
  }
  const tail = src.slice(start).trim()
  if (tail.length > 0 || out.length > 0) out.push(tail)
  return out.filter((p) => p.length > 0)
}

/** Parse a scalar token: `"str"`, number, `true`/`false`. Returns undefined if not scalar. */
function scalar(tok: string): OpenUiValue | undefined {
  if (isQuoted(tok)) return tok.slice(1, -1).replaceAll(String.raw`\"`, '"')
  if (tok === "true") return true
  if (tok === "false") return false
  return isNumberToken(tok) ? Number(tok) : undefined
}

/** Parse the contents of a `[ … ]` list into refs, numbers, or strings. */
function parseList(inner: string): OpenUiArg {
  const items = splitArgs(inner)
  if (items.every((it) => isNumberToken(it))) {
    return { kind: "numbers", values: items.map(Number) }
  }
  if (items.every((it) => it.startsWith('"'))) {
    return { kind: "strings", values: items.map((it) => it.slice(1, -1)) }
  }
  return { kind: "refs", ids: items.filter((it) => isIdent(it)) }
}

/** Parse a single argument token into an {@link OpenUiArg}. */
function parseArg(tok: string): OpenUiArg | undefined {
  if (tok.startsWith("[") && tok.endsWith("]")) {
    return parseList(tok.slice(1, -1))
  }
  const sc = scalar(tok)
  if (sc !== undefined) return { kind: "value", value: sc }
  return isIdent(tok) ? { kind: "ref", id: tok } : undefined
}

/** Split a call's argument list into positional args and `key: value` props. */
function parseCallArgs(raw: string): { args: OpenUiArg[]; props: Record<string, OpenUiValue> } {
  const args: OpenUiArg[] = []
  const props: Record<string, OpenUiValue> = {}
  for (const piece of splitArgs(raw)) {
    const colon = piece.indexOf(":")
    // A top-level `key: value` (key is a bare identifier, value a scalar).
    if (colon > 0 && !piece.startsWith('"') && !piece.startsWith("[")) {
      const key = piece.slice(0, colon).trim()
      const val = scalar(piece.slice(colon + 1).trim())
      if (isIdent(key) && val !== undefined) {
        props[key] = val
        continue
      }
    }
    const arg = parseArg(piece)
    if (arg) args.push(arg)
  }
  return { args, props }
}

/** Match `name(…)` and return the component name + inner argument source. */
const CALL_RE = /^([a-z_]\w*)\s*\(([\s\S]*)\)$/i

/** Every child-id reference an argument carries (empty for scalar/list args). */
function argRefs(arg: OpenUiArg): string[] {
  if (arg.kind === "ref") return [arg.id]
  if (arg.kind === "refs") return arg.ids
  return []
}

/**
 * Parse one source line into `nodes`. Returns an error string, or undefined on
 * success (blank lines and `#` comments are skipped).
 */
function parseLine(line: string, nodes: Map<string, OpenUiNode>): string | undefined {
  if (line.length === 0 || line.startsWith("#")) return undefined
  const eq = line.indexOf("=")
  if (eq === -1) return `expected 'id = Component(…)': ${line}`
  const id = line.slice(0, eq).trim()
  if (!isIdent(id)) return `invalid id '${id}'`
  const call = CALL_RE.exec(line.slice(eq + 1).trim())
  if (!call) return `invalid component call for '${id}'`
  const { args, props } = parseCallArgs(call[2] ?? "")
  nodes.set(id, { comp: call[1] ?? "", args, props })
  return undefined
}

/**
 * Parse an OpenUI Lang source string into a node table + root id.
 *
 * Each non-empty, non-comment line must read `id = Component(args)`. Parsing is
 * total: any malformed line, a missing `root`, or an unresolved child ref
 * yields `{ ok: false, error }` so the renderer can degrade to the raw source.
 */
export function parseOpenUi(source: string): OpenUiProgram {
  const nodes = new Map<string, OpenUiNode>()
  for (const rawLine of source.split("\n")) {
    const error = parseLine(rawLine.trim(), nodes)
    if (error !== undefined) return { ok: false, error }
  }
  if (!nodes.has("root")) return { ok: false, error: "missing 'root = …' entry" }
  // Every ref must resolve — fail loudly rather than render a hole.
  for (const [id, node] of nodes) {
    for (const arg of node.args) {
      for (const ref of argRefs(arg)) {
        if (!nodes.has(ref)) return { ok: false, error: `'${id}' references unknown '${ref}'` }
      }
    }
  }
  return { ok: true, root: "root", nodes }
}
