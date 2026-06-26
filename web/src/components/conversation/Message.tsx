import { useState } from "react"
import { Check, ChevronDown, Copy, Terminal, User } from "lucide-react"
import type { ChatMessage } from "@/lib/types"
import { Markdown, type MarkdownVariant } from "@/lib/support/markdown"
import {
  splitMessageSegments,
  MessageFileChip,
  type UploadedFile,
} from "@/components/threads/fileUpload"
import { cn } from "@/lib/utils"

/**
 * Props every message renderer accepts.
 *
 * `agentId` + `onOpenFile` are threaded through so a ` ```file-upload ` block
 * embedded in the body can render its clickable attachment chip **inline** (via
 * {@link MessageBody}): `agentId` lets the chip verify the file still exists,
 * `onOpenFile` opens the shared Quick Look drawer for it. Both optional — a
 * surface that doesn't wire them still renders prose (and static, existence-
 * unchecked chips) unchanged.
 */
interface MessageProps {
  msg: ChatMessage
  /** owning agent — enables the inline attachment chip's existence check */
  agentId?: string
  /** open the shared Quick Look drawer for an inline attachment */
  onOpenFile?: (file: UploadedFile) => void
  /** navigate the Finder to a file's parent and select it */
  onShowInFinder?: (path: string) => void
}

export function Message({ msg, agentId, onOpenFile, onShowInFinder }: MessageProps) {
  if (msg.role === "tool" && msg.tool) return <ToolMessage msg={msg} />
  if (msg.role === "user") return <UserMessage msg={msg} agentId={agentId} onOpenFile={onOpenFile} onShowInFinder={onShowInFinder} />
  return <AssistantMessage msg={msg} agentId={agentId} onOpenFile={onOpenFile} onShowInFinder={onShowInFinder} />
}

/**
 * Render a message body, splicing any ` ```file-upload ` block into a clickable
 * attachment chip **at the exact position the block appeared** (not as a
 * separate trailing block), interleaved with the surrounding markdown prose.
 *
 * A body with no upload block is a single markdown render — zero behavioural
 * change for ordinary messages. Because the chip renders inside the bubble, it
 * inherits the bubble's side (user = right, assistant = left), so attachments
 * align with their author for free.
 */
function MessageBody({
  text,
  variant,
  agentId,
  onOpenFile,
  onShowInFinder,
}: {
  text: string
  variant: MarkdownVariant
  agentId?: string
  onOpenFile?: (file: UploadedFile) => void
  onShowInFinder?: (path: string) => void
}) {
  const segments = splitMessageSegments(text)
  // Fast path: no attachment block → a single markdown render.
  if (segments.every((s) => s.type === "text")) {
    return <Markdown text={text} variant={variant} />
  }
  return (
    <>
      {segments.map((seg, i) =>
        seg.type === "text" ? (
          <Markdown key={i} text={seg.text} variant={variant} />
        ) : (
          <div key={i} className="my-1">
            <MessageFileChip
              file={seg.file}
              agentId={agentId}
              onAccent={variant === "onAccent"}
              onOpen={onOpenFile ? () => onOpenFile(seg.file) : undefined}
              onShowInFinder={onShowInFinder ? () => onShowInFinder(seg.file.path) : undefined}
            />
          </div>
        ),
      )}
    </>
  )
}

/**
 * Discrete copy-to-clipboard affordance shown beneath a message bubble.
 *
 * Sits quietly at low opacity (brightening on hover/focus) so it never competes
 * with the message itself, and on click copies the message's plain text and
 * **transforms into a green check for ~2 s** before reverting — the only
 * feedback the action gives, matching the requested "discrete, click → green
 * tick for a few seconds" behaviour.
 *
 * `align` mirrors the bubble's side so the control tucks under the message's
 * own edge (user bubbles are right-aligned, assistant left-aligned).
 */
export function CopyButton({
  text,
  getText,
  align,
  label = "Copy",
  className: extra,
}: {
  /** Static text to copy. Ignored when `getText` is provided. */
  text?: string
  /** Lazy text extraction — called on click, for DOM-derived content. */
  getText?: () => string
  align: "start" | "end"
  /** Button label shown next to the icon (e.g. "Copy code", "Copy table"). */
  label?: string
  className?: string
}) {
  const [copied, setCopied] = useState(false)

  const onCopy = () => {
    const t = getText ? getText() : text ?? ""
    // `?.` guards environments without the async clipboard API (insecure
    // origin / older browser); a failed write is silently ignored — the worst
    // case is the tick simply doesn't flash, never a thrown error in the UI.
    navigator.clipboard?.writeText(t).then(
      () => {
        setCopied(true)
        window.setTimeout(() => setCopied(false), 2000)
      },
      () => {},
    )
  }

  return (
    <button
      type="button"
      onClick={onCopy}
      aria-label={copied ? "Copied" : label}
      className={cn(
        "flex items-center gap-1 rounded-md px-1 py-0.5 text-[10px] transition-colors",
        "opacity-50 hover:opacity-100 focus-visible:opacity-100 outline-none",
        copied ? "text-[var(--ok)] opacity-100" : cn("text-muted-foreground/70 hover:text-foreground", extra),
        align === "end" ? "self-end" : "self-start",
      )}
    >
      {copied ? <Check className="size-3" /> : <Copy className="size-3" />}
      <span>{copied ? "Copied" : label}</span>
    </button>
  )
}

function UserMessage({ msg, agentId, onOpenFile, onShowInFinder }: MessageProps) {
  return (
    <div className="rise flex flex-col items-end gap-1 py-2">
      <div className="max-w-[78%] rounded-2xl rounded-br-md bg-[var(--signal)] px-3.5 py-2 text-[13px] leading-relaxed text-[var(--primary-foreground)] card-shadow">
        <MessageBody text={msg.text ?? ""} variant="onAccent" agentId={agentId} onOpenFile={onOpenFile} onShowInFinder={onShowInFinder} />
      </div>
      <span className="flex items-center gap-1 pr-1 text-[10px] text-muted-foreground/60">
        <User className="size-2.5" />
        {msg.ts}
      </span>
      <CopyButton text={msg.text ?? ""} align="end" label="Copy message" />
    </div>
  )
}

function AssistantMessage({ msg, agentId, onOpenFile, onShowInFinder }: MessageProps) {
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
        <MessageBody text={msg.text ?? ""} variant="default" agentId={agentId} onOpenFile={onOpenFile} onShowInFinder={onShowInFinder} />
        {msg.streaming && (
          <span className="cursor-blink ml-0.5 inline-block h-3.5 w-[7px] translate-y-0.5 bg-[var(--signal)]" />
        )}
      </div>
      {!msg.streaming && (
        <div className="pl-7">
          <CopyButton text={msg.text ?? ""} align="start" label="Copy message" />
        </div>
      )}
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
