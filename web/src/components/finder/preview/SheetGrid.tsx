import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import type { FUniver, IWorkbookData } from "@univerjs/presets"
import { createUniver, LocaleType, mergeLocales } from "@univerjs/presets"
import { UniverSheetsCorePreset } from "@univerjs/preset-sheets-core"
import UniverPresetSheetsCoreEnUS from "@univerjs/preset-sheets-core/locales/en-US"
import UniverPresetSheetsCoreFrFR from "@univerjs/preset-sheets-core/locales/fr-FR"
import "@univerjs/preset-sheets-core/lib/index.css"
import { UniverSheetsConditionalFormattingPreset } from "@univerjs/preset-sheets-conditional-formatting"
import CfEnUS from "@univerjs/preset-sheets-conditional-formatting/locales/en-US"
import CfFrFR from "@univerjs/preset-sheets-conditional-formatting/locales/fr-FR"
import "@univerjs/preset-sheets-conditional-formatting/lib/index.css"
import { UniverSheetsHyperLinkPreset } from "@univerjs/preset-sheets-hyper-link"
import LinkEnUS from "@univerjs/preset-sheets-hyper-link/locales/en-US"
import LinkFrFR from "@univerjs/preset-sheets-hyper-link/locales/fr-FR"
import "@univerjs/preset-sheets-hyper-link/lib/index.css"
import { UniverSheetsNotePreset } from "@univerjs/preset-sheets-note"
import NoteEnUS from "@univerjs/preset-sheets-note/locales/en-US"
import NoteFrFR from "@univerjs/preset-sheets-note/locales/fr-FR"
import "@univerjs/preset-sheets-note/lib/index.css"
import { UniverSheetsFindReplacePreset } from "@univerjs/preset-sheets-find-replace"
import FindEnUS from "@univerjs/preset-sheets-find-replace/locales/en-US"
import FindFrFR from "@univerjs/preset-sheets-find-replace/locales/fr-FR"
import "@univerjs/preset-sheets-find-replace/lib/index.css"
import { Save } from "lucide-react"
import { cn } from "@/lib/utils"
import { saveToRealm, toWorkbookData, touchedSheet, type SheetPayload } from "./sheetState"

/** Backend Univer snapshot when present (xlsx/xlsm), else the values-only payload. */
function resolveWorkbook(
  workbook: Record<string, unknown> | undefined,
  sheets: SheetPayload[],
): IWorkbookData {
  if (isWorkbookData(workbook)) return workbook
  return toWorkbookData(sheets)
}

/** Shape check for the backend snapshot (free-form object in the OpenAPI contract). */
function isWorkbookData(value: unknown): value is IWorkbookData {
  if (typeof value !== "object" || value === null) return false
  return "sheetOrder" in value && Array.isArray(value.sheetOrder) && "sheets" in value
}

/**
 * EN locale as complete base, FR overrides what's translated.
 * mergeLocales() does a SHALLOW top-key merge, so custom patches must be
 * deep-merged into the FR pack BEFORE calling mergeLocales — otherwise a
 * `{ "sheets-ui": { info } }` patch would REPLACE the entire sheets-ui key.
 */
function patchedLocale() {
  const fr = structuredClone(UniverPresetSheetsCoreFrFR) as Record<
    string,
    Record<string, Record<string, string>>
  >
  fr["sheets-ui"] ??= {}
  fr["sheets-ui"]["info"] ??= {}
  fr["sheets-ui"]["info"]["error"] = "Erreur dans la cellule"
  fr["sheets-ui"]["info"]["forceStringInfo"] = "Valeur forcée en texte"
  return mergeLocales(
    UniverPresetSheetsCoreEnUS,
    CfEnUS,
    LinkEnUS,
    NoteEnUS,
    FindEnUS,
    fr,
    CfFrFR,
    LinkFrFR,
    NoteFrFR,
    FindFrFR,
  )
}

/**
 * Core preset (formula bar; footer with sheet tabs — whose list shows and
 * unhides hidden sheets — selection statistics and zoom slider) plus
 * conditional formats, hyperlinks, notes (read from workbook resources) and
 * find & replace (Ctrl/Cmd+F). Formulas are computed in `worker`.
 */
function sheetPresets(container: HTMLElement, worker: Worker) {
  return [
    UniverSheetsCorePreset({
      container,
      workerURL: worker,
      formulaBar: true,
      footer: { sheetBar: true, statisticBar: true, menus: true, zoomSlider: true },
    }),
    UniverSheetsConditionalFormattingPreset(),
    UniverSheetsHyperLinkPreset(),
    UniverSheetsNotePreset(),
    UniverSheetsFindReplacePreset(),
  ]
}

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
  workbook,
  activeSheet,
  path,
  fleetId,
}: {
  sheets: SheetPayload[]
  /** Backend Univer snapshot (xlsx/xlsm) and the sheet id it opens on. Wins over `sheets`. */
  workbook?: Record<string, unknown> | undefined
  activeSheet?: string | undefined
  path: string
  /** Agent realm to save back to. Without this, Save is disabled. */
  fleetId?: string | undefined
}) {
  const containerRef = useRef<HTMLDivElement>(null)
  const univerRef = useRef<FUniver | null>(null)
  // Sheets the user changed since the last save: Save uploads only these.
  const touchedRef = useRef(new Set<string>())
  const [ready, setReady] = useState(false)
  const [dirty, setDirty] = useState(false)
  const [saveStatus, setSaveStatus] = useState<"idle" | "saving" | "saved" | "error">("idle")
  const [saveError, setSaveError] = useState<string | null>(null)

  const workbookData = useMemo(() => resolveWorkbook(workbook, sheets), [workbook, sheets])
  // v2: snapshots from the old values-only payload must not mask the full workbook.
  const storageKey = `univer-sheet:v2:${path}`
  // Backend workbooks (xlsx/xlsm) skip the localStorage backup: serializing a
  // multi-MB snapshot after every edit froze the page and overflowed the quota.
  const backup = !workbook

  const handleSave = useCallback(async () => {
    if (!univerRef.current || !fleetId) return
    setSaveStatus("saving")
    setSaveError(null)
    try {
      await saveToRealm(univerRef.current, fleetId, path, touchedRef.current)
      touchedRef.current.clear()
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
  }, [fleetId, path, storageKey])

  useEffect(() => {
    const el = containerRef.current
    if (!el) return

    // Prefer localStorage snapshot over calamine data (survives reloads).
    let data = workbookData
    try {
      const saved = localStorage.getItem(storageKey)
      if (!backup) localStorage.removeItem(storageKey)
      else if (saved) data = JSON.parse(saved) as typeof workbookData
    } catch {
      /* corrupt entry — fall through to calamine data */
    }

    // Formula engine off the main thread: recalculating a large workbook froze the tab.
    const worker = new Worker(new URL("../support/sheetWorker.ts", import.meta.url), {
      type: "module",
    })
    const { univerAPI } = createUniver({
      locale: LocaleType.FR_FR,
      locales: { [LocaleType.FR_FR]: patchedLocale() },
      presets: sheetPresets(el, worker),
    })

    // Univer mutates the cells it is given; a clone keeps those edits out of the
    // react-query cache (a remount would otherwise send style ids from a dead table).
    const fWorkbook = univerAPI.createWorkbook(structuredClone(data))
    const opening = activeSheet ? fWorkbook.getSheetBySheetId(activeSheet) : null
    if (opening) fWorkbook.setActiveSheet(opening)
    univerRef.current = univerAPI
    // Deferred: avoids synchronous setState in effect (eslint-react/set-state-in-effect).
    const readyFrame = requestAnimationFrame(() => setReady(true))

    // Track edits: set dirty on any command, autosave to localStorage
    // as crash recovery (not the primary save — that's the explicit Save
    // button which uploads to the agent realm).
    let timer: ReturnType<typeof setTimeout> | null = null
    touchedRef.current.clear()
    const sub = univerAPI.addEvent(univerAPI.Event.CommandExecuted, (event) => {
      const sheetId = touchedSheet(event)
      if (sheetId) touchedRef.current.add(sheetId)
      setDirty(true)
      if (!backup) return
      if (timer) clearTimeout(timer)
      timer = setTimeout(() => {
        const wb = univerAPI.getActiveWorkbook()
        if (wb) {
          try {
            localStorage.setItem(storageKey, JSON.stringify(wb.save()))
          } catch {
            /* quota exceeded — silently skip */
          }
        }
      }, 1000)
    })

    return () => {
      // Flush pending save on unmount.
      if (timer) clearTimeout(timer)
      cancelAnimationFrame(readyFrame)
      const wb = backup ? univerAPI.getActiveWorkbook() : null
      if (wb) {
        try {
          localStorage.setItem(storageKey, JSON.stringify(wb.save()))
        } catch {
          /* quota exceeded — silently skip */
        }
      }
      sub.dispose()
      univerAPI.dispose()
      worker.terminate()
      univerRef.current = null
      setReady(false)
    }
  }, [workbookData, storageKey, activeSheet, backup])

  return (
    <div className="relative flex min-h-0 flex-1 flex-col">
      {/* Save + dirty indicator — top-right overlay */}
      {(dirty || saveStatus !== "idle") && (
        <div className="pointer-events-none absolute top-1.5 right-2 z-50 flex items-center gap-1.5">
          {dirty && fleetId && (
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
          {dirty && !fleetId && (
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
