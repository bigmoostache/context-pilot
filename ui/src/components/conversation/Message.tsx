import { ChevronDown, Terminal, User } from "lucide-react"
import type { ChatMessage } from "@/lib/types"
import { renderInline } from "@/lib/inline"
import { cn } from "@/lib/utils"

export function Message({ msg }: { msg: ChatMessage }) {
  if (msg.role === "tool" && msg.tool) return <ToolMessage msg={msg} />
  if (msg.role === "user") return <UserMessage msg={msg} />
  return <AssistantMessage msg={msg} />
}

function UserMessage({ msg }: { msg: ChatMessage }) {
  return (
    <div className="rise flex flex-col items-end gap-1 py-2">
      <div className="max-w-[78%] rounded-2xl rounded-br-md bg-[var(--signal)] px-3.5 py-2 text-[13px] leading-relaxed text-[var(--primary-foreground)] card-shadow">
        {renderInline(msg.text ?? "")}
      </div>
      <span className="flex items-center gap-1 pr-1 text-[10px] text-muted-foreground/60">
        <User className="size-2.5" />
        {msg.ts}
      </span>
    </div>
  )
}

function AssistantMessage({ msg }: { msg: ChatMessage }) {
  return (
    <div className="rise flex flex-col gap-1.5 py-2">
      <div className="flex items-center gap-2">
        <span className="flex size-5 items-center justify-center rounded-full bg-[var(--signal)]/15">
          <span className="size-2 rounded-full bg-[var(--signal)]" />
        </span>
        <span className="text-[12px] font-semibold text-foreground/85">Context Pilot</span>
        <span className="text-[10px] text-muted-foreground/60">{msg.ts}</span>
      </div>
      <div className="max-w-[88%] pl-7 text-[13.5px] leading-relaxed text-foreground/90">
        {renderInline(msg.text ?? "")}
        {msg.streaming && (
          <span className="cursor-blink ml-0.5 inline-block h-3.5 w-[7px] translate-y-0.5 bg-[var(--signal)]" />
        )}
      </div>
    </div>
  )
}

function ToolMessage({ msg }: { msg: ChatMessage }) {
  const t = msg.tool!
  return (
    <div className="rise py-2 pl-7">
      <div
        className={cn(
          "max-w-[88%] overflow-hidden rounded-xl border bg-card card-shadow",
          t.isError ? "border-[var(--danger)]/50" : "border-border",
        )}
      >
        {/* header */}
        <div className="flex items-center gap-2 border-b border-border bg-muted/50 px-3 py-1.5">
          <Terminal className="size-3.5 text-[var(--interactive)]" />
          <span className="text-[12px] font-semibold text-foreground/90">{t.name}</span>
          <span className="truncate text-[11px] text-muted-foreground">{t.intent}</span>
          <ChevronDown className="ml-auto size-3.5 text-muted-foreground/50" />
        </div>
        {/* params */}
        <div className="px-3 py-2">
          {Object.entries(t.params).map(([k, v]) => (
            <div key={k} className="flex gap-2 font-mono text-[11px] leading-relaxed">
              <span className="shrink-0 text-muted-foreground/70">{k}</span>
              <span className="truncate text-foreground/75">{v}</span>
            </div>
          ))}
          {t.result && (
            <pre
              className={cn(
                "mt-2 overflow-x-auto whitespace-pre-wrap rounded-md bg-muted/60 px-2.5 py-1.5 font-mono text-[10.5px] leading-relaxed",
                t.isError ? "text-[var(--danger)]" : "text-muted-foreground",
              )}
            >
              {t.result}
            </pre>
          )}
        </div>
      </div>
    </div>
  )
}
