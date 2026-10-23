#!/usr/bin/env node
// office-edit — edit a realm .pptx so an open BetterOffice editor merges the
// change in place (no reload). See docs/design-office-live-updates.md.
//
// Usage:
//   node web/scripts/office-edit.mjs <file.pptx> <edit.mjs> [--fleet <id>] [--api URL]
//   node web/scripts/office-edit.mjs --ops     # the edit API, from the engine's types
//
// <file.pptx> is relative to the cwd (or absolute) and must sit inside an
// agent's folder; the fleet id is found from the registry
// (~/.context-pilot/agents/*.json) unless --fleet is given.
// <edit.mjs> default-exports `async (doc) => {}`; `doc` is the opened
// @betteroffice/pptx presentation (see --ops).
//
// Flow: GET fs/office/log?base=1 → open the shared base bytes with the logged
// state → run the edit → POST fs/office/update with the new full state + the
// saved file. Retries once when another writer moved the log (409).
import { readdirSync, readFileSync } from "node:fs"
import { homedir } from "node:os"
import { join, relative, resolve, sep } from "node:path"
import { pathToFileURL } from "node:url"
import { parseArgs } from "node:util"

const { values, positionals } = parseArgs({
  options: {
    api: { type: "string", default: "http://127.0.0.1:7878" },
    fleet: { type: "string" },
    ops: { type: "boolean", default: false },
  },
  allowPositionals: true,
})

const webDir = resolve(import.meta.dirname, "..")
const pkg = join(
  webDir,
  "node_modules/.pnpm/@betteroffice+pptx@0.2.0/node_modules/@betteroffice/pptx",
)

/** Types an edit script touches, printed by --ops after the handle's methods. */
const OPS_TYPES = [
  "DeckSnapshot",
  "SlideSnapshot",
  "ShapeSnapshot",
  "StorySnapshot",
  "TextStyle",
  "TextStylePatch",
  "ShapeDraft",
  "PresetShapeDraft",
  "PictureDraft",
  "ShapeStroke",
]

/** Handle members left out of --ops: profiling twins, proposals, CRDT plumbing. */
const OPS_HIDDEN =
  /Profiled|[Pp]roposal|clientId|encodeState|encodeDiff|applyUpdate|onUpdate|anchorCaret|resolveCaretAnchor|registerFont|registerFallbackFont|layoutSlide|hitTest|mediaBytes|version\(\)|save\(\)|[Uu]ndo|[Rr]edo|dispose/

/** The `interface|type <name>` block of a .d.ts, brace-matched. */
function typeBlock(dts, name) {
  const start = dts.search(new RegExp(`^(interface|type) ${name}\\b`, "m"))
  if (start < 0) return null
  const lineEnd = dts.indexOf("\n", start)
  if (!dts.slice(start, lineEnd).includes("{")) return dts.slice(start, lineEnd)
  let depth = 0
  for (let i = dts.indexOf("{", start); i < dts.length; i += 1) {
    if (dts[i] === "{") depth += 1
    if (dts[i] === "}") depth -= 1
    if (depth === 0) return dts.slice(start, i + 1)
  }
  return null
}

if (values.ops) {
  const dts = readFileSync(join(pkg, "dist/index.d.ts"), "utf8")
  const lines = (typeBlock(dts, "PresentationHandle") ?? "").split("\n").slice(1, -1)
  // One member = its doc comment + its signature, which may span lines.
  const members = []
  let chunk = []
  let depth = 0
  for (const line of lines) {
    chunk.push(line)
    depth += (line.match(/[{(]/g) ?? []).length - (line.match(/[})]/g) ?? []).length
    if (depth === 0 && line.trimEnd().endsWith(";")) {
      members.push(chunk)
      chunk = []
    }
  }
  const shown = members.filter(
    (m) => !OPS_HIDDEN.test(m.find((l) => !/^\s*(\/\*\*|\*)/.test(l)) ?? ""),
  )
  console.log("// doc: PresentationHandle (@betteroffice/pptx 0.2.0), editing members")
  console.log(`interface PresentationHandle {\n${shown.map((m) => m.join("\n")).join("\n")}\n}`)
  for (const name of OPS_TYPES) {
    const block = typeBlock(dts, name)
    if (block) console.log(`\n${block}`)
  }
  process.exit(0)
}

/** Registry entry whose agent folder contains `file`, as [fleetId, folder]. */
function findAgent(file) {
  const dir = join(homedir(), ".context-pilot/agents")
  const hits = readdirSync(dir)
    .filter((n) => n.endsWith(".json"))
    .map((n) => JSON.parse(readFileSync(join(dir, n), "utf8")))
    .filter((e) => e.folder && (file + sep).startsWith(resolve(e.folder) + sep))
    .sort((a, b) => b.folder.length - a.folder.length)
  return hits[0] ? [hits[0].id, resolve(hits[0].folder)] : null
}

const [fileArg, editFile] = positionals
if (!fileArg || !editFile) {
  console.error("usage: office-edit <file.pptx> <edit.mjs> [--fleet <id>] [--api URL] | --ops")
  process.exit(2)
}
if (!fileArg.toLowerCase().endsWith(".pptx")) {
  console.error("office-edit: only .pptx is supported for now")
  process.exit(2)
}
const absFile = resolve(fileArg)
const agent = findAgent(absFile)
if (!agent) {
  console.error(`office-edit: ${absFile} is not inside any agent's folder`)
  process.exit(2)
}
values.fleet ??= agent[0]
values.path = relative(agent[1], absFile)

const engine = await import(pathToFileURL(join(pkg, "dist/index.js")).href)
await engine.initWasm(readFileSync(join(pkg, "dist/generated/pptx_wasm_bg.wasm")))
const edit = (await import(pathToFileURL(resolve(editFile)).href)).default

const base = `${values.api}/api/fleet/${values.fleet}/fs/office`
const q = `path=${encodeURIComponent(values.path)}`
const b64 = (s) => new Uint8Array(Buffer.from(s, "base64"))

async function attempt() {
  const res = await fetch(`${base}/log?${q}&base=1`)
  if (!res.ok) throw new Error(`GET log: ${res.status} ${await res.text()}`)
  const log = await res.json()
  const opts = log.state ? { initialUpdate: b64(log.state) } : {}
  const doc = engine.openPresentation(b64(log.base), opts)
  await edit(doc)
  const state = doc.encodeStateAsUpdate()
  const file = doc.save()
  const body = new Uint8Array(state.length + file.length)
  body.set(state, 0)
  body.set(file, state.length)
  const post = await fetch(`${base}/update?${q}&seq=${log.seq}&stateLen=${state.length}`, {
    method: "POST",
    body,
  })
  if (post.status === 409) return null
  if (!post.ok) throw new Error(`POST update: ${post.status} ${await post.text()}`)
  return post.json()
}

const done = (await attempt()) ?? (await attempt())
if (!done) {
  console.error("office-edit: the log kept moving (409 twice); retry")
  process.exit(1)
}
console.log(JSON.stringify(done))
