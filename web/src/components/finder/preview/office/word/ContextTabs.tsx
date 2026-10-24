import { useDocxCommand } from "@betteroffice/docx-react"
import type { DocxTableAction } from "@betteroffice/docx-react"
import {
  ArrowRotateClockwise20Regular,
  ArrowRotateCounterclockwise20Regular,
  BorderAll20Regular,
  BorderBottom20Regular,
  BorderInside20Regular,
  BorderLeft20Regular,
  BorderNone20Regular,
  BorderOutside20Regular,
  BorderRight20Regular,
  BorderTop20Regular,
  FlipHorizontal20Regular,
  FlipVertical20Regular,
  Grid20Regular,
  ImageEdit20Regular,
  TableCellsMerge20Regular,
  TableCellsSplit20Regular,
  TableCursor20Regular,
  TableDeleteColumn20Regular,
  TableDeleteRow20Regular,
  TableDismiss20Regular,
  TableInsertColumn20Regular,
  TableInsertRow20Regular,
  TableSettings20Regular,
  TextWrap20Regular,
} from "@fluentui/react-icons"
import type { ReactNode } from "react"
import { RibbonGroup } from "../fluent/controls"
import { DropBtn, Menu, MenuItem } from "../fluent/menus"
import { W } from "./WordBtn"
import { useWordCmd } from "./wordCmd"

/** Word's Picture tab, shown while an image is selected: wrap, rotate, flip, properties. */
export function PictureTab() {
  return (
    <>
      <RibbonGroup label="Arrange">
        <WrapMenu />
      </RibbonGroup>
      <RibbonGroup
        label="Rotate"
        rows={[
          <Transform
            key="r"
            action="rotateCW"
            label="Rotate Right 90°"
            icon={<ArrowRotateClockwise20Regular />}
          />,
          <Transform
            key="l"
            action="rotateCCW"
            label="Rotate Left 90°"
            icon={<ArrowRotateCounterclockwise20Regular />}
          />,
        ]}
      />
      <RibbonGroup
        label="Flip"
        rows={[
          <Transform
            key="h"
            action="flipH"
            label="Flip Horizontal"
            icon={<FlipHorizontal20Regular />}
          />,
          <Transform
            key="v"
            action="flipV"
            label="Flip Vertical"
            icon={<FlipVertical20Regular />}
          />,
        ]}
      />
      <RibbonGroup label="Picture" last>
        <W id="imageProperties" size="lg" icon={<ImageEdit20Regular />} text="Properties" />
      </RibbonGroup>
    </>
  )
}

function Transform({
  action,
  label,
  icon,
}: {
  action: "rotateCW" | "rotateCCW" | "flipH" | "flipV"
  label: string
  icon: ReactNode
}) {
  return <W id="imageTransform" args={{ action }} label={label} icon={icon} />
}

/** Wrap Text: the engine's wrap choices, the current one checked. */
function WrapMenu() {
  const wrap = useDocxCommand("imageWrap")
  const options = wrap.state.options ?? []
  const current = JSON.stringify(wrap.state.value)
  return (
    <DropBtn
      label="Wrap Text"
      icon={<TextWrap20Regular />}
      text="Wrap Text"
      menu={
        <Menu label="Wrap Text">
          {options.map((o) => (
            <MenuItem
              key={o.label}
              label={o.label}
              checked={JSON.stringify(o.args.wrap) === current}
              onSelect={() => void wrap.execute(o.args)}
            />
          ))}
        </Menu>
      }
    />
  )
}

/** Word's Table tab, shown while the selection is in a table. */
export function TableTab() {
  return (
    <>
      <RibbonGroup
        label="Rows & Columns"
        rows={[
          <T key="ra" a="addRowAbove" label="Insert Above" icon={<TableInsertRow20Regular />} />,
          <T key="rb" a="addRowBelow" label="Insert Below" icon={<TableInsertRow20Regular />} />,
        ]}
      />
      <RibbonGroup
        label="Columns"
        rows={[
          <T
            key="cl"
            a="addColumnLeft"
            label="Insert Left"
            icon={<TableInsertColumn20Regular />}
          />,
          <T
            key="cr"
            a="addColumnRight"
            label="Insert Right"
            icon={<TableInsertColumn20Regular />}
          />,
        ]}
      />
      <RibbonGroup label="Delete">
        <TableMenu
          label="Delete"
          icon={<TableDismiss20Regular />}
          items={[
            ["deleteRow", "Delete Rows", <TableDeleteRow20Regular key="r" />],
            ["deleteColumn", "Delete Columns", <TableDeleteColumn20Regular key="c" />],
            ["deleteTable", "Delete Table", <TableDismiss20Regular key="t" />],
          ]}
        />
      </RibbonGroup>
      <RibbonGroup
        label="Merge"
        rows={[
          <T key="m" a="mergeCells" label="Merge Cells" icon={<TableCellsMerge20Regular />} />,
          <T key="s" a="splitCell" label="Split Cells" icon={<TableCellsSplit20Regular />} />,
        ]}
      />
      <RibbonGroup label="Borders">
        <TableMenu label="Borders" icon={<BorderAll20Regular />} items={BORDERS} />
      </RibbonGroup>
      <RibbonGroup label="Table" last>
        <TableMenu
          label="Select"
          icon={<TableCursor20Regular />}
          items={[
            ["selectTable", "Select Table", null],
            ["selectRow", "Select Row", null],
            ["selectColumn", "Select Column", null],
          ]}
        />
        <W id="tableGridlines" label="View Gridlines" icon={<Grid20Regular />} />
        <T
          a={{ type: "openTableProperties" }}
          label="Properties"
          icon={<TableSettings20Regular />}
        />
      </RibbonGroup>
    </>
  )
}

/** One table action as a ribbon button. */
function T({ a, label, icon }: { a: DocxTableAction; label: string; icon: ReactNode }) {
  return <W id="tableAction" args={a} label={label} icon={icon} />
}

type TableItem = readonly [DocxTableAction, string, ReactNode]

const BORDERS: readonly TableItem[] = [
  ["borderAll", "All Borders", <BorderAll20Regular key="a" />],
  ["borderOutside", "Outside Borders", <BorderOutside20Regular key="o" />],
  ["borderInside", "Inside Borders", <BorderInside20Regular key="i" />],
  ["borderNone", "No Border", <BorderNone20Regular key="n" />],
  ["borderTop", "Top Border", <BorderTop20Regular key="t" />],
  ["borderBottom", "Bottom Border", <BorderBottom20Regular key="b" />],
  ["borderLeft", "Left Border", <BorderLeft20Regular key="l" />],
  ["borderRight", "Right Border", <BorderRight20Regular key="r" />],
]

/** A menu of table actions (Delete, Borders, Select). */
function TableMenu({
  label,
  icon,
  items,
}: {
  label: string
  icon: ReactNode
  items: readonly TableItem[]
}) {
  const cmd = useWordCmd("tableAction")
  return (
    <DropBtn
      label={label}
      icon={icon}
      text={label}
      menu={
        <Menu label={label}>
          {items.map(([a, text, ic]) => (
            <MenuItem key={text} label={text} icon={ic} cmd={cmd} args={a} />
          ))}
        </Menu>
      }
    />
  )
}
