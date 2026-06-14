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
    <div className="rise flex flex-col items-end gap-1 py-1.5">
      <div className="flex items-center gap-1.5 pr-0.5">
        <span className="label">you</span>
        <User className="size-3 text-muted-foreground" />
      </div>
      <div className="max-w-[78%] rounded-[4px] rounded-tr-[1px] border border-border bg-[oklch(0.21_0.008_75)] px-3 py-1.5 text-[12.5px] leading-relaxed text-foreground/90">
        {renderInline(msg.text ?? "")}
      </div>
      <span className="pr-0.5 text-[9px] tabular-nums text-muted-foreground/50">{msg.ts}</span>
    </div>
  )
}

function AssistantMessage({ msg }: { msg: ChatMessage }) {
  return (
    <div className="rise flex flex-col gap-1 py-1.5">
      <div className="flex items-center gap-1.5">
        <span className="flex size-4 items-center justify-center rounded-[3px] bg-[var(--signal)]/15 text-[var(--signal)]">
          <span className="size-1.5 rounded-full bg-[var(--signal)] shadow-[0_0_5px_var(--signal)]" />
        </span>
        <span className="label glow-signal" style={{ color: "var(--signal)" }}>
          context·pilot
        </span>
        <span className="text-[9px] tabular-nums text-muted-foreground/50">{msg.ts}</span>
      </div>
      <div className="max-w-[88%] pl-1 font-sans text-[13px] leading-relaxed text-foreground/85">
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
    <div className="rise py-1.5">
      <div
        className={cn(
          "max-w-[88%] overflow-hidden rounded-[4px] border bg-[oklch(0.185_0.007_75)]",
          t.isError ? "border-[var(--danger)]/50" : "border-border",
        )}
      >
        {/* header */}
        <div className="flex items-center gap-2 border-b border-border/70 bg-[oklch(0.205_0.008_75)] px-2.5 py-1">
          <Terminal className="size-3 text-[var(--interactive)]" />
          <span className="text-[11.5px] font-semibold text-foreground/90">{t.name}</span>
          <span className="rounded-[2px] bg-[var(--interactive)]/12 px-1 text-[9px] uppercase tracking-wider text-[var(--interactive)]">
            {t.verb}
          </span>
          <span className="truncate text-[10px] text-muted-foreground">{t.intent}</span>
          <ChevronDown className="ml-auto size-3 text-muted-foreground/50" />
        </div>
        {/* params */}
        <div className="px-2.5 py-1.5">
          {Object.entries(t.params).map(([k, v]) => (
            <div key={k} className="flex gap-2 text-[11px] leading-relaxed">
              <span className="shrink-0 text-muted-foreground/70">{k}</span>
              <span className="truncate text-[var(--signal-dim)]">{v}</span>
            </div>
          ))}
          {t.result && (
            <pre
              className={cn(
                "mt-1.5 overflow-x-auto whitespace-pre-wrap rounded-[2px] border-l-2 pl-2 text-[10.5px] leading-relaxed",
                t.isError
                  ? "border-[var(--danger)] text-[var(--danger)]/90"
                  : "border-[var(--grid)] text-muted-foreground",
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
