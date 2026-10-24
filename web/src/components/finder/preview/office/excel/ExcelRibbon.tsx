import { EditorToolbar, useXlsxCommand } from "@betteroffice/xlsx-react"
import type { XlsxCommandArgs, XlsxCommandId, XlsxCommandValues } from "@betteroffice/xlsx-react"
import {
  AlignBottom20Regular,
  AlignCenterVertical20Regular,
  AlignTop20Regular,
  ArrowRedo20Regular,
  ArrowUndo20Regular,
  BorderBottom20Regular,
  CurrencyDollarEuro20Regular,
  DecimalArrowLeft20Regular,
  DecimalArrowRight20Regular,
  FontDecrease20Regular,
  FontIncrease20Regular,
  NumberSymbol20Regular,
  PaintBrush20Regular,
  PaintBucket20Regular,
  TableCellsMerge20Regular,
  TextAlignCenter20Regular,
  TextAlignLeft20Regular,
  TextAlignRight20Regular,
  TextBold20Regular,
  TextColor20Regular,
  TextItalic20Regular,
  TextStrikethrough20Regular,
  TextWrap20Regular,
  type FluentIcon,
} from "@fluentui/react-icons"
import { useState, type ReactNode } from "react"
import { Btn, Combo, Sep } from "../fluent/controls"
import { ColorPicker, DropBtn, Menu, MenuItem, SplitBtn } from "../fluent/menus"
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

/** Excel web's Home tab, left to right; only groups the engine has commands for. */
function HomeRibbon() {
  return (
    <>
      <Plain id="undo" icon={<ArrowUndo20Regular />} />
      <Plain id="redo" icon={<ArrowRedo20Regular />} />
      <Sep />
      <Plain id="paintFormat" icon={<PaintBrush20Regular />} />
      <Sep />
      <FontGroup />
      <Sep />
      <AlignmentGroup />
      <Sep />
      <NumberGroup />
    </>
  )
}

function FontGroup() {
  return (
    <>
      <FontBox />
      <Valued id="fontSizeStep" args={{ direction: "increase" }} icon={<FontIncrease20Regular />} />
      <Valued id="fontSizeStep" args={{ direction: "decrease" }} icon={<FontDecrease20Regular />} />
      <Plain id="bold" icon={<TextBold20Regular />} />
      <Plain id="italic" icon={<TextItalic20Regular />} />
      <Plain id="strikethrough" icon={<TextStrikethrough20Regular />} />
      <BordersSplit />
      <ColorSplit id="fillColor" start="FFFF00" label="Fill colours" Icon={PaintBucket20Regular} />
      <ColorSplit id="textColor" start="FF0000" label="Font colours" Icon={TextColor20Regular} />
    </>
  )
}

function AlignmentGroup() {
  return (
    <>
      <Valued id="verticalAlignment" args={{ value: "top" }} icon={<AlignTop20Regular />} />
      <Valued
        id="verticalAlignment"
        args={{ value: "middle" }}
        icon={<AlignCenterVertical20Regular />}
      />
      <Valued id="verticalAlignment" args={{ value: "bottom" }} icon={<AlignBottom20Regular />} />
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
    </>
  )
}

function NumberGroup() {
  return (
    <>
      <NumberFormatMenu />
      <Valued
        id="numberFormat"
        args={{ value: "currency" }}
        icon={<CurrencyDollarEuro20Regular />}
      />
      <Valued id="numberFormat" args={{ value: "percent" }} icon={<PercentGlyph />} />
      <Valued
        id="decimalPlaces"
        args={{ direction: "increase" }}
        icon={<DecimalArrowRight20Regular />}
      />
      <Valued
        id="decimalPlaces"
        args={{ direction: "decrease" }}
        icon={<DecimalArrowLeft20Regular />}
      />
    </>
  )
}

type Format = XlsxCommandArgs["numberFormat"]["value"]

const FORMATS: readonly [Format, string][] = [
  ["automatic", "General"],
  ["number", "Number"],
  ["currency", "Currency"],
  ["date", "Date"],
  ["time", "Time"],
  ["percent", "Percentage"],
  ["scientific", "Scientific"],
  ["plainText", "Text"],
]

/** Number Format list: the engine's formats, the selection's one checked. */
function NumberFormatMenu() {
  const cmd = useExcelCmd("numberFormat")
  const current = cmd.state.value?.kind
  return (
    <DropBtn
      label="Number Format"
      icon={<NumberSymbol20Regular />}
      menu={
        <Menu label="Number Format">
          {FORMATS.map(([value, label]) => (
            <MenuItem
              key={value}
              label={label}
              checked={current === value}
              cmd={cmd}
              args={{ value }}
            />
          ))}
        </Menu>
      }
    />
  )
}

type Preset = XlsxCommandArgs["borderPreset"]["value"]

const BORDERS: readonly [Preset, string][] = [
  ["bottom", "Bottom Border"],
  ["top", "Top Border"],
  ["left", "Left Border"],
  ["right", "Right Border"],
  ["none", "No Border"],
  ["all", "All Borders"],
  ["outer", "Outside Borders"],
  ["inner", "Inside Borders"],
  ["horizontal", "Inside Horizontal Borders"],
  ["vertical", "Inside Vertical Borders"],
]

/** Borders: main half applies the bottom border, like Excel; the menu lists every preset. */
function BordersSplit() {
  const cmd = useExcelCmd("borderPreset")
  return (
    <SplitBtn
      cmd={{ ...cmd, label: "Borders" }}
      args={{ value: "bottom" }}
      icon={<BorderBottom20Regular />}
      menu={
        <Menu label="Borders">
          {BORDERS.map(([value, label]) => (
            <MenuItem key={value} label={label} cmd={cmd} args={{ value }} />
          ))}
        </Menu>
      }
    />
  )
}

/** Fill / Font Color: main half reapplies the last pick, the menu picks a new one. */
function ColorSplit({
  id,
  start,
  label,
  Icon,
}: {
  id: "fillColor" | "textColor"
  start: string
  label: string
  Icon: FluentIcon
}) {
  const [hex, setHex] = useState(start)
  const cmd = useExcelCmd(id)
  return (
    <SplitBtn
      cmd={cmd}
      args={{ color: `#${hex}` }}
      icon={<Icon color={`#${hex}`} />}
      menu={
        <ColorPicker
          label={label}
          onPick={(h) => {
            if (h === null) return
            setHex(h)
            void cmd.execute({ color: `#${h}` })
          }}
        />
      }
    />
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
      className="excel-formula"
      style={{
        height: EXCEL.formulaH,
        background: "var(--of-chrome)",
        padding: "0 8px",
        display: "flex",
        alignItems: "center",
      }}
    >
      <style href="excel-formula" precedence="default">
        {FORMULA_BOXES}
      </style>
      <EditorToolbar.FormulaBar style={{ flex: 1, fontSize: 14.67, color: "var(--of-ink)" }} />
    </div>
  )
}

/**
 * Excel web's formula row, measured on the reference: three 24px outlined boxes
 * on the chrome (no white fill), name box x8–131, fx box x141–226, formula
 * input from x236. The engine styles them inline, hence `!important`.
 */
const FORMULA_BOXES = `
.excel-formula [aria-label="Formula bar"] { gap: 10px !important; }
.excel-formula [aria-label="Formula bar"] > * { height: 24px !important; box-sizing: border-box; background: transparent !important; border: 1px solid #d1d1d1 !important; border-radius: 4px !important; }
.excel-formula [aria-label="Name box"] { width: 123px !important; flex: none !important; }
.excel-formula [aria-label="Formula bar"] > span { width: 85px !important; flex: none !important; display: flex; align-items: center; justify-content: center; }
.excel-formula [aria-label="Formula bar"] > input:last-child { flex: 1 !important; }
`

/** Status bar: zoom, a live engine command (no fit-to-window in the engine). */
function ExcelStatus() {
  const zoom = useExcelCmd("zoom")
  return (
    <StatusBar
      right={
        // Excel web ends the zoom's + glyph 13px from the edge (the bar pads 8, buttons 8 more).
        <span style={{ display: "flex", alignItems: "center", marginRight: -3 }}>
          <Zoom cmd={zoom} toArgs={(s) => ({ scale: s === "fit" ? 1 : s })} fit={false} compact />
        </span>
      }
    />
  )
}

type NoArgCommand = "bold" | "italic" | "strikethrough" | "undo" | "redo" | "paintFormat"

function Plain({ id, icon }: { id: NoArgCommand; icon: ReactNode }) {
  const cmd = useExcelCmd(id)
  return <Btn cmd={cmd} args={null} icon={icon} />
}

type ValuedId =
  | "horizontalAlignment"
  | "verticalAlignment"
  | "textWrapping"
  | "merge"
  | "numberFormat"
  | "fontSizeStep"
  | "decimalPlaces"

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
