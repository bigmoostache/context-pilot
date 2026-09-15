import { useEffect, useId, useState, type ReactNode } from "react"

import { cn } from "@/lib/utils"

/** The mermaid API surface (the dynamic import's default export). */
type MermaidApi = Awaited<typeof import("mermaid")>["default"]

/**
 * Memoised import promise + the theme mermaid was last initialised for.
 *
 * mermaid config is GLOBAL to the library, so `initialize` must run once per
 * theme — not once per component mount (the old code re-initialised on every
 * diagram, pure waste). Held on a const object (fields mutated, binding never
 * reassigned) to keep module state without a reassigned top-level `let`.
 */
const memo: { mod: Promise<MermaidApi> | null; theme: "dark" | "light" | null } = {
  mod: null,
  theme: null,
}

/**
 * Rendered-SVG cache keyed by `"<theme>:<source>"`. Identical diagrams (repeats)
 * and re-mounts (scrolling a message row in/out) reuse the SVG verbatim instead
 * of re-running the expensive dagre/d3 layout + serialize on the main thread.
 */
const svgCache = new Map<string, string>()

/**
 * Import mermaid once (memoised promise), (re)initialise only when the theme
 * changed since the last call, and return the API. The first diagram on the
 * page pays the chunk load; every diagram after reuses this.
 */
async function loadMermaid(dark: boolean): Promise<MermaidApi> {
  memo.mod ??= import("mermaid").then((m) => m.default)
  const mermaid = await memo.mod
  const theme = dark ? "dark" : "light"
  if (memo.theme !== theme) {
    mermaid.initialize({ startOnLoad: false, securityLevel: "strict", theme: dark ? "dark" : "default" })
    memo.theme = theme
  }
  return mermaid
}

/**
 * Render a ```mermaid``` fenced block as an actual diagram.
 *
 * `mermaid` is a heavy dependency (it pulls in d3, dagre, cytoscape), so it is
 * **lazy-loaded** via dynamic `import()` on first mount — the initial bundle
 * never pays for it, only threads that actually contain a mermaid diagram do.
 *
 * Rendering is async: `mermaid.render(id, code)` returns an SVG string that we
 * inject as inner HTML (the same pattern as the Finder's highlight.js preview).
 * `securityLevel: "strict"` makes mermaid sanitise the output (DOMPurify
 * internally) and forbid inline scripts/click handlers, so the injected markup
 * is safe by construction. The theme tracks the app's light/dark class so the
 * diagram reads on either background. A parse/render failure degrades to the
 * raw code in a muted box rather than throwing.
 *
 * `onAccent` is true inside the coloured user bubble — the error fallback then
 * uses a translucent-dark box instead of the muted surface so it stays legible.
 */
export function Mermaid({ code, onAccent }: { code: string; onAccent: boolean }): ReactNode {
  const dark =
    typeof document !== "undefined" && document.documentElement.classList.contains("dark")
  const cacheKey = `${dark ? "dark" : "light"}:${code}`
  // Seed from the cache during render (not via setState in the effect): a repeat
  // diagram or a re-mount paints its SVG on the FIRST frame with no flash and no
  // cascading effect render. A miss seeds `null` → the effect renders it async.
  const [svg, setSvg] = useState<string | null>(() => svgCache.get(cacheKey) ?? null)
  const [error, setError] = useState<string | null>(null)
  // `useId` gives a stable, globally-unique id per component instance; strip the
  // framework's `:` delimiters so it is a valid DOM/SVG id (mermaid uses it in
  // selectors internally).
  const rawId = useId()
  const id = `cp-mermaid-${rawId.replaceAll(/[^a-z0-9]/gi, "")}`

  useEffect(() => {
    // Already rendered (seeded from cache at mount, or filled on a prior pass) —
    // nothing to do. Layout + serialize only ever runs on a genuine miss.
    if (svgCache.has(cacheKey)) {
      return
    }
    // Object-held flag (not a bare `let`): the async cleanup mutates it, and a
    // plain boolean would be narrowed to its literal init by the type-checker,
    // making the `alive.current` guards read as "always truthy".
    const alive = { current: true }
    void (async () => {
      try {
        const mermaid = await loadMermaid(dark)
        const { svg: out } = await mermaid.render(id, code)
        if (alive.current) {
          svgCache.set(cacheKey, out)
          setSvg(out)
          setError(null)
        }
      } catch (err) {
        if (alive.current) {
          setError(err instanceof Error ? err.message : "Mermaid render failed")
          setSvg(null)
        }
      }
    })()

    return () => {
      alive.current = false
    }
  }, [cacheKey, dark, id, code])

  if (error !== null) {
    return (
      <pre
        className={cn(
          "my-2 overflow-x-auto rounded-lg p-3 font-mono text-[12px] leading-relaxed",
          onAccent ? "bg-black/20" : "border border-border bg-muted/60 text-foreground/90",
        )}
      >
        {code}
      </pre>
    )
  }

  if (svg === null) {
    return (
      <div className="my-2 flex justify-center">
        <span className="text-[12px] text-muted-foreground/60">Rendering diagram…</span>
      </div>
    )
  }

  return (
    <div
      className="my-2 flex justify-center overflow-x-auto [&>svg]:h-auto [&>svg]:max-w-full"
      // mermaid renders with securityLevel:"strict" (DOMPurify-sanitised SVG,
      // no inline scripts/handlers), so the markup is safe to inject.
      dangerouslySetInnerHTML={{ __html: svg }}
    />
  )
}
