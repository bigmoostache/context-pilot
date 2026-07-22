import { Loader2 } from "lucide-react"
import { useToolCall } from "@/lib/live"
import type { ToolCallDetail } from "@/lib/api"
import { cn } from "@/lib/utils"

/**
 * The adaptive detail bubble for one auto tool-activity trace (T584).
 *
 * An auto trace carries only its `toolRef` content-hash; this component mounts
 * when the user expands the row, fetches the full tool-call record ON CLICK via
 * {@link useToolCall} (never prefetched — the params + potentially large result
 * stay off the thread-list payload), then dispatches on the tool `name` to a
 * shape-appropriate renderer: a red/green diff for `Edit`, a terminal block for
 * `console_*`, a query+result view for `search`, and a generic
 * key/value + result fallback for everything else.
 *
 * Rendered only while expanded, so `enabled` is always true here; it is still
 * threaded through so a parent that keeps the bubble mounted (e.g. an exit
 * animation) can gate the fetch.
 */
export function ToolCallBubble({
  agentId,
  hash,
  enabled = true,
}: {
  agentId: string
  hash: string
  enabled?: boolean
}) {
  const { data, loading, error } = useToolCall(agentId, hash, enabled)

  return (
    <div className="mt-1 mb-2 ml-1 overflow-hidden rounded-lg border border-border/70 bg-card/60 text-[11px]">
      {loading ? (
        <div className="flex items-center gap-2 px-3 py-2 text-muted-foreground">
          <Loader2 className="size-3.5 animate-spin" />
          Loading tool call…
        </div>
      ) : error || !data ? (
        <div className="px-3 py-2 text-muted-foreground">Tool-call detail unavailable.</div>
      ) : (
        <BubbleBody detail={data} />
      )}
    </div>
  )
}

/** Coerce an arbitrary `params` value to a display string. */
function paramStr(params: ToolCallDetail["params"], key: string): string {
  const v = params[key]
  if (v === undefined || v === null) return ""
  return typeof v === "string" ? v : JSON.stringify(v, null, 2)
}

/** Dispatch on the tool name to the shape-appropriate renderer. */
function BubbleBody({ detail }: { detail: ToolCallDetail }) {
  const name = detail.name
  if (name === "Edit" && detail.params["old_string"] !== undefined) {
    return <EditDiff detail={detail} />
  }
  if (name.startsWith("console_")) {
    return <TerminalView detail={detail} />
  }
  if (name === "search" || name === "Open" || name.startsWith("brave_") || name.startsWith("firecrawl_")) {
    return <QueryResult detail={detail} />
  }
  return <KvResult detail={detail} />
}

/** Shared header: tool name + intent + error badge. */
function BubbleHeader({ detail }: { detail: ToolCallDetail }) {
  return (
    <div className="flex items-center gap-2 border-b border-border/60 bg-muted/40 px-3 py-1.5">
      <span className="font-mono font-semibold text-foreground/85">{detail.name}</span>
      {detail.intent && <span className="truncate text-muted-foreground">{detail.intent}</span>}
      {detail.isError && (
        <span className="ml-auto rounded-sm bg-(--danger)/15 px-1.5 py-0.5 text-[10px] text-(--danger)">
          error
        </span>
      )}
    </div>
  )
}

/** A raw result block — mono, scrollable, error-tinted. Empty result omitted. */
function ResultBlock({ detail }: { detail: ToolCallDetail }) {
  if (!detail.result) return null
  return (
    <pre
      className={cn(
        "max-h-64 overflow-auto px-3 py-2 font-mono text-[10.5px] leading-relaxed whitespace-pre-wrap",
        detail.isError ? "text-(--danger)" : "text-muted-foreground",
      )}
    >
      {detail.result}
    </pre>
  )
}

/** `Edit` — old_string (removed) over new_string (added), diff-tinted. */
function EditDiff({ detail }: { detail: ToolCallDetail }) {
  const file = paramStr(detail.params, "file_path")
  const oldStr = paramStr(detail.params, "old_string")
  const newStr = paramStr(detail.params, "new_string")
  return (
    <>
      <BubbleHeader detail={detail} />
      {file && <div className="px-3 pt-2 font-mono text-[10.5px] text-muted-foreground/80">{file}</div>}
      <div className="space-y-px p-2 font-mono text-[10.5px] leading-relaxed">
        <pre className="overflow-x-auto rounded-sm bg-(--danger)/10 px-2 py-1 whitespace-pre-wrap text-(--danger)">
          {oldStr}
        </pre>
        <pre className="overflow-x-auto rounded-sm bg-(--ok)/10 px-2 py-1 whitespace-pre-wrap text-(--ok)">
          {newStr}
        </pre>
      </div>
      <ResultBlock detail={detail} />
    </>
  )
}

/** `console_*` — the command/input, then the captured output as a terminal. */
function TerminalView({ detail }: { detail: ToolCallDetail }) {
  const cmd = paramStr(detail.params, "command") || paramStr(detail.params, "input")
  return (
    <>
      <BubbleHeader detail={detail} />
      {cmd && (
        <pre className="overflow-x-auto border-b border-border/40 bg-background/60 px-3 py-1.5 font-mono text-[10.5px] whitespace-pre-wrap text-(--interactive)">
          <span className="text-muted-foreground/60">$ </span>
          {cmd}
        </pre>
      )}
      <ResultBlock detail={detail} />
    </>
  )
}

/** `search` / read-style — the query line, then the result. */
function QueryResult({ detail }: { detail: ToolCallDetail }) {
  const query = paramStr(detail.params, "query") || paramStr(detail.params, "path")
  return (
    <>
      <BubbleHeader detail={detail} />
      {query && (
        <div className="border-b border-border/40 px-3 py-1.5 font-mono text-[10.5px] text-foreground/80">
          {query}
        </div>
      )}
      <ResultBlock detail={detail} />
    </>
  )
}

/** Generic fallback — a key/value table of every param, then the result. */
function KvResult({ detail }: { detail: ToolCallDetail }) {
  const entries = Object.entries(detail.params).filter(([k]) => k !== "intent" && k !== "verb")
  return (
    <>
      <BubbleHeader detail={detail} />
      {entries.length > 0 && (
        <div className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 px-3 py-2 font-mono text-[10.5px]">
          {entries.map(([k, v]) => (
            <div key={k} className="contents">
              <span className="text-muted-foreground/70">{k}</span>
              <span className="truncate text-foreground/80">
                {typeof v === "string" ? v : JSON.stringify(v)}
              </span>
            </div>
          ))}
        </div>
      )}
      <ResultBlock detail={detail} />
    </>
  )
}
