import type { CSSProperties, ReactNode } from "react"
import { useDocxCommand } from "@betteroffice/docx-react"
import type { DocxCommandArgs } from "@betteroffice/docx-react"
import { WORD } from "./wordTheme"

/** Commands that take no arguments and toggle or fire on click. */
export type PlainCommand =
  | "undo"
  | "redo"
  | "bold"
  | "italic"
  | "underline"
  | "strikethrough"
  | "subscript"
  | "superscript"
  | "clearFormatting"
  | "bulletList"
  | "numberedList"
  | "indent"
  | "outdent"
  | "find"
  | "replace"

/** A 28×24 ribbon button bound to a no-argument docx command. */
export function Cmd({ id, icon, text }: { id: PlainCommand; icon: ReactNode; text?: string }) {
  const cmd = useDocxCommand(id)
  return (
    <RibbonButton
      label={cmd.label}
      active={cmd.state.active === true}
      enabled={cmd.state.enabled}
      onClick={() => void cmd.execute()}
      icon={icon}
      text={text}
    />
  )
}

/** A 28×24 alignment button; active when the selection has that alignment. */
export function Align({
  value,
  icon,
}: {
  value: DocxCommandArgs["alignment"]["value"]
  icon: ReactNode
}) {
  const cmd = useDocxCommand("alignment", { value })
  return (
    <RibbonButton
      label={cmd.label}
      active={cmd.state.value === value}
      enabled={cmd.state.enabled}
      onClick={() => void cmd.execute({ value })}
      icon={icon}
    />
  )
}

/** Grow/shrink font buttons: step the selection's size by one point. */
export function FontStep({ delta, icon }: { delta: 1 | -1; icon: ReactNode }) {
  const cmd = useDocxCommand("fontSize")
  const current = cmd.state.value ?? 11
  return (
    <RibbonButton
      label={delta > 0 ? "Grow Font" : "Shrink Font"}
      active={false}
      enabled={cmd.state.enabled}
      onClick={() => void cmd.execute({ points: Math.max(1, current + delta) })}
      icon={icon}
    />
  )
}

/** A control with no engine command yet: Word's look, no action. */
export function Static({ label, icon, text }: { label: string; icon: ReactNode; text?: string }) {
  return <RibbonButton label={label} active={false} enabled icon={icon} text={text} />
}

/** Font name and size boxes mirroring the selection, as in Word's Font group. */
export function FontBoxes() {
  const family = useDocxCommand("fontFamily")
  const size = useDocxCommand("fontSize")
  return (
    <>
      <span style={field(126)}>{family.state.value ?? "Calibri"}</span>
      <span style={field(56)}>{size.state.value ?? 11}</span>
    </>
  )
}

/** A big 40-wide button with its label under the icon (Paste, Dictate…). */
export function BigButton({ label, icon }: { label: string; icon: ReactNode }) {
  return (
    <button type="button" title={label} aria-label={label} style={bigStyle}>
      {icon}
      <span style={{ fontSize: 12, lineHeight: "16px" }}>{label}</span>
    </button>
  )
}

/** Vertical hairline between ribbon groups (17px slot, as measured). */
export function GroupSep() {
  return (
    <span style={{ width: 17, alignSelf: "stretch", display: "flex", justifyContent: "center" }}>
      <span style={{ width: 1, margin: "10px 0", background: WORD.line }} />
    </span>
  )
}

function RibbonButton({
  label,
  active,
  enabled,
  onClick,
  icon,
  text,
}: {
  label: string
  active: boolean
  enabled: boolean
  onClick?: (() => void) | undefined
  icon: ReactNode
  text?: string | undefined
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      aria-pressed={active}
      disabled={!enabled}
      onClick={onClick}
      className="word-ribbon-btn"
      style={{
        minWidth: 28,
        height: 24,
        padding: text === undefined ? 0 : "0 6px",
        display: "inline-flex",
        alignItems: "center",
        justifyContent: text === undefined ? "center" : "flex-start",
        gap: 6,
        border: "none",
        borderRadius: 4,
        fontFamily: WORD.font,
        fontSize: 14,
        color: WORD.ink,
        background: active ? WORD.pressed : "transparent",
        opacity: enabled ? 1 : 0.4,
        cursor: enabled ? "pointer" : "default",
      }}
    >
      {icon}
      {text}
    </button>
  )
}

const bigStyle: CSSProperties = {
  width: 52,
  height: 72,
  display: "inline-flex",
  flexDirection: "column",
  alignItems: "center",
  justifyContent: "center",
  gap: 4,
  border: "none",
  borderRadius: 4,
  background: "transparent",
  color: WORD.ink,
  fontFamily: WORD.font,
  cursor: "pointer",
}

function field(width: number): CSSProperties {
  return {
    width,
    height: 22,
    border: `1px solid ${WORD.line}`,
    borderRadius: 4,
    padding: "0 6px",
    display: "inline-flex",
    alignItems: "center",
    fontSize: 14,
    color: WORD.ink,
    background: WORD.surface,
    overflow: "hidden",
    whiteSpace: "nowrap",
    boxSizing: "border-box",
  }
}
