import { useDocxCommand } from "@betteroffice/docx-react"
import type { DocxCommandArgs, DocxCommandId, DocxCommandValues } from "@betteroffice/docx-react"
import type { Cmd } from "../fluent/tokens"

/** A docx command in the kit's `Cmd` shape. */
export type WordCmd<K extends DocxCommandId> = Cmd<
  NonNullable<DocxCommandValues[K]>,
  DocxCommandArgs[K]
>

/**
 * Binds a docx command and adapts it to the kit: `'mixed'` toggles read as not
 * pressed, `null` values as absent, and a call without args falls back to the
 * args bound here.
 */
export function useWordCmd<K extends DocxCommandId>(id: K, args?: DocxCommandArgs[K]): WordCmd<K> {
  const c = useDocxCommand(id, args)
  const { enabled, active, value } = c.state
  return {
    label: c.label,
    state: {
      enabled,
      active: active === undefined ? undefined : active === true,
      value: value ?? undefined,
    },
    execute: (a) => c.execute(a ?? args),
  }
}

/** Word's font size list (Font Size box and grow/shrink steps). */
export const FONT_SIZES = [8, 9, 10, 10.5, 11, 12, 14, 16, 18, 20, 24, 28, 36, 48, 72] as const

/** Increase/Decrease Font Size: next entry of {@link FONT_SIZES}, ±1 pt past the ends. */
export function useFontStep(delta: 1 | -1): Cmd {
  const size = useWordCmd("fontSize")
  const current = size.state.value ?? 11
  return {
    label: delta > 0 ? "Increase Font Size" : "Decrease Font Size",
    state: { enabled: size.state.enabled },
    execute: () => size.execute({ points: nextSize(current, delta) }),
  }
}

function nextSize(current: number, delta: 1 | -1): number {
  const next =
    delta > 0 ? FONT_SIZES.find((s) => s > current) : FONT_SIZES.findLast((s) => s < current)
  return next ?? Math.max(1, current + delta)
}

/** Fixes a command's args so it fits the kit's no-arg `Cmd` (search items). */
export function plain<V, A>(cmd: Cmd<V, A>, args: A): Cmd {
  return { label: cmd.label, state: cmd.state, execute: () => cmd.execute(args) }
}
