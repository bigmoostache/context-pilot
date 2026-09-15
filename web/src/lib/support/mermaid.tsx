import { useEffect, useId, useState, type ReactNode } from "react"

import { cn } from "@/lib/utils"

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
  const [svg, setSvg] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  // `useId` gives a stable, globally-unique id per component instance; strip the
  // framework's `:` delimiters so it is a valid DOM/SVG id (mermaid uses it in
  // selectors internally).
  const rawId = useId()
  const id = `cp-mermaid-${rawId.replaceAll(/[^a-z0-9]/gi, "")}`

  useEffect(() => {
    // Object-held flag (not a bare `let`): the async cleanup mutates it, and a
    // plain boolean would be narrowed to its literal init by the type-checker,
    // making the `alive.current` guards read as "always truthy".
    const alive = { current: true }
    const dark =
      typeof document !== "undefined" && document.documentElement.classList.contains("dark")

    void (async () => {
      try {
        const mod = await import("mermaid")
        const mermaid = mod.default
        mermaid.initialize({
          startOnLoad: false,
          securityLevel: "strict",
          theme: dark ? "dark" : "default",
        })
        const { svg: out } = await mermaid.render(id, code)
        if (alive.current) {
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
  }, [code, id])

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
