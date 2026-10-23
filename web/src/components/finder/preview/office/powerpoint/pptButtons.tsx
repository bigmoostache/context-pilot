import type { CSSProperties, ReactNode } from "react"
import { usePptxCommand } from "@betteroffice/pptx-react"
import type { PptxCommandArgs } from "@betteroffice/pptx-react"
import { PPT } from "./pptTheme"

type NoArgCommand = "bold" | "italic" | "underline" | "undo" | "redo" | "insertImage"

/** A ribbon button bound to a command that takes no arguments. */
export function Toggle({ id, icon }: { id: NoArgCommand; icon: ReactNode }) {
  const cmd = usePptxCommand(id)
  return (
    <RibbonButton
      label={cmd.label}
      active={cmd.state.active === true}
      enabled={cmd.state.enabled}
      onClick={() => void cmd.execute(null)}
      icon={icon}
    />
  )
}

type ValuedId = "alignment" | "fontSizeStep" | "insertSlide" | "tool"

/** A ribbon button bound to one fixed choice of a command, e.g. centre alignment. */
export function Valued<K extends ValuedId>({
  id,
  args,
  icon,
}: {
  id: K
  args: PptxCommandArgs[K]
  icon: ReactNode
}) {
  const cmd = usePptxCommand(id, args)
  return (
    <RibbonButton
      label={cmd.label}
      active={cmd.state.active === true}
      enabled={cmd.state.enabled}
      onClick={() => void cmd.execute(args)}
      icon={icon}
    />
  )
}

/** Font name and size boxes, read-only mirrors of the text selection. */
export function FontBox() {
  const family = usePptxCommand("fontFamily")
  const size = usePptxCommand("fontSize")
  return (
    <>
      <span style={box(126)}>{family.state.value ?? ""}</span>
      <span style={box(56)}>{size.state.value ?? ""}</span>
    </>
  )
}

function box(width: number): CSSProperties {
  return {
    width,
    height: 28,
    marginRight: 4,
    background: PPT.surface,
    border: `1px solid ${PPT.line}`,
    borderRadius: 4,
    padding: "0 8px",
    display: "inline-flex",
    alignItems: "center",
    fontSize: 14,
    color: PPT.ink,
    overflow: "hidden",
    whiteSpace: "nowrap",
    boxSizing: "border-box",
  }
}

/** The 32×32 icon button every ribbon command uses; pressed state shades it grey. */
export function RibbonButton({
  label,
  active,
  enabled,
  onClick,
  icon,
}: {
  label: string
  active: boolean
  enabled: boolean
  onClick: () => void
  icon: ReactNode
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      aria-pressed={active}
      disabled={!enabled}
      onClick={onClick}
      style={{
        width: 32,
        height: 32,
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        border: "none",
        borderRadius: 4,
        color: enabled ? PPT.ink : PPT.inkDisabled,
        background: active ? PPT.pressed : "transparent",
        cursor: enabled ? "pointer" : "default",
      }}
    >
      {icon}
    </button>
  )
}

/** Vertical divider between ribbon groups. */
export function Sep() {
  return <span style={{ width: 1, height: 20, background: PPT.line, margin: "0 4px" }} />
}
