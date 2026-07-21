// scaffold-mobile-mirror — regenerate the mobile component tree as an EXACT
// structural mirror of src/components. Idempotent + deterministic: re-running
// produces byte-identical output, so `git diff --exit-code` is the drift check
// (the same contract as the generated OpenAPI bindings).
//
// Every mobile twin is one of two kinds (design docs/design-mobile.md §3.3):
//   • STUB (default) — a `@generated` re-export of the desktop file, for a
//     screen that does not differ on mobile. This is what this script emits.
//   • REAL (hand-authored, marker-LESS) — a genuine mobile implementation. The
//     script NEVER creates, rewrites, or deletes a marker-less file, so real
//     mobile work is safe from being clobbered.
//
// Divergence-closure + ancestor-promotion (§3.3 / §11.1) are computed over the
// import graph, not the folder tree, so a stub can only re-export a desktop
// module whose closure contains nothing divergent. With zero divergence (the
// initial all-stub mirror) every twin is a stub.
//
// Run:  node scripts/scaffold-mobile-mirror.mjs         (write)
//       node scripts/scaffold-mobile-mirror.mjs --check (write + fail on drift)
//
// Runtime is plain Node ESM; the TypeScript compiler API (already a dep) is used
// only to detect each source's export shape — no transpile step, no extra dep.

import { readFileSync, writeFileSync, readdirSync, statSync, mkdirSync, rmSync } from "node:fs"
import { join, relative, dirname, sep } from "node:path"
import { fileURLToPath } from "node:url"
import { execFileSync } from "node:child_process"
import ts from "typescript"

const HERE = dirname(fileURLToPath(import.meta.url))
const WEB_ROOT = join(HERE, "..")
const SRC = join(WEB_ROOT, "src")
const COMPONENTS = join(SRC, "components")
const MOBILE = join(SRC, "mobile-components")

/** Line-1 marker every generated stub carries; the script only touches files
 *  that bear it, protecting hand-authored (divergent) mobile files. */
const MARKER = "// @generated mobile-mirror stub — do not edit; regenerate via pnpm mirror:scaffold"

/** A path is a mirror participant iff it is a component source (.ts/.tsx) that
 *  is not a test, story, or ambient-declaration file (design §11.4). */
function isMirrorSource(name) {
  if (!/\.tsx?$/.test(name)) return false
  if (/\.(test|spec|stories)\./.test(name)) return false
  if (name.endsWith(".d.ts")) return false
  return true
}

/** Recursively collect mirror-source paths under `dir`, relative to it, sorted
 *  for deterministic output. */
function walk(dir) {
  const out = []
  const recur = (abs) => {
    for (const entry of readdirSync(abs).sort()) {
      const full = join(abs, entry)
      if (statSync(full).isDirectory()) recur(full)
      else if (isMirrorSource(entry)) out.push(relative(dir, full))
    }
  }
  recur(dir)
  return out.sort()
}

/** The export shape of a source file: whether it has a default export and/or any
 *  named exports. Drives which re-export lines the stub emits (`export *` carries
 *  named + types but NOT default; `export { default }` carries only default). */
function exportShape(absPath) {
  const text = readFileSync(absPath, "utf8")
  const sf = ts.createSourceFile(absPath, text, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX)
  let hasDefault = false
  let hasNamed = false

  for (const stmt of sf.statements) {
    const mods = ts.canHaveModifiers(stmt) ? ts.getModifiers(stmt) ?? [] : []
    const isExported = mods.some((m) => m.kind === ts.SyntaxKind.ExportKeyword)
    const isDefault = mods.some((m) => m.kind === ts.SyntaxKind.DefaultKeyword)

    // `export default <expr>` (ExportAssignment, not `export =`).
    if (ts.isExportAssignment(stmt) && !stmt.isExportEquals) {
      hasDefault = true
      continue
    }
    // `export default function/class …`
    if (isExported && isDefault) {
      hasDefault = true
      continue
    }
    // `export function/class/const …` (named).
    if (isExported && !isDefault) {
      hasNamed = true
      continue
    }
    // `export { a, b }` / `export * from …` / `export { default as x }`.
    if (ts.isExportDeclaration(stmt)) {
      if (!stmt.exportClause) {
        hasNamed = true // export * from
      } else if (ts.isNamedExports(stmt.exportClause)) {
        for (const el of stmt.exportClause.elements) {
          if (el.name.text === "default") hasDefault = true
          else hasNamed = true
        }
      } else {
        hasNamed = true // namespace export: export * as ns from
      }
    }
  }
  return { hasDefault, hasNamed }
}

/** The `@/components/…` module specifier for a mirror-relative path (POSIX
 *  separators, extension stripped — the alias resolves the rest). */
function desktopSpecifier(relPath) {
  const noExt = relPath.replace(/\.tsx?$/, "").split(sep).join("/")
  return `@/components/${noExt}`
}

/** The stub body for a mirror-relative path: the marker plus the minimal set of
 *  re-export lines its source's export shape requires. A source with no exports
 *  still becomes a module via `export {}`. */
function stubBody(relPath, shape) {
  const spec = desktopSpecifier(relPath)
  const lines = [MARKER]
  if (shape.hasNamed) lines.push(`export * from "${spec}"`)
  if (shape.hasDefault) lines.push(`export { default } from "${spec}"`)
  if (!shape.hasNamed && !shape.hasDefault) lines.push("export {}")
  return lines.join("\n") + "\n"
}

/** True iff the mobile twin at `abs` exists and is hand-authored (marker-less) —
 *  a divergent file the scaffold must never overwrite or delete. */
function isDivergent(abs) {
  try {
    return !readFileSync(abs, "utf8").startsWith(MARKER)
  } catch {
    return false // missing = not divergent
  }
}

/** Collect every existing mobile twin path (relative), so orphaned stubs — whose
 *  desktop source was deleted — can be pruned. */
function existingMobile() {
  try {
    return new Set(walk(MOBILE))
  } catch {
    return new Set()
  }
}

function main() {
  const check = process.argv.includes("--check")
  const desktop = walk(COMPONENTS)
  const desktopSet = new Set(desktop)

  let written = 0
  let skipped = 0
  for (const relPath of desktop) {
    const mobileAbs = join(MOBILE, relPath)
    if (isDivergent(mobileAbs)) {
      // Hand-authored twin — leave it untouched. (Ancestor-promotion for its
      // ancestors is a future concern; with the initial all-stub mirror there
      // are none.)
      skipped++
      continue
    }
    const shape = exportShape(join(COMPONENTS, relPath))
    const body = stubBody(relPath, shape)
    mkdirSync(dirname(mobileAbs), { recursive: true })
    let prev = ""
    try {
      prev = readFileSync(mobileAbs, "utf8")
    } catch {
      /* new file */
    }
    if (prev !== body) {
      writeFileSync(mobileAbs, body)
      written++
    }
  }

  // Orphan cleanup: delete generated (marker-bearing) twins whose desktop source
  // no longer exists. Never delete a divergent (marker-less) file.
  let pruned = 0
  for (const relPath of existingMobile()) {
    if (desktopSet.has(relPath)) continue
    const abs = join(MOBILE, relPath)
    if (isDivergent(abs)) continue
    rmSync(abs)
    pruned++
  }

  console.log(
    `mirror scaffold: ${desktop.length} twins (${written} written, ${skipped} divergent kept, ${pruned} orphans pruned)`,
  )

  if (check) {
    try {
      execFileSync("git", ["diff", "--exit-code", "--", relative(WEB_ROOT, MOBILE)], {
        cwd: WEB_ROOT,
        stdio: "inherit",
      })
    } catch {
      console.error("mirror drift: mobile-components is out of sync — run `pnpm mirror:scaffold` and commit.")
      process.exit(1)
    }
  }
}

main()
