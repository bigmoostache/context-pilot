import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import { createUniver, LocaleType, mergeLocales } from "@univerjs/presets"
import { UniverSheetsCorePreset } from "@univerjs/preset-sheets-core"
import UniverPresetSheetsCoreEnUS from "@univerjs/preset-sheets-core/locales/en-US"
import UniverPresetSheetsCoreFrFR from "@univerjs/preset-sheets-core/locales/fr-FR"
import "@univerjs/preset-sheets-core/lib/index.css"
import { Save } from "lucide-react"
import { cn } from "@/lib/utils"
import { saveToRealm, toWorkbookData, type SheetPayload } from "./sheetState"

/**
 * Univer spreadsheet viewer/editor — near-Excel UX in Finder preview.
 *
 * Full spreadsheet engine: range selection, formatting, formulas, sheet
 * tabs, merge cells, undo/redo, context menus, auto-fill, find & replace.
 * Autosave snapshots to localStorage (crash recovery). Explicit Save
 * button uploads the modified XLSX back to the agent realm — the realm
 * file is the single source of truth for downloads.
 */
export function SheetGrid({
  sheets,
  path,
  agentId,
}: {
  sheets: SheetPayload[]
  path: string
  /** Agent realm to save back to. Without this, Save is disabled. */
  agentId?: string | undefined
}) {
  const containerRef = useRef<HTMLDivElement>(null)
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const univerRef = useRef<any>(null)
  const [ready, setReady] = useState(false)
  const [dirty, setDirty] = useState(false)
  const [saveStatus, setSaveStatus] = useState<"idle" | "saving" | "saved" | "error">("idle")
  const [saveError, setSaveError] = useState<string | null>(null)

  const workbookData = useMemo(() => toWorkbookData(sheets), [sheets])
  const storageKey = `univer-sheet:${path}`

  const handleSave = useCallback(async () => {
    if (!univerRef.current || !agentId) return
    setSaveStatus("saving")
    setSaveError(null)
    try {
      await saveToRealm(univerRef.current, agentId, path)
      // Remote file is now the source of truth — wipe localStorage so next
      // mount loads from the freshly-saved remote, not a stale snapshot.
      localStorage.removeItem(storageKey)
      setDirty(false)
      setSaveStatus("saved")
      setTimeout(() => setSaveStatus("idle"), 1500)
    } catch (error) {
      setSaveStatus("error")
      setSaveError(error instanceof Error ? error.message : "Save failed")
    }
  }, [agentId, path])

  useEffect(() => {
    const el = containerRef.current
    if (!el) return

    // Prefer localStorage snapshot over calamine data (survives reloads).
    let data = workbookData
    try {
      const saved = localStorage.getItem(storageKey)
      if (saved) data = JSON.parse(saved) as typeof workbookData
    } catch {
      /* corrupt entry — fall through to calamine data */
    }

    // EN locale as complete base, FR overrides what's translated.
    // mergeLocales() does a SHALLOW top-key merge, so custom patches must be
    // deep-merged into the FR pack BEFORE calling mergeLocales — otherwise our
    // patches object { "sheets-ui": { info: {...} } } would REPLACE the entire
    // sheets-ui key from EN/FR, wiping toolbar/align/etc.
    const frWithPatches = structuredClone(UniverPresetSheetsCoreFrFR) as Record<string, Record<string, Record<string, string>>>
    frWithPatches["sheets-ui"] ??= {}
    frWithPatches["sheets-ui"]["info"] ??= {}
    frWithPatches["sheets-ui"]["info"]["error"] = "Erreur dans la cellule"
    frWithPatches["sheets-ui"]["info"]["forceStringInfo"] = "Valeur forcée en texte"
    const patchedLocale = mergeLocales(UniverPresetSheetsCoreEnUS, frWithPatches)

    const { univerAPI } = createUniver({
      locale: LocaleType.FR_FR,
      locales: { [LocaleType.FR_FR]: patchedLocale },
      presets: [
        UniverSheetsCorePreset({
          container: el,
          formulaBar: false,
        }),
      ],
    })

    univerAPI.createWorkbook(data)
    univerRef.current = univerAPI
    setReady(true)

    // Track edits: set dirty on any command, autosave to localStorage
    // as crash recovery (not the primary save — that's the explicit Save
    // button which uploads to the agent realm).
    let timer: ReturnType<typeof setTimeout> | null = null
    const sub = univerAPI.onCommandExecuted(() => {
      setDirty(true)
      if (timer) clearTimeout(timer)
      timer = setTimeout(() => {
        const wb = univerAPI.getActiveWorkbook()
        if (wb) {
          try {
            localStorage.setItem(storageKey, JSON.stringify(wb.getSnapshot()))
          } catch {
            /* quota exceeded — silently skip */
          }
        }
      }, 1000)
    })

    return () => {
      // Flush pending save on unmount.
      if (timer) clearTimeout(timer)
      const wb = univerAPI.getActiveWorkbook()
      if (wb) {
        try {
          localStorage.setItem(storageKey, JSON.stringify(wb.getSnapshot()))
        } catch {
          /* quota exceeded — silently skip */
        }
      }
      sub.dispose()
      univerAPI.dispose()
      univerRef.current = null
      setReady(false)
    }
  }, [workbookData, storageKey])

  return (
    <div className="relative flex min-h-0 flex-1 flex-col">
      {/* Save + dirty indicator — top-right overlay */}
      {(dirty || saveStatus !== "idle") && (
        <div className="pointer-events-none absolute right-2 top-1.5 z-50 flex items-center gap-1.5">
          {dirty && agentId && (
            <button
              type="button"
              className="pointer-events-auto flex items-center gap-1 rounded-md bg-warn/90 px-2 py-0.5 text-[10px] font-semibold text-primary-foreground transition-colors hover:bg-warn"
              onClick={() => void handleSave()}
              title="Save changes to file"
            >
              <Save className="size-3" />
              <span>Save</span>
            </button>
          )}
          {dirty && !agentId && (
            <span className="rounded-md bg-warn/90 px-2 py-0.5 text-[10px] font-medium text-primary-foreground">
              Unsaved changes
            </span>
          )}
          {saveStatus === "saving" && (
            <span className="rounded-md bg-muted/90 px-2 py-0.5 text-[10px] font-medium text-muted-foreground">
              Saving…
            </span>
          )}
          {saveStatus === "saved" && (
            <span className="rounded-md bg-muted/90 px-2 py-0.5 text-[10px] font-medium text-ok">
              Saved ✓
            </span>
          )}
          {saveStatus === "error" && (
            <span
              className="rounded-md bg-danger/90 px-2 py-0.5 text-[10px] font-medium text-primary-foreground"
              title={saveError ?? undefined}
            >
              Save failed
            </span>
          )}
        </div>
      )}

      {/* Univer mount point */}
      <div
        ref={containerRef}
        className={cn(
          "univer-container min-h-0 flex-1 transition-opacity duration-150",
          ready ? "opacity-100" : "opacity-0",
        )}
      />
    </div>
  )
}
