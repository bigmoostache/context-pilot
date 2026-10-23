import { useEffect } from "react"
import { useQuery } from "@tanstack/react-query"
import { faviconUrl, fetchBranding } from "@/lib/api"

/** index.html's tab title and icon hrefs, captured before any override. */
const DEFAULT_TITLE = document.title
const ICON_LINKS = [...document.querySelectorAll<HTMLLinkElement>('link[rel="icon"]')]
const DEFAULT_ICONS = ICON_LINKS.map((link) => ({ href: link.href, type: link.type }))

/**
 * Apply the orchestrator-wide branding (settings → General) to the browser tab:
 * the custom title and the uploaded favicon, or index.html's defaults when unset.
 * Shared by both device trees, hence mounted above the desktop/mobile switch.
 */
export function useBranding(): void {
  const { data } = useQuery({ queryKey: ["branding"], queryFn: fetchBranding })
  const title = data?.title ?? null
  const version = data?.favicon_version ?? null

  useEffect(() => {
    document.title = title ?? DEFAULT_TITLE
  }, [title])

  useEffect(() => {
    for (const [i, link] of ICON_LINKS.entries()) {
      const fallback = DEFAULT_ICONS[i]
      // The uploaded image's type is sniffed server-side; drop the link's
      // hard-coded type so the browser doesn't skip a PNG labelled SVG.
      link.href = version === null ? (fallback?.href ?? "") : faviconUrl(version)
      link.type = version === null ? (fallback?.type ?? "") : ""
    }
  }, [version])
}
