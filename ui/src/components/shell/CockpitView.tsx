import { useState } from "react"
import { LeftRail } from "@/components/shell/LeftRail"
import { PanelPane } from "@/components/panels/cockpit/PanelPane"
import { Conversation } from "@/components/conversation/Conversation"

/**
 * Cockpit (panel-centered) view. Three columns:
 *   LeftRail (context navigator) │ PanelPane (the selected panel — the star) │ Conversation (secondary)
 *
 * The rail's panel selection is lifted here so it drives the center pane: pick
 * a panel on the left, see its bespoke maquette fill the middle. The
 * conversation stays docked on the right as a narrower companion so the agent's
 * dialogue is always in view while inspecting a panel.
 */
export function CockpitView() {
  const [selected, setSelected] = useState("P5")

  return (
    <div className="flex min-h-0 flex-1">
      <LeftRail selected={selected} onSelect={setSelected} />
      <PanelPane panelId={selected} />
      <div className="flex w-[420px] shrink-0 flex-col border-l border-border">
        <Conversation />
      </div>
    </div>
  )
}
