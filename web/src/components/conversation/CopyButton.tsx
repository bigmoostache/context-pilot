import { useState } from "react"
import { Check, Copy } from "lucide-react"
import { cn, clipboard } from "@/lib/utils"

/**
 * Discrete copy-to-clipboard affordance shown beneath a message bubble (and
 * reused by the markdown renderer for its code-block / table "Copy" controls).
 *
 * Sits quietly at low opacity (brightening on hover/focus) so it never competes
 * with the message itself, and on click copies the message's plain text and
 * **transforms into a green check for ~2 s** before reverting — the only
 * feedback the action gives, matching the requested "discrete, click → green
 * tick for a few seconds" behaviour.
 *
 * `align` mirrors the bubble's side so the control tucks under the message's
 * own edge (user bubbles are right-aligned, assistant left-aligned).
 *
 * Lives in its OWN module (not alongside {@link Message}) so the markdown
 * renderer can import it without forming an import cycle: `Message` imports the
 * `Markdown` renderer, and the renderer imports this button — routing the
 * button through a shared leaf keeps that edge acyclic (import-x/no-cycle).
 */
export function CopyButton({
  text,
  getText,
  align,
  label = "Copy",
  className: extra,
}: {
  /** Static text to copy. Ignored when `getText` is provided. */
  text?: string | undefined
  /** Lazy text extraction — called on click, for DOM-derived content. */
  getText?: (() => string) | undefined
  align: "start" | "end"
  /** Button label shown next to the icon (e.g. "Copy code", "Copy table"). */
  label?: string | undefined
  className?: string | undefined
}) {
  const [copied, setCopied] = useState(false)

  const onCopy = () => {
    const t = getText ? getText() : (text ?? "")
    // `clipboard()` returns the API honestly typed as `Clipboard | undefined`
    // (it is genuinely absent on an insecure origin / older browser). The `?.`
    // guard is real — a missing clipboard is a silent no-op, never a throw.
    void clipboard()
      ?.writeText(t)
      .then(
        () => {
          setCopied(true)
          window.setTimeout(() => setCopied(false), 2000)
        },
        () => {
          /* clipboard write rejected (insecure origin / no API) — ignore: the confirmation tick simply won't flash */
        },
      )
  }

  return (
    <button
      type="button"
      onClick={onCopy}
      aria-label={copied ? "Copied" : label}
      className={cn(
        "flex items-center gap-1 rounded-md px-1 py-0.5 text-[10px] transition-colors",
        "opacity-50 hover:opacity-100 focus-visible:opacity-100 outline-none",
        copied
          ? "text-[var(--ok)] opacity-100"
          : cn("text-muted-foreground/70 hover:text-foreground", extra),
        align === "end" ? "self-end" : "self-start",
      )}
    >
      {copied ? <Check className="size-3" /> : <Copy className="size-3" />}
      <span>{copied ? "Copied" : label}</span>
    </button>
  )
}
