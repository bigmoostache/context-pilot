#!/usr/bin/env node
// office-edit — edit a realm .pptx so an open BetterOffice editor merges the
// change in place (no reload). See docs/design-office-live-updates.md.
//
// Usage:
//   node web/scripts/office-edit.mjs --fleet <id> --path <realm-relative.pptx> <edit.mjs>
//   [--api http://127.0.0.1:7878]
//
// <edit.mjs> default-exports `async (doc) => {}`; `doc` is the opened
// @betteroffice/pptx presentation (snapshot(), addTextBox(), insertText()...).
//
// Flow: GET fs/office/log?base=1 → open the shared base bytes with the logged
// state → run the edit → POST fs/office/update with the new full state + the
// saved file. Retries once when another writer moved the log (409).
import { readFileSync } from "node:fs"
import { join, resolve } from "node:path"
import { pathToFileURL } from "node:url"
import { parseArgs } from "node:util"

const { values, positionals } = parseArgs({
  options: {
    api: { type: "string", default: "http://127.0.0.1:7878" },
    fleet: { type: "string" },
    path: { type: "string" },
  },
  allowPositionals: true,
})
const [editFile] = positionals
if (!values.fleet || !values.path || !editFile) {
  console.error("usage: office-edit --fleet <id> --path <file.pptx> <edit.mjs> [--api URL]")
  process.exit(2)
}
if (!values.path.toLowerCase().endsWith(".pptx")) {
  console.error("office-edit: only .pptx is supported for now")
  process.exit(2)
}

const webDir = resolve(import.meta.dirname, "..")
const pkg = join(
  webDir,
  "node_modules/.pnpm/@betteroffice+pptx@0.2.0/node_modules/@betteroffice/pptx",
)
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
