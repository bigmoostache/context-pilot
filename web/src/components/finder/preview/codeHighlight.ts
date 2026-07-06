// Syntax highlighting for live code-file previews (T280).
//
// Uses highlight.js' "common" bundle (≈37 mainstream languages auto-registered:
// rust, ts/js, python, go, c/cpp, java, ruby, bash, sql, json, yaml, css, …)
// plus a curated set of extras the Finder's `code` kind recognises but that the
// common bundle omits (scala, elixir, erlang, haskell, ocaml, dart, fsharp).
// The output HTML is class-tagged (`hljs-keyword`, `hljs-string`, …); those
// classes are themed in index.css against the app's CSS tokens so the colours
// track both palettes. highlight.js escapes the input, so the emitted markup is
// safe to inject.

import hljs from "highlight.js/lib/common"
import scala from "highlight.js/lib/languages/scala"
import elixir from "highlight.js/lib/languages/elixir"
import erlang from "highlight.js/lib/languages/erlang"
import haskell from "highlight.js/lib/languages/haskell"
import ocaml from "highlight.js/lib/languages/ocaml"
import dart from "highlight.js/lib/languages/dart"
import fsharp from "highlight.js/lib/languages/fsharp"

// Register the extras once at module load (common langs are pre-registered).
const EXTRAS: Record<string, Parameters<typeof hljs.registerLanguage>[1]> = {
  scala,
  elixir,
  erlang,
  haskell,
  ocaml,
  dart,
  fsharp,
}
for (const [name, lang] of Object.entries(EXTRAS)) {
  if (!hljs.getLanguage(name)) hljs.registerLanguage(name, lang)
}

/**
 * Map a filename extension to a highlight.js language id. Returns `undefined`
 * for an extension we don't map — the caller then falls back to auto-detection.
 * Keys are lower-case, bare (no dot).
 */
const EXT_LANG: Record<string, string> = {
  rs: "rust",
  ts: "typescript",
  tsx: "typescript",
  js: "javascript",
  jsx: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  py: "python",
  go: "go",
  c: "c",
  h: "c",
  cpp: "cpp",
  cc: "cpp",
  cxx: "cpp",
  hpp: "cpp",
  hxx: "cpp",
  java: "java",
  rb: "ruby",
  sh: "bash",
  bash: "bash",
  zsh: "bash",
  lua: "lua",
  swift: "swift",
  kt: "kotlin",
  kts: "kotlin",
  scala: "scala",
  ex: "elixir",
  exs: "elixir",
  erl: "erlang",
  hs: "haskell",
  ml: "ocaml",
  mli: "ocaml",
  css: "css",
  scss: "scss",
  less: "less",
  html: "xml",
  xml: "xml",
  svg: "xml",
  vue: "xml",
  sql: "sql",
  r: "r",
  pl: "perl",
  pm: "perl",
  php: "php",
  cs: "csharp",
  fs: "fsharp",
  fsx: "fsharp",
  dart: "dart",
  json: "json",
  json5: "json",
  jsonc: "json",
  yaml: "yaml",
  yml: "yaml",
  toml: "ini",
  ini: "ini",
  cfg: "ini",
  conf: "ini",
  md: "markdown",
  mdx: "markdown",
  diff: "diff",
  patch: "diff",
  makefile: "makefile",
  mk: "makefile",
  m: "objectivec",
}

export interface HighlightResult {
  /** Highlighted, class-tagged HTML (safe — highlight.js escapes the source). */
  html: string
  /** The language actually used to render (resolved or auto-detected). */
  language: string
}

/**
 * Highlight `code` for a file named `filename`. Resolves the language from the
 * extension; if unmapped (or the resolved grammar isn't registered) it falls
 * back to highlight.js auto-detection over the registered set. Never throws — a
 * highlight failure degrades to escaped, unstyled text.
 */
export function highlightCode(code: string, filename: string): HighlightResult {
  const ext = filename.includes(".")
    ? filename.slice(filename.lastIndexOf(".") + 1).toLowerCase()
    : filename.toLowerCase() // e.g. "Makefile" has no dot
  const lang = EXT_LANG[ext]

  try {
    if (lang && hljs.getLanguage(lang)) {
      const { value } = hljs.highlight(code, { language: lang, ignoreIllegals: true })
      return { html: value, language: lang }
    }
    const auto = hljs.highlightAuto(code)
    return { html: auto.value, language: auto.language ?? "text" }
  } catch {
    return { html: escapeHtml(code), language: "text" }
  }
}

/** Minimal HTML escape for the degraded (highlight-failed) path. */
function escapeHtml(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;")
}
