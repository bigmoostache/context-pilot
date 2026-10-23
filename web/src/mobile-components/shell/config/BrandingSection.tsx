import { useRef, useState } from "react"
import { useQuery, useQueryClient } from "@tanstack/react-query"
import {
  type Branding,
  faviconUrl,
  fetchBranding,
  fetchSettings,
  resetFavicon,
  updateBrandingTitle,
  uploadFavicon,
} from "@/lib/api"

const KEY = ["branding"]

/**
 * Admin-only **Branding** section: the browser tab title and favicon, stored by
 * the orchestrator so every browser shows the same. Writes are optimistic and
 * roll back to the previous branding (and the typed title) on failure.
 */
export function BrandingSection() {
  const qc = useQueryClient()
  const { data: settings } = useQuery({ queryKey: ["settings"], queryFn: fetchSettings })
  const { data: branding } = useQuery({ queryKey: KEY, queryFn: fetchBranding })
  const [draft, setDraft] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const fileRef = useRef<HTMLInputElement>(null)

  if (!settings?.is_admin || branding === undefined) return null
  const saved = branding.title ?? ""
  const title = draft ?? saved

  const run = async (optimistic: Partial<Branding>, call: () => Promise<Branding>) => {
    const before = qc.getQueryData<Branding>(KEY)
    if (before) qc.setQueryData<Branding>(KEY, { ...before, ...optimistic })
    setBusy(true)
    try {
      qc.setQueryData<Branding>(KEY, await call())
      return true
    } catch {
      qc.setQueryData(KEY, before)
      return false
    } finally {
      setBusy(false)
    }
  }

  const commitTitle = async () => {
    const next = title.trim()
    if (next === saved) {
      setDraft(null)
      return
    }
    if (await run({ title: next || null }, () => updateBrandingTitle(next))) setDraft(null)
  }

  const onFile = (file: File | undefined) => {
    if (file) void run({}, () => uploadFavicon(file))
    if (fileRef.current) fileRef.current.value = ""
  }

  const version = branding.favicon_version
  return (
    <div className="flex flex-col gap-2 pb-1">
      <div className="flex items-baseline gap-2">
        <span className="text-[10.5px] font-semibold tracking-[0.07em] text-muted-foreground/80 uppercase">
          Branding
        </span>
        <span className="text-[11px] text-muted-foreground/60">Browser tab title and icon</span>
        {busy && (
          <span className="size-3 animate-spin self-center rounded-full border-2 border-muted-foreground/30 border-t-(--signal)" />
        )}
      </div>
      <div className="card-shadow flex flex-col gap-3 rounded-lg bg-card px-3.5 py-3">
        <label className="flex flex-col gap-1">
          <span className="text-[13px] font-medium text-foreground/90">Tab title</span>
          <input
            type="text"
            value={title}
            maxLength={120}
            placeholder="Context·Pilot — Cockpit"
            onChange={(e) => setDraft(e.target.value)}
            onBlur={() => void commitTitle()}
            onKeyDown={(e) => {
              if (e.key === "Enter") e.currentTarget.blur()
            }}
            className="rounded-md border border-input bg-background px-2.5 py-1.5 text-[12.5px]"
          />
        </label>
        <div className="flex items-center gap-3">
          <img
            src={version === null ? "/favicon.svg" : faviconUrl(version)}
            alt="Current favicon"
            className="size-8 rounded-sm object-contain"
          />
          <div className="flex min-w-0 flex-1 flex-col">
            <span className="text-[13px] font-medium text-foreground/90">Favicon</span>
            <span className="text-[11px] text-muted-foreground">
              {version === null ? "Built-in icon" : "Custom image"} · PNG, SVG, ICO… up to 2 MB
            </span>
          </div>
          <input
            ref={fileRef}
            type="file"
            accept="image/*,.ico"
            className="hidden"
            onChange={(e) => onFile(e.target.files?.[0])}
          />
          <button
            type="button"
            disabled={busy}
            onClick={() => fileRef.current?.click()}
            className="rounded-md bg-(--signal) px-2.5 py-1 text-[12px] font-medium text-(--primary-foreground)"
          >
            Upload
          </button>
          {version !== null && (
            <button
              type="button"
              disabled={busy}
              onClick={() => void run({ favicon_version: null }, resetFavicon)}
              className="rounded-md px-2.5 py-1 text-[12px] text-muted-foreground hover:bg-muted/40"
            >
              Reset
            </button>
          )}
        </div>
      </div>
    </div>
  )
}
