import { EditorToolbar, useXlsxCommand } from "@betteroffice/xlsx-react"
import type { XlsxCommandArgs, XlsxCommandId, XlsxCommandValues } from "@betteroffice/xlsx-react"
import {
  ArrowRedo20Regular,
  ArrowUndo20Regular,
  CurrencyDollarEuro20Regular,
  TableCellsMerge20Regular,
  TextAlignCenter20Regular,
  TextAlignLeft20Regular,
  TextAlignRight20Regular,
  TextBold20Regular,
  TextItalic20Regular,
  TextStrikethrough20Regular,
  TextWrap20Regular,
} from "@fluentui/react-icons"
import type { ReactNode } from "react"
import { Btn, Combo, Sep } from "../fluent/controls"
import { OfficeRibbon } from "../fluent/Frame"
import { StatusBar, Zoom } from "../fluent/StatusBar"
import type { Cmd } from "../fluent/tokens"
import { EXCEL } from "./excelTheme"

/**
 * An xlsx-react command in the kit's engine-agnostic `Cmd` shape. `'mixed'`
 * reads as not pressed and a `null` value (mixed selection) as no value.
 */
function useExcelCmd<K extends XlsxCommandId>(
  id: K,
  args?: XlsxCommandArgs[K],
): Cmd<NonNullable<XlsxCommandValues[K]>, XlsxCommandArgs[K]> {
  const c = useXlsxCommand(id, args)
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

/**
 * Excel for the web's single-line ribbon on the shared Office kit, with the
 * formula bar below it and the zoom status bar portalled into the shell.
 * Passed to XlsxEditor as `toolbar` so every control resolves editor commands
 * (passing `toolbar` also drops the stock formula bar, mounted back here).
 */
export function ExcelToolbar() {
  return (
    <OfficeRibbon
      layout="single"
      tabs={[{ id: "home", label: "Home", ribbon: <HomeRibbon /> }]}
      belowRibbon={<FormulaRow />}
      status={<ExcelStatus />}
    />
  )
}

function HomeRibbon() {
  return (
    <>
      <Plain id="undo" icon={<ArrowUndo20Regular />} />
      <Plain id="redo" icon={<ArrowRedo20Regular />} />
      <Sep />
      <FontBox />
      <Sep />
      <Plain id="bold" icon={<TextBold20Regular />} />
      <Plain id="italic" icon={<TextItalic20Regular />} />
      <Plain id="strikethrough" icon={<TextStrikethrough20Regular />} />
      <Sep />
      <Valued id="horizontalAlignment" args={{ value: "left" }} icon={<TextAlignLeft20Regular />} />
      <Valued
        id="horizontalAlignment"
        args={{ value: "center" }}
        icon={<TextAlignCenter20Regular />}
      />
      <Valued
        id="horizontalAlignment"
        args={{ value: "right" }}
        icon={<TextAlignRight20Regular />}
      />
      <Valued id="textWrapping" args={{ value: "wrap" }} icon={<TextWrap20Regular />} />
      <Valued id="merge" args={{ value: "all" }} icon={<TableCellsMerge20Regular />} />
      <Sep />
      <Valued
        id="numberFormat"
        args={{ value: "currency" }}
        icon={<CurrencyDollarEuro20Regular />}
      />
      <Valued id="numberFormat" args={{ value: "percent" }} icon={<PercentGlyph />} />
    </>
  )
}

/** Excel's Percent Style button shows a bare "%" (Fluent has no plain percent icon). */
function PercentGlyph() {
  return <span style={{ fontSize: 16, lineHeight: "20px", width: 20 }}>%</span>
}

/** The engine's name box and formula input in Excel's 32px row. */
function FormulaRow() {
  return (
    <div
      style={{
        height: EXCEL.formulaH,
        background: "var(--of-chrome)",
        padding: "0 8px",
        display: "flex",
        alignItems: "center",
      }}
    >
      <EditorToolbar.FormulaBar style={{ flex: 1, fontSize: 14.67, color: "var(--of-ink)" }} />
    </div>
  )
}

/** Status bar: zoom, a live engine command (no fit-to-window in the engine). */
function ExcelStatus() {
  const zoom = useExcelCmd("zoom")
  return (
    <StatusBar
      right={
        <span style={{ display: "flex", alignItems: "center", marginRight: 11 }}>
          <Zoom cmd={zoom} toArgs={(s) => ({ scale: s === "fit" ? 1 : s })} fit={false} />
        </span>
      }
    />
  )
}

type NoArgCommand = "bold" | "italic" | "strikethrough" | "undo" | "redo"

function Plain({ id, icon }: { id: NoArgCommand; icon: ReactNode }) {
  const cmd = useExcelCmd(id)
  return <Btn cmd={cmd} args={null} icon={icon} />
}

type ValuedId = "horizontalAlignment" | "textWrapping" | "merge" | "numberFormat"

/** A command bound to one value; the engine reports `active` for that value. */
function Valued<K extends ValuedId>({
  id,
  args,
  icon,
}: {
  id: K
  args: XlsxCommandArgs[K]
  icon: ReactNode
}) {
  const cmd = useExcelCmd(id, args)
  return <Btn cmd={cmd} args={args} icon={icon} />
}

/** Common Excel fonts offered in the family list; any typed name is accepted. */
const FONTS = [
  "Aptos Narrow",
  "Aptos",
  "Calibri",
  "Arial",
  "Cambria",
  "Consolas",
  "Segoe UI",
  "Times New Roman",
] as const
/** Excel's font size list (points). */
const SIZES = [8, 9, 10, 11, 12, 14, 16, 18, 20, 22, 24, 26, 28, 36, 48, 72] as const

/** Editable font family and size boxes bound to the selection's font commands. */
function FontBox() {
  const family: Cmd<string, { family: string }> = useExcelCmd("fontFamily")
  const size: Cmd<number, { points: number }> = useExcelCmd("fontSize")
  return (
    <>
      <Combo
        label={family.label}
        width={128}
        options={FONTS}
        cmd={family}
        toArgs={(f) => ({ family: f })}
      />
      <Combo
        label={size.label}
        width={58}
        options={SIZES}
        cmd={size}
        toArgs={(t) => ({ points: Math.min(400, Math.max(1, Number(t) || 11)) })}
      />
    </>
  )
}
