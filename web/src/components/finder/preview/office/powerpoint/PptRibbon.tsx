import {
  ArrowRedo20Regular,
  ArrowUndo20Regular,
  Cursor20Regular,
  FontDecrease20Regular,
  FontIncrease20Regular,
  ImageAdd20Regular,
  Play16Regular,
  Shapes20Regular,
  TextAlignCenter20Regular,
  TextAlignJustify20Regular,
  TextAlignLeft20Regular,
  TextAlignRight20Regular,
  TextBold20Regular,
  TextField20Regular,
  TextItalic20Regular,
  TextUnderline20Regular,
} from "@fluentui/react-icons"
import type { PptxCommandArgs, PptxEditorTool } from "@betteroffice/pptx-react"
import { Btn, Combo, Sep } from "../fluent/controls"
import { OfficeRibbon } from "../fluent/Frame"
import { StatusBar, StatusItem, Zoom } from "../fluent/StatusBar"
import type { Cmd } from "../fluent/tokens"
import { NewSlide } from "./NewSlide"
import { usePptCmd } from "./pptCmd"

const FAMILIES = ["Calibri", "Calibri Light", "Arial", "Helvetica", "Times New Roman", "Aptos"]
const SIZES = [
  8, 9, 10, 11, 12, 14, 16, 18, 20, 24, 28, 32, 36, 40, 44, 48, 54, 60, 66, 72, 80, 88, 96,
]

/**
 * PowerPoint for the web's single-line ribbon on the shared Office kit. Passed
 * to PptxEditor as `toolbar` so every control resolves editor commands; the
 * status bar is portalled into the shell from here for the same reason.
 */
export function PptRibbon() {
  return (
    <OfficeRibbon
      layout="single"
      tabs={[{ id: "home", label: "Home", ribbon: <HomeRibbon /> }]}
      status={<PptStatus />}
    />
  )
}

function HomeRibbon() {
  const undo = usePptCmd("undo")
  const redo = usePptCmd("redo")
  const grow = usePptCmd("fontSizeStep", { direction: "increase" })
  const shrink = usePptCmd("fontSizeStep", { direction: "decrease" })
  const bold = usePptCmd("bold")
  const italic = usePptCmd("italic")
  const underline = usePptCmd("underline")
  const image = usePptCmd("insertImage")
  return (
    <>
      <Btn cmd={undo} args={null} icon={<ArrowUndo20Regular />} />
      <Btn cmd={redo} args={null} icon={<ArrowRedo20Regular />} />
      <Sep />
      <NewSlide />
      <Sep />
      <FontCombos />
      <Btn
        cmd={{ ...grow, label: "Increase Font Size" }}
        args={{ direction: "increase" }}
        icon={<FontIncrease20Regular />}
      />
      <Btn
        cmd={{ ...shrink, label: "Decrease Font Size" }}
        args={{ direction: "decrease" }}
        icon={<FontDecrease20Regular />}
      />
      <Btn cmd={bold} args={null} icon={<TextBold20Regular />} />
      <Btn cmd={italic} args={null} icon={<TextItalic20Regular />} />
      <Btn cmd={underline} args={null} icon={<TextUnderline20Regular />} />
      <Sep />
      <Align value="l" icon={<TextAlignLeft20Regular />} />
      <Align value="ctr" icon={<TextAlignCenter20Regular />} />
      <Align value="r" icon={<TextAlignRight20Regular />} />
      <Align value="just" icon={<TextAlignJustify20Regular />} />
      <Sep />
      <Tool value="select" label="Select" icon={<Cursor20Regular />} />
      <Tool value="textBox" label="Text Box" icon={<TextField20Regular />} />
      <Tool value="shape:rect" label="Rectangle" icon={<Shapes20Regular />} />
      <Btn cmd={image} args={null} icon={<ImageAdd20Regular />} />
    </>
  )
}

/** Editable font name and size boxes, bound to fontFamily / fontSize. */
function FontCombos() {
  const family = usePptCmd("fontFamily")
  const size = usePptCmd("fontSize")
  return (
    <>
      <Combo
        label={family.label}
        width={128}
        options={FAMILIES}
        cmd={family}
        toArgs={(t) => ({ family: t })}
      />
      <Combo
        label={size.label}
        width={58}
        options={SIZES}
        cmd={size}
        toArgs={(t) => ({ points: clampPoints(Number(t), size.state.value ?? 18) })}
      />
    </>
  )
}

type ParagraphAlignment = PptxCommandArgs["alignment"]["value"]

const ALIGN_LABEL: Record<ParagraphAlignment, string> = {
  l: "Align Left",
  ctr: "Center",
  r: "Align Right",
  just: "Justify",
}

/** Alignment reports a value, not `active`: pressed when the paragraph's alignment is this one. */
function Align({ value, icon }: { value: ParagraphAlignment; icon: React.ReactNode }) {
  const cmd = usePptCmd("alignment", { value })
  return <Btn cmd={pressedWhen(cmd, ALIGN_LABEL[value], value)} args={{ value }} icon={icon} />
}

/** The current tool is a value too: pressed when it is this one. */
function Tool({
  value,
  label,
  icon,
}: {
  value: PptxEditorTool
  label: string
  icon: React.ReactNode
}) {
  const cmd = usePptCmd("tool", { value })
  return <Btn cmd={pressedWhen(cmd, label, value)} args={{ value }} icon={icon} />
}

/** `cmd` renamed to `label`, pressed when its value equals `pressed`. */
function pressedWhen<V, A>(cmd: Cmd<V, A>, label: string, pressed: V): Cmd<V, A> {
  return { ...cmd, label, state: { ...cmd.state, active: cmd.state.value === pressed } }
}

/** Status bar: slideshow + zoom, both live commands; the language is static text. */
function PptStatus() {
  const show = usePptCmd("slideshow")
  const zoom = usePptCmd("zoom")
  const showCmd: Cmd = { ...show, execute: () => show.execute(null) }
  const zoomCmd: Cmd<number, { scale: number | "fit" }> = {
    ...zoom,
    state: {
      ...zoom.state,
      value: typeof zoom.state.value === "number" ? zoom.state.value : undefined,
    },
  }
  return (
    <StatusBar
      left={<StatusItem text="English (U.S.)" />}
      right={
        <>
          <StatusItem cmd={showCmd} icon={<Play16Regular />} />
          <Zoom cmd={zoomCmd} toArgs={(scale) => ({ scale })} />
        </>
      }
    />
  )
}

/** Font size in points, 1 to 400; an unparsable entry keeps the current size. */
function clampPoints(n: number, fallback: number) {
  return Number.isFinite(n) ? Math.min(400, Math.max(1, n)) : fallback
}
