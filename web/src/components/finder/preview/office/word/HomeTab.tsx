import { Fragment, useState } from "react"
import { useDocxCommand } from "@betteroffice/docx-react"
import {
  ArrowRedo20Regular,
  ArrowSwap20Regular,
  ArrowUndo20Regular,
  FontDecrease20Regular,
  FontIncrease20Regular,
  Highlight20Regular,
  Search20Regular,
  TextAlignCenter20Regular,
  TextAlignJustify20Regular,
  TextAlignLeft20Regular,
  TextAlignRight20Regular,
  TextBold20Regular,
  TextBulletListLtr20Regular,
  TextClearFormatting20Regular,
  TextColor20Regular,
  TextDirectionHorizontalLtr20Regular,
  TextDirectionHorizontalRtl20Regular,
  TextIndentDecreaseLtr20Regular,
  TextIndentIncreaseLtr20Regular,
  TextItalic20Regular,
  TextLineSpacing20Regular,
  TextNumberListLtr20Regular,
  TextStrikethrough20Regular,
  TextSubscript20Regular,
  TextSuperscript20Regular,
  TextUnderline20Regular,
} from "@fluentui/react-icons"
import { Btn, Combo, RibbonGroup } from "../fluent/controls"
import { ColorPicker, DropBtn, Menu, MenuItem, SplitBtn } from "../fluent/menus"
import { W } from "./WordBtn"
import { FONT_SIZES, useFontStep, useWordCmd } from "./wordCmd"

const FONTS = [
  "Aptos",
  "Arial",
  "Calibri",
  "Cambria",
  "Courier New",
  "Georgia",
  "Times New Roman",
  "Verdana",
] as const

const LINE_SPACINGS = [1, 1.15, 1.5, 2, 2.5, 3] as const

/** Word's Home tab: Undo, Font, Paragraph, Styles, Editing. Every control is a live command. */
export function HomeTab() {
  return (
    <>
      <RibbonGroup
        label="Undo"
        rows={[
          <W key="u" id="undo" icon={<ArrowUndo20Regular />} />,
          <W key="r" id="redo" icon={<ArrowRedo20Regular />} />,
        ]}
      />
      <FontGroup />
      <ParagraphGroup />
      <StylesGroup />
      <RibbonGroup
        label="Editing"
        last
        rows={[
          <W key="f" id="find" icon={<Search20Regular />} text="Find" />,
          <W key="r" id="replace" icon={<ArrowSwap20Regular />} text="Replace" />,
        ]}
      />
    </>
  )
}

function FontGroup() {
  const family = useWordCmd("fontFamily")
  const size = useWordCmd("fontSize")
  const grow = useFontStep(1)
  const shrink = useFontStep(-1)
  return (
    <RibbonGroup
      label="Font"
      rows={[
        <Fragment key="r81">
          <Combo
            label="Font"
            width={128}
            options={FONTS}
            cmd={family}
            toArgs={(t) => ({ family: t })}
          />
          <Combo
            label="Font Size"
            width={56}
            options={FONT_SIZES}
            cmd={size}
            toArgs={(t) => ({ points: Math.min(1638, Math.max(1, Number(t) || 11)) })}
          />
          <Btn cmd={grow} icon={<FontIncrease20Regular />} />
          <Btn cmd={shrink} icon={<FontDecrease20Regular />} />
          <W id="clearFormatting" icon={<TextClearFormatting20Regular />} />
        </Fragment>,
        <Fragment key="r100">
          <W id="bold" icon={<TextBold20Regular />} />
          <W id="italic" icon={<TextItalic20Regular />} />
          <W id="underline" icon={<TextUnderline20Regular />} />
          <W id="strikethrough" icon={<TextStrikethrough20Regular />} />
          <W id="subscript" icon={<TextSubscript20Regular />} />
          <W id="superscript" icon={<TextSuperscript20Regular />} />
          <HighlightSplit />
          <FontColorSplit />
        </Fragment>,
      ]}
    />
  )
}

function ParagraphGroup() {
  const spacing = useWordCmd("lineSpacing")
  const twips = spacing.state.value
  const lines = twips === undefined ? undefined : Math.round((twips / 240) * 100) / 100
  return (
    <RibbonGroup
      label="Paragraph"
      rows={[
        <Fragment key="r130">
          <W id="bulletList" icon={<TextBulletListLtr20Regular />} />
          <W id="numberedList" icon={<TextNumberListLtr20Regular />} />
          <W id="outdent" icon={<TextIndentDecreaseLtr20Regular />} />
          <W id="indent" icon={<TextIndentIncreaseLtr20Regular />} />
          <W id="setLtr" icon={<TextDirectionHorizontalLtr20Regular />} />
          <W id="setRtl" icon={<TextDirectionHorizontalRtl20Regular />} />
        </Fragment>,
        <Fragment key="r138">
          <Align value="left" icon={<TextAlignLeft20Regular />} />
          <Align value="center" icon={<TextAlignCenter20Regular />} />
          <Align value="right" icon={<TextAlignRight20Regular />} />
          <Align value="both" icon={<TextAlignJustify20Regular />} />
          <DropBtn
            label="Line and Paragraph Spacing"
            icon={<TextLineSpacing20Regular />}
            menu={
              <Menu label="Line Spacing">
                {LINE_SPACINGS.map((l) => (
                  <MenuItem
                    key={l}
                    label={String(l)}
                    cmd={spacing}
                    args={{ value: Math.round(l * 240) }}
                    checked={lines === l}
                  />
                ))}
              </Menu>
            }
          />
        </Fragment>,
      ]}
    />
  )
}

function Align({
  value,
  icon,
}: {
  value: "left" | "center" | "right" | "both"
  icon: React.ReactNode
}) {
  return <W id="alignment" args={{ value }} pressedValue={value} icon={icon} />
}

/** Word's highlight names, shown as swatches (the engine takes the name). */
const HIGHLIGHTS: Record<string, string> = {
  FFFF00: "yellow",
  "00FF00": "green",
  "00FFFF": "cyan",
  FF00FF: "magenta",
  "0000FF": "blue",
  FF0000: "red",
  "000080": "darkBlue",
  "008080": "darkCyan",
  "008000": "darkGreen",
  "800080": "darkMagenta",
  "800000": "darkRed",
  "808000": "darkYellow",
  "808080": "darkGray",
  C0C0C0: "lightGray",
  "000000": "black",
}

/** Text Highlight Color: main half reapplies the last pick (yellow first). */
function HighlightSplit() {
  const [hex, setHex] = useState("FFFF00")
  const cmd = useWordCmd("highlightColor")
  const keys = Object.keys(HIGHLIGHTS)
  return (
    <SplitBtn
      cmd={cmd}
      args={{ color: HIGHLIGHTS[hex] ?? "yellow" }}
      icon={<Highlight20Regular color={`#${hex}`} />}
      menu={
        <ColorPicker
          label="Highlight colours"
          colors={[keys.slice(0, 8), keys.slice(8)]}
          noneLabel="No Color"
          onPick={(h) => {
            if (h === null) {
              void cmd.execute({ color: "none" })
              return
            }
            setHex(h)
            void cmd.execute({ color: HIGHLIGHTS[h] ?? "yellow" })
          }}
        />
      }
    />
  )
}

/** Font Color: main half reapplies the last pick (dark red first). */
function FontColorSplit() {
  const [hex, setHex] = useState("C00000")
  const cmd = useWordCmd("textColor")
  return (
    <SplitBtn
      cmd={cmd}
      args={{ color: { rgb: hex } }}
      icon={<TextColor20Regular color={`#${hex}`} />}
      menu={
        <ColorPicker
          label="Font colours"
          noneLabel="Automatic"
          onPick={(h) => {
            if (h === null) {
              void cmd.execute({ color: "auto" })
              return
            }
            setHex(h)
            void cmd.execute({ color: { rgb: h } })
          }}
        />
      }
    />
  )
}

/** Paragraph style box: the engine's style list, shown and typed by name. */
function StylesGroup() {
  const style = useDocxCommand("paragraphStyle")
  const options = style.state.options ?? []
  const current = options.find((o) => o.args.styleId === style.state.value)
  return (
    <RibbonGroup label="Styles">
      <Combo
        label="Styles"
        width={150}
        options={options.map((o) => o.label)}
        value={current?.label ?? style.state.value ?? ""}
        onCommit={(t) => {
          const hit = options.find(
            (o) => o.label.toLowerCase() === t.toLowerCase() || o.args.styleId === t,
          )
          if (hit) void style.execute(hit.args)
        }}
      />
    </RibbonGroup>
  )
}
