import { useState } from "react"
import { LeftRail } from "@/components/shell/LeftRail"
import { PanelPane } from "@/components/panels/cockpit/PanelPane"

/**
 * Cockpit (panel-centered) view. Two columns:
 *   LeftRail (context navigator) │ PanelPane (the selected surface — the star)
 *
 * The conversation is no longer a docked side column: it is treated as **just
 * another panel** (T24). The rail lists a dedicated "Conversation" entry above
 * the panel list; selecting it renders the full conversation in the center
 * pane, exactly where any panel maquette would appear. Selection is lifted here
 * so the rail drives the center pane, and the conversation is the default view.
 */
export function CockpitView() {
  const [selected, setSelected] = useState("conversation")

  return (
    <div className="flex min-h-0 flex-1">
      <LeftRail selected={selected} onSelect={setSelected} />
      <PanelPane panelId={selected} />
    </div>
  )
}
