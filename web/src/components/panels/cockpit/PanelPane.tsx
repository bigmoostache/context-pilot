import type { ContextPanel, PanelKind } from "@/lib/types"
import { usePanels } from "@/lib/live"
import { panelIcon } from "@/lib/panelMeta"
import { Conversation } from "@/components/conversation/Conversation"
import { PanelFrame } from "./PanelFrame"
import { TreePanel } from "./TreePanel"
import { MemoryPanel } from "./MemoryPanel"
import { RadarPanel } from "./RadarPanel"
import { TodoPanel } from "./TodoPanel"
import { ThreadsPanel } from "./ThreadsPanel"
import { StatsPanel } from "./StatsPanel"
import { ToolsPanel } from "./ToolsPanel"
import { EntitiesPanel } from "./EntitiesPanel"
import { SpinePanel } from "./SpinePanel"
import { CallbacksPanel } from "./CallbacksPanel"
import { QueuePanel } from "./QueuePanel"
import { ScratchpadPanel } from "./ScratchpadPanel"

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
  if (panelId === "conversation") return <Conversation />
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
    case "threads":
      return <ThreadsPanel panel={panel} agentId={agentId} />
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
