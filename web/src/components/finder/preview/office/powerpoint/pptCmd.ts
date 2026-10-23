import { usePptxCommand } from "@betteroffice/pptx-react"
import type { PptxCommandArgs, PptxCommandId, PptxCommandValues } from "@betteroffice/pptx-react"
import type { Cmd } from "../fluent/tokens"

/**
 * A pptx-react command in the kit's engine-agnostic `Cmd` shape. `'mixed'`
 * reads as not pressed and a `null` value (mixed selection) as no value.
 */
export function usePptCmd<K extends PptxCommandId>(
  id: K,
  args?: PptxCommandArgs[K],
): Cmd<NonNullable<PptxCommandValues[K]>, PptxCommandArgs[K]> {
  const c = usePptxCommand(id, args)
  const { active, value } = c.state
  return {
    label: c.label,
    state: {
      enabled: c.state.enabled,
      active: active === "mixed" ? false : active,
      value: value ?? undefined,
    },
    execute: (a) => c.execute(a),
  }
}
