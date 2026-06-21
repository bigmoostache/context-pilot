import type { ContextPanel, PanelKind } from "@/lib/types"
import { usePanels } from "@/lib/live"
import { panelIcon } from "@/lib/support/panelMeta"
import { Conversation } from "@/components/conversation/Conversation"
import { PanelFrame } from "./PanelFrame"
import { TreePanel } from "./panels/context/TreePanel"
import { MemoryPanel } from "./panels/context/MemoryPanel"
import { RadarPanel } from "./panels/context/RadarPanel"
import { TodoPanel } from "./panels/context/TodoPanel"
import { StatsPanel } from "./panels/context/StatsPanel"
import { ToolsPanel } from "./panels/modules/ToolsPanel"
import { EntitiesPanel } from "./panels/modules/EntitiesPanel"
import { SpinePanel } from "./panels/modules/SpinePanel"
import { CallbacksPanel } from "./panels/modules/CallbacksPanel"
import { QueuePanel } from "./panels/modules/QueuePanel"
import { ScratchpadPanel } from "./panels/modules/ScratchpadPanel"

/**
 * Cockpit panel router — given the selected panel id, looks the panel up in the
 * live panel registry and renders its bespoke view by kind. The center "star" of
 * the panel-centered cockpit view. The special id "conversation" renders the
 * full conversation surface (the conversation is treated as just another panel,
 * T24). Unknown / not-yet-designed kinds fall back to a graceful placeholder
 * inside the shared frame.
 */
export function PanelPane({ agentId, panelId }: { agentId: string; panelId: string }) {
  const { data: panels = [] } = usePanels(agentId)
  if (panelId === "conversation") return <Conversation agentId={agentId} />
  const panel = panels.find((p) => p.id === panelId) ?? panels[0]
  if (!panel) return <EmptyState />
  return renderPanel(panel, agentId)
}

function EmptyState() {
  return (
    <div className="flex flex-1 items-center justify-center text-[12.5px] text-muted-foreground/60">
      No panels available
    </div>
  )
}

function renderPanel(panel: ContextPanel, agentId: string) {
  switch (panel.kind) {
    case "tree":
      return <TreePanel panel={panel} agentId={agentId} />
    case "memory":
      return <MemoryPanel panel={panel} agentId={agentId} />
    case "radar":
      return <RadarPanel panel={panel} agentId={agentId} />
    case "todo":
      return <TodoPanel panel={panel} agentId={agentId} />
    case "stats":
      return <StatsPanel panel={panel} agentId={agentId} />
    case "tools":
      return <ToolsPanel panel={panel} agentId={agentId} />
    case "entities":
      return <EntitiesPanel panel={panel} agentId={agentId} />
    case "spine":
      return <SpinePanel panel={panel} agentId={agentId} />
    case "callback":
      return <CallbacksPanel panel={panel} agentId={agentId} />
    case "queue":
      return <QueuePanel panel={panel} agentId={agentId} />
    case "scratchpad":
      return <ScratchpadPanel panel={panel} agentId={agentId} />
    default:
      return <Placeholder kind={panel.kind} name={panel.name} />
  }
}

function Placeholder({ kind, name }: { kind: PanelKind; name: string }) {
  const Icon = panelIcon[kind]
  return (
    <PanelFrame icon={Icon} name={name}>
      <div className="rounded-lg border border-dashed border-border py-16 text-center text-[12.5px] text-muted-foreground/60">
        No view for “{kind}” panels yet.
      </div>
    </PanelFrame>
  )
}
