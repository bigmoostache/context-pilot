import { useMemo } from "react"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Message } from "./Message"
import { InputBar } from "./InputBar"
import { useConversation, useStreamingTokens, type LiveTokens } from "@/lib/live"
import type { ConversationMsg } from "@/lib/api"
import type { ChatMessage } from "@/lib/types"

// ── Cockpit conversation surface — LIVE (§7) ─────────────────────────
//
// Renders the agent's own conversation from the durable inspection plane
// (`useConversation`) and overlays the ephemeral stream plane
// (`useStreamingTokens`) so assistant text paints **as it is typed**.
//
// Reconciliation model:
//   • The durable conversation is authoritative. Each message is keyed by its
//     stable `Message::id`.
//   • While a message streams, the live token buffer (keyed by the same id)
//     holds more text than the not-yet-flushed durable record — so we show the
//     longer of the two and a blinking cursor while the live buffer leads.
//   • A streaming message that has no durable record yet (the agent is typing a
//     brand-new assistant turn that hasn't been flushed to disk) is rendered as
//     a synthetic trailing bubble so the user sees the very first tokens live.

/** Relative-time label for a message timestamp (epoch ms). */
function ago(ms: number): string {
  const s = Math.max(0, Math.floor((Date.now() - ms) / 1000))
  if (s < 5) return "just now"
  if (s < 60) return `${s}s ago`
  const m = Math.floor(s / 60)
  if (m < 60) return `${m}m ago`
  const h = Math.floor(m / 60)
  if (h < 24) return `${h}h ago`
  return `${Math.floor(h / 24)}d ago`
}

/** Stringify a tool-input value map to display strings. */
function toParams(input: Record<string, unknown> | undefined): Record<string, string> {
  const out: Record<string, string> = {}
  if (!input) return out
  for (const [k, v] of Object.entries(input)) {
    out[k] = typeof v === "string" ? v : JSON.stringify(v)
  }
  return out
}

/**
 * Map durable conversation messages → renderer `ChatMessage`s, then overlay the
 * live token buffers. `tool_result` rows are folded into their matching
 * `tool_call` card rather than rendered as empty user bubbles.
 */
function buildMessages(durable: ConversationMsg[], live: LiveTokens): ChatMessage[] {
  // Index tool results by the tool name so a tool_call can attach its result.
  const out: ChatMessage[] = []
  const renderedIds = new Set<string>()

  for (const m of durable) {
    const kind = m.message_type ?? "text"
    if (kind === "tool_result") continue // folded into its tool_call card

    if (kind === "tool_call") {
      // Generated `tool_uses` is `Array<{ [key: string]: unknown }>` (the
      // OpenAPI spec can't express the per-tool shape), so name it the shape we
      // actually read off it.
      const use = m.tool_uses?.[0] as { name?: string; input?: Record<string, unknown> } | undefined
      if (!use) continue
      out.push({
        id: m.id,
        role: "tool",
        ts: ago(m.timestamp_ms),
        tool: {
          name: use.name ?? "tool",
          intent: (use.input?.["intent"] as string | undefined) ?? "",
          verb: (use.input?.["verb"] as string | undefined) ?? "",
          params: toParams(use.input),
        },
      })
      renderedIds.add(m.id)
      continue
    }

    // Plain user / assistant text. Overlay live buffer when it leads.
    const liveText = live[m.id]
    const text = liveText != null && liveText.length > m.content.length ? liveText : m.content
    const streaming = liveText != null && liveText.length > m.content.length
    out.push({
      id: m.id,
      role: m.role === "assistant" ? "assistant" : "user",
      text,
      ts: ago(m.timestamp_ms),
      streaming,
    })
    renderedIds.add(m.id)
  }

  // Synthetic trailing bubbles for streaming messages with no durable record
  // yet (first tokens of a brand-new assistant turn).
  for (const [id, text] of Object.entries(live)) {
    if (renderedIds.has(id) || !text) continue
    out.push({ id, role: "assistant", text, ts: "now", streaming: true })
  }

  return out
}

export function Conversation({ agentId }: { agentId: string }) {
  const { data: durable = [], loading } = useConversation(agentId)
  const live = useStreamingTokens(agentId)
  const messages = useMemo(() => buildMessages(durable, live), [durable, live])
  const isStreaming = Object.keys(live).length > 0

  return (
    <main className="rise flex min-w-0 flex-1 flex-col bg-background">
      {/* header strip */}
      <div className="flex h-11 shrink-0 items-center gap-2.5 border-b border-border px-5">
        <span className="text-[13px] font-semibold text-foreground/90">Conversation</span>
        <span className="text-[11.5px] text-muted-foreground">
          {messages.length} message{messages.length === 1 ? "" : "s"}
        </span>
        {isStreaming && (
          <div className="ml-auto flex items-center gap-1.5">
            <span className="size-1.5 animate-pulse rounded-full bg-[var(--signal)]" />
            <span className="text-[11px] text-muted-foreground">Streaming</span>
          </div>
        )}
      </div>

      <ScrollArea className="min-h-0 flex-1">
        <div className="mx-auto flex max-w-[760px] flex-col px-5 py-4">
          {loading && messages.length === 0 ? (
            <div className="py-8 text-center text-[12px] text-muted-foreground">
              Loading conversation…
            </div>
          ) : (
            messages.map((m) => <Message key={m.id} msg={m} agentId={agentId} />)
          )}
        </div>
      </ScrollArea>

      <div className="mx-auto w-full max-w-[760px]">
        <InputBar />
      </div>
    </main>
  )
}
