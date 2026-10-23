import { SlideAdd20Regular } from "@fluentui/react-icons"
import { usePptxCommand } from "@betteroffice/pptx-react"
import { Gallery, Menu, SplitBtn } from "../fluent/menus"
import { usePptCmd } from "./pptCmd"

/**
 * New Slide split button: the main half adds a slide with the current
 * slide's layout, the chevron opens the deck's layouts as a gallery. Layouts
 * come from the engine's `insertSlide` options, so the list is the real deck's.
 */
export function NewSlide() {
  const cmd = usePptCmd("insertSlide", {})
  const { options = [] } = usePptxCommand("insertSlide").state
  const layouts = options.flatMap((o) => {
    const path = o.args.layoutPartPath
    return typeof path === "string" && o.state.enabled
      ? [{ value: path, label: o.label, preview: <LayoutTile label={o.label} /> }]
      : []
  })
  return (
    <SplitBtn
      cmd={cmd}
      args={{}}
      icon={<SlideAdd20Regular />}
      menu={
        <Menu label="Slide layouts">
          <Gallery
            label="Slide layouts"
            items={layouts}
            value={cmd.state.value}
            onPick={(layoutPartPath) => void cmd.execute({ layoutPartPath })}
            columns={3}
          />
        </Menu>
      }
    />
  )
}

/** Text tile standing in for a layout thumbnail (the engine exposes no layout preview). */
function LayoutTile({ label }: { label: string }) {
  return (
    <span
      style={{
        width: 112,
        height: 63,
        display: "flex",
        alignItems: "flex-end",
        padding: 6,
        boxSizing: "border-box",
        border: "1px solid var(--of-line)",
        borderRadius: 4,
        fontSize: 11,
        color: "var(--of-ink-muted)",
        textAlign: "left",
        overflow: "hidden",
      }}
    >
      {label}
    </span>
  )
}
