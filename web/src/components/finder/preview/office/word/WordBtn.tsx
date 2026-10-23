import type { ReactNode } from "react"
import type { DocxCommandArgs, DocxCommandId, DocxCommandValues } from "@betteroffice/docx-react"
import { Btn } from "../fluent/controls"
import { useWordCmd } from "./wordCmd"

/**
 * A kit `Btn` bound to one docx command. `pressedValue` marks it pressed when
 * the command's value equals it (alignment, editing mode), for commands that
 * report a value rather than `active`.
 */
export function W<K extends DocxCommandId>({
  id,
  args,
  icon,
  text,
  size,
  pressedValue,
}: {
  id: K
  args?: DocxCommandArgs[K] | undefined
  icon: ReactNode
  text?: string | undefined
  size?: "sm" | "lg"
  pressedValue?: DocxCommandValues[K] | undefined
}) {
  const cmd = useWordCmd(id, args)
  const bound =
    pressedValue === undefined
      ? cmd
      : { ...cmd, state: { ...cmd.state, active: cmd.state.value === pressedValue } }
  return <Btn cmd={bound} args={args} icon={icon} text={text} size={size ?? "sm"} />
}
