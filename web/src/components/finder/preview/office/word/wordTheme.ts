import { FLUENT } from "../fluent/tokens"

/**
 * DocxEditor `style` override: the engine paints its canvas with `--doc-bg`
 * (#f8f9fa, set on `.oox-root`), so a parent cannot recolour it; Word's canvas
 * is the same #f5f5f5 as its chrome.
 */
export const WORD_EDITOR_STYLE = {
  "--doc-bg": FLUENT.chrome,
  "--doc-bg-subtle": FLUENT.chrome,
} as const
