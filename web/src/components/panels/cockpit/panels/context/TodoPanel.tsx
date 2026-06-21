import { ListTodo, Circle, CircleDot, CheckCircle2 } from "lucide-react"
import type { ContextPanel, TodoItem } from "@/lib/types"
import { useTodos } from "@/lib/live"
import { PanelFrame } from "../../PanelFrame"

const STATUS_META: Record<TodoItem["status"], { icon: typeof Circle; color: string }> = {
  pending: { icon: Circle, color: "var(--muted-foreground)" },
  in_progress: { icon: CircleDot, color: "var(--signal)" },
  done: { icon: CheckCircle2, color: "var(--ok)" },
}

/**
 * Todo List panel — the agent's hierarchical task plan. Rows are
 * depth-indented; status drives a colored glyph (○ pending · ◉ in-progress ·
 * ✓ done) and strikes through completed items. A header progress bar reports
 * overall completion.
 */
export function TodoPanel({ panel, agentId }: { panel: ContextPanel; agentId: string }) {
  const { data: todoItems = [] } = useTodos(agentId)
  const done = todoItems.filter((t) => t.status === "done").length
  const ratio = todoItems.length > 0 ? done / todoItems.length : 0

  return (
    <PanelFrame
      icon={ListTodo}
      name="Todo List"
      subtitle={`${done} of ${todoItems.length} done in view · 142 / 145 total`}
      tokens={panel.tokens}
      cost={panel.costUsd}
    >
      <div className="mb-4">
        <div className="h-1.5 overflow-hidden rounded-full bg-muted">
          <div
            className="h-full rounded-full fill-sweep"
            style={{ width: `${ratio * 100}%`, background: "var(--ok)" }}
          />
        </div>
      </div>

      <ul className="flex flex-col gap-0.5">
        {todoItems.map((t) => {
          const { icon: Icon, color } = STATUS_META[t.status]
          return (
            <li
              key={t.id}
              className="flex items-start gap-2.5 rounded-md px-2 py-1.5 hover:bg-muted/50"
              style={{ paddingLeft: `${t.depth * 18 + 8}px` }}
            >
              <Icon className="mt-0.5 size-4 shrink-0" style={{ color }} />
              <span className="shrink-0 font-mono text-[10px] tabular-nums text-muted-foreground/60">
                {t.id}
              </span>
              <span
                className="text-[12.5px] leading-snug text-foreground/85"
                style={t.status === "done" ? { textDecoration: "line-through", opacity: 0.55 } : undefined}
              >
                {t.name}
              </span>
            </li>
          )
        })}
      </ul>
    </PanelFrame>
  )
}
