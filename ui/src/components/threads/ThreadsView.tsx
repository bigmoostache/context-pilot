import { useState } from "react"
import { FolderGit2, PanelLeft } from "lucide-react"
import { ThreadList } from "./ThreadList"
import { ThreadConversation } from "./ThreadConversation"
import { Button } from "@/components/ui/button"
import { threadDetails, agents } from "@/lib/mock"

/**
 * Thread-centered view — the conversation-first layout: thread list (left) |
 * conversation (center). Per-thread metadata used to occupy a permanent right
 * rail; it now lives in an on-demand popup (the ⓘ button in the conversation
 * header), keeping the messaging surface wide.
 *
 * Scoped to the **active agent's realm**: an agent lives in its folder and owns
 * the threads inside it, so we only ever show that agent's threads — never a
 * cross-agent global list. Complements the panel-centered cockpit.
 */
export function ThreadsView({
  activeAgentId,
}: {
  activeAgentId: string
}) {
  const agent = agents.find((a) => a.id === activeAgentId)
  const realmThreads = threadDetails.filter((t) => t.agentId === activeAgentId)

  const [selectedId, setSelectedId] = useState(realmThreads[0]?.id ?? "")
  const [collapsed, setCollapsed] = useState(false)
  // Keep selection valid when the active agent (realm) changes.
  const thread =
    realmThreads.find((t) => t.id === selectedId) ?? realmThreads[0]

  if (!agent || realmThreads.length === 0) {
    return <EmptyRealm agentName={agent?.name} />
  }

  return (
    <div className="relative flex min-h-0 flex-1">
      <ThreadList
        threads={realmThreads}
        selectedId={thread?.id ?? ""}
        onSelect={setSelectedId}
        collapsed={collapsed}
        onToggleCollapse={() => setCollapsed((v) => !v)}
      />
      {collapsed && (
        <Button
          variant="ghost"
          size="icon-sm"
          onClick={() => setCollapsed(false)}
          title="Show threads"
          className="absolute left-2 top-2 z-10 border border-border bg-card text-muted-foreground card-shadow"
        >
          <PanelLeft className="size-4" />
        </Button>
      )}
      {thread && <ThreadConversation thread={thread} />}
    </div>
  )
}

/** Shown when the active agent's realm holds no threads yet. */
function EmptyRealm({ agentName }: { agentName?: string }) {
  return (
    <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-3 bg-background text-center">
      <span className="flex size-12 items-center justify-center rounded-2xl bg-muted text-muted-foreground/60">
        <FolderGit2 className="size-6" />
      </span>
      <p className="max-w-[320px] text-[13px] text-muted-foreground">
        {agentName ? (
          <>
            <span className="font-medium text-foreground/80">{agentName}</span> has no
            threads yet — start one to put it to work in its folder.
          </>
        ) : (
          "Select an agent to see its threads."
        )}
      </p>
    </div>
  )
}
