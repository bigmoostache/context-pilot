/**
 * Univer formula-engine worker for the Finder spreadsheet viewer.
 *
 * Spawned by `SheetGrid` (passed as the core preset's `workerURL`): workbook
 * recalculation runs here instead of on the page's main thread, so opening a
 * large workbook no longer freezes the tab.
 */
import { createUniver } from "@univerjs/presets"
import { UniverSheetsCoreWorkerPreset } from "@univerjs/preset-sheets-core/worker"

createUniver({ presets: [UniverSheetsCoreWorkerPreset()] })
