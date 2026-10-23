import {
  AArrowDown,
  AArrowUp,
  AlignCenter,
  AlignJustify,
  AlignLeft,
  AlignRight,
  Bold,
  ImagePlus,
  Italic,
  MousePointer2,
  Plus,
  Redo2,
  Shapes,
  Type,
  Underline,
  Undo2,
} from "lucide-react"
import { FontBox, Sep, Toggle, Valued } from "./pptButtons"
import { PPT, PPT_TABS } from "./pptTheme"

const ICON = 16

/**
 * PowerPoint's tab row plus single-line Home ribbon, passed to PptxEditor as
 * `toolbar` so every button sits inside the editor's command context.
 */
export function PptRibbon() {
  return (
    <div style={{ background: PPT.chrome, fontFamily: PPT.font }}>
      <PptTabs />
      <div style={{ height: PPT.ribbonH, padding: "0 8px", display: "flex" }}>
        <div
          style={{
            flex: 1,
            display: "flex",
            alignItems: "center",
            gap: 0,
            padding: "0 8px",
            background: PPT.surface,
            borderRadius: 8,
            boxShadow: PPT.shadow,
          }}
        >
          <Toggle id="undo" icon={<Undo2 size={ICON} />} />
          <Toggle id="redo" icon={<Redo2 size={ICON} />} />
          <Sep />
          <Valued id="insertSlide" args={{}} icon={<Plus size={ICON} />} />
          <Sep />
          <FontBox />
          <Valued
            id="fontSizeStep"
            args={{ direction: "increase" }}
            icon={<AArrowUp size={ICON} />}
          />
          <Valued
            id="fontSizeStep"
            args={{ direction: "decrease" }}
            icon={<AArrowDown size={ICON} />}
          />
          <Toggle id="bold" icon={<Bold size={ICON} />} />
          <Toggle id="italic" icon={<Italic size={ICON} />} />
          <Toggle id="underline" icon={<Underline size={ICON} />} />
          <Sep />
          <Valued id="alignment" args={{ value: "l" }} icon={<AlignLeft size={ICON} />} />
          <Valued id="alignment" args={{ value: "ctr" }} icon={<AlignCenter size={ICON} />} />
          <Valued id="alignment" args={{ value: "r" }} icon={<AlignRight size={ICON} />} />
          <Valued id="alignment" args={{ value: "just" }} icon={<AlignJustify size={ICON} />} />
          <Sep />
          <Valued id="tool" args={{ value: "select" }} icon={<MousePointer2 size={ICON} />} />
          <Valued id="tool" args={{ value: "textBox" }} icon={<Type size={ICON} />} />
          <Valued id="tool" args={{ value: "shape:rect" }} icon={<Shapes size={ICON} />} />
          <Toggle id="insertImage" icon={<ImagePlus size={ICON} />} />
        </div>
      </div>
    </div>
  )
}

/** Tab row. Only tabs with live commands are listed (no dead controls). */
function PptTabs() {
  return (
    <div
      role="tablist"
      style={{
        height: PPT.tabsH,
        padding: "0 8px",
        display: "flex",
        alignItems: "flex-start",
      }}
    >
      {PPT_TABS.map((label) => (
        <span key={label} role="tab" aria-selected style={tab(true)}>
          {label}
        </span>
      ))}
    </div>
  )
}

function tab(active: boolean) {
  return {
    height: 31,
    padding: "0 10px",
    display: "inline-flex",
    alignItems: "center",
    fontSize: 14,
    fontWeight: active ? 600 : 400,
    color: active ? PPT.ink : PPT.inkTab,
    boxShadow: active ? `inset 0 -3px 0 ${PPT.brand}` : "none",
    cursor: "default",
    whiteSpace: "nowrap" as const,
  }
}
