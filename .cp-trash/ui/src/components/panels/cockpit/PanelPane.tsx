import type { ContextPanel, PanelKind } from "@/lib/types"
import { panels } from "@/lib/mock"
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
 * mock registry and renders its bespoke maquette by kind. The center "star" of
 * the panel-centered cockpit view. The special id "conversation" renders the
 * full conversation surface (the conversation is treated as just another panel,
 * T24). Unknown / not-yet-designed kinds fall back to a graceful placeholder
 * inside the shared frame.
 */
export function PanelPane({ panelId }: { panelId: string }) {
  if (panelId === "conversation") return <Conversation />
  const panel = panels.find((p) => p.id === panelId) ?? panels[0]
  return renderPanel(panel)
}

function renderPanel(panel: ContextPanel) {
  switch (panel.kind) {
    case "tree":
      return <TreePanel panel={panel} />
    case "memory":
      return <MemoryPanel panel={panel} />
    case "radar":
      return <RadarPanel panel={panel} />
    case "todo":
      return <TodoPanel panel={panel} />
    case "threads":
      return <ThreadsPanel panel={panel} />
    case "stats":
      return <StatsPanel panel={panel} />
    case "tools":
      return <ToolsPanel panel={panel} />
    case "entities":
      return <EntitiesPanel panel={panel} />
    case "spine":
      return <SpinePanel panel={panel} />
    case "callback":
      return <CallbacksPanel panel={panel} />
    case "queue":
      return <QueuePanel panel={panel} />
    case "scratchpad":
      return <ScratchpadPanel panel={panel} />
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
