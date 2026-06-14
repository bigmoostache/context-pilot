import { useState } from "react"
import { ThreadList } from "./ThreadList"
import { ThreadConversation } from "./ThreadConversation"
import { ThreadDetailRail } from "./ThreadDetailRail"
import { threadDetails } from "@/lib/mock"

/**
 * Thread-centered view — the "classic" three-pane messaging layout:
 * thread list (left) | conversation (center) | thread detail (right).
 * Complements the panel-centered cockpit; switch via the TopBar.
 */
export function ThreadsView({ onOpenCockpit }: { onOpenCockpit: () => void }) {
  const [selectedId, setSelectedId] = useState(threadDetails[0]?.id ?? "")
  const thread = threadDetails.find((t) => t.id === selectedId) ?? threadDetails[0]

  return (
    <div className="flex min-h-0 flex-1">
      <ThreadList threads={threadDetails} selectedId={selectedId} onSelect={setSelectedId} />
      {thread && <ThreadConversation thread={thread} />}
      {thread && <ThreadDetailRail thread={thread} onOpenCockpit={onOpenCockpit} />}
    </div>
  )
}
