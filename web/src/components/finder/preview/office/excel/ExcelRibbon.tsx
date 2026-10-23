import type { ReactNode } from "react"
import { EditorToolbar, useXlsxCommand } from "@betteroffice/xlsx-react"
import type { XlsxCommandArgs } from "@betteroffice/xlsx-react"
import {
  AlignCenter,
  AlignLeft,
  AlignRight,
  Bold,
  DollarSign,
  Italic,
  Percent,
  Redo2,
  Strikethrough,
  TableCellsMerge,
  Undo2,
  WrapText,
} from "lucide-react"
import { EXCEL } from "./excelTheme"

type NoArgCommand = "bold" | "italic" | "strikethrough" | "undo" | "redo"

/**
 * Excel's single-line Home ribbon, passed to XlsxEditor as `toolbar` so it
 * renders inside the editor's command context.
 */
export function ExcelRibbon() {
  return (
    <div
      style={{
        height: EXCEL.ribbonH,
        background: EXCEL.chrome,
        padding: "0 8px",
        fontFamily: EXCEL.font,
        display: "flex",
      }}
    >
      <div
        style={{
          flex: 1,
          display: "flex",
          alignItems: "center",
          gap: 2,
          padding: "0 8px",
          background: EXCEL.surface,
          borderRadius: 8,
          boxShadow: "0 0 2px rgba(0,0,0,0.12), 0 1px 2px rgba(0,0,0,0.14)",
        }}
      >
        <Toggle id="undo" icon={<Undo2 size={16} />} />
        <Toggle id="redo" icon={<Redo2 size={16} />} />
        <Sep />
        <FontBox />
        <Sep />
        <Toggle id="bold" icon={<Bold size={16} />} />
        <Toggle id="italic" icon={<Italic size={16} />} />
        <Toggle id="strikethrough" icon={<Strikethrough size={16} />} />
        <Sep />
        <Valued id="horizontalAlignment" args={{ value: "left" }} icon={<AlignLeft size={16} />} />
        <Valued
          id="horizontalAlignment"
          args={{ value: "center" }}
          icon={<AlignCenter size={16} />}
        />
        <Valued
          id="horizontalAlignment"
          args={{ value: "right" }}
          icon={<AlignRight size={16} />}
        />
        <Valued id="textWrapping" args={{ value: "wrap" }} icon={<WrapText size={16} />} />
        <Valued id="merge" args={{ value: "all" }} icon={<TableCellsMerge size={16} />} />
        <Sep />
        <Valued id="numberFormat" args={{ value: "currency" }} icon={<DollarSign size={16} />} />
        <Valued id="numberFormat" args={{ value: "percent" }} icon={<Percent size={16} />} />
      </div>
    </div>
  )
}

/**
 * Ribbon plus the engine's name box and formula input in Excel's 32px row.
 * Passing `toolbar` drops the stock formula bar, so it is mounted back here.
 */
export function ExcelToolbar() {
  return (
    <>
      <ExcelRibbon />
      <div
        style={{
          height: EXCEL.formulaH,
          background: EXCEL.chrome,
          padding: "0 8px",
          display: "flex",
          alignItems: "center",
          fontFamily: EXCEL.font,
        }}
      >
        <EditorToolbar.FormulaBar style={{ flex: 1, fontSize: 14.67, color: EXCEL.ink }} />
      </div>
    </>
  )
}

/** Font family and size boxes, read-only mirrors of the selection. */
function FontBox() {
  const family = useXlsxCommand("fontFamily")
  const size = useXlsxCommand("fontSize")
  return (
    <>
      <span style={box(128, "#f0f0f0")}>{family.state.value ?? "Aptos Narrow"}</span>
      <span style={box(58, EXCEL.surface)}>{size.state.value ?? 11}</span>
    </>
  )
}

function box(width: number, background: string) {
  return {
    width,
    height: 28,
    background,
    border: `1px solid ${EXCEL.line}`,
    borderRadius: 4,
    padding: "0 8px",
    display: "inline-flex",
    alignItems: "center",
    fontSize: 14,
    color: EXCEL.ink,
    overflow: "hidden",
    whiteSpace: "nowrap" as const,
    boxSizing: "border-box" as const,
  }
}

function Toggle({ id, icon }: { id: NoArgCommand; icon: ReactNode }) {
  const cmd = useXlsxCommand(id)
  return (
    <RibbonButton
      label={cmd.label}
      active={cmd.state.active === true}
      enabled={cmd.state.enabled}
      onClick={() => void cmd.execute()}
      icon={icon}
    />
  )
}

type ValuedId = "horizontalAlignment" | "textWrapping" | "merge" | "numberFormat"

function Valued<K extends ValuedId>({
  id,
  args,
  icon,
}: {
  id: K
  args: XlsxCommandArgs[K]
  icon: ReactNode
}) {
  const cmd = useXlsxCommand(id, args)
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

function RibbonButton({
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
      className="excel-ribbon-btn"
      style={{
        width: 32,
        height: 32,
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        border: "none",
        borderRadius: 4,
        color: EXCEL.ink,
        background: active ? EXCEL.pressed : "transparent",
        opacity: enabled ? 1 : 0.4,
        cursor: enabled ? "pointer" : "default",
      }}
    >
      {icon}
    </button>
  )
}

function Sep() {
  return <span style={{ width: 1, height: 20, background: EXCEL.line, margin: "0 6px" }} />
}
