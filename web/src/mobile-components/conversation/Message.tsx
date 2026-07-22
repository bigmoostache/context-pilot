// ── Chat message renderer (conversation) — mobile twin ──────────────
//
// Touch twin of the desktop Message. Same three renderers (user / assistant /
// tool) and the same inline `file-upload` chip splicing; the presentation is
// mobile-tuned: bubbles claim more horizontal room on a narrow viewport (a
// desktop 78/88% would wrap hard on a phone), and the secondary copy/delete
// actions stay readable without a hover (there is none on touch). The inline
// attachment chip + segment splitter are pulled from the mobile mirror tree
// (leak guard) so a mobile message never reaches into the desktop tree.

import { useEffect, useRef } from "react"
import { animate, createSpring } from "animejs"
import { ChevronDown, Terminal, Trash2, User } from "lucide-react"
import type { ChatMessage } from "@/lib/types"
import { Markdown, type MarkdownVariant } from "@/lib/support/markdown"
import { CopyButton } from "./CopyButton"
import { MessageFileChip } from "@/mobile-components/threads/fileUpload"
import { splitMessageSegments, type UploadedFile } from "@/mobile-components/threads/fileUpload/helpers"
import { cn, prefersReducedMotion } from "@/lib/utils"

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
  agentId?: string | undefined
  /** open the shared Quick Look drawer for an inline attachment */
  onOpenFile?: ((file: UploadedFile) => void) | undefined
  /** navigate the Finder to a file's parent and select it */
  onShowInFinder?: ((path: string) => void) | undefined
  /** permanently delete this message from the thread */
  onDelete?: (() => void) | undefined
  /** true for a message that just APPENDED (not part of the initial load) —
   *  drives the anime.js spring pop that reads as send/receive confirmation. */
  fresh?: boolean | undefined
}

export function Message({ msg, agentId, onOpenFile, onShowInFinder, onDelete, fresh }: MessageProps) {
  if (msg.role === "tool" && msg.tool) return <ToolMessage msg={msg} />
  if (msg.role === "user")
    return (
      <UserMessage
        msg={msg}
        agentId={agentId}
        onOpenFile={onOpenFile}
        onShowInFinder={onShowInFinder}
        onDelete={onDelete}
        fresh={fresh}
      />
    )
  return (
    <AssistantMessage
      msg={msg}
      agentId={agentId}
      onOpenFile={onOpenFile}
      onShowInFinder={onShowInFinder}
      onDelete={onDelete}
      fresh={fresh}
    />
  )
}

/**
 * Spring-pop a just-appended message bubble (iMessage send/receive
 * confirmation). No-op unless `fresh` is set — the initial history batch mounts
 * every row at once and must NOT all pop; only a message appended after the
 * first render carries `fresh`. Honours prefers-reduced-motion. Keyed on
 * `fresh`: the appended row mounts already-fresh so the pop fires once; when a
 * later append demotes it to non-fresh the effect re-runs and early-returns.
 */
function useBubblePop(fresh: boolean | undefined) {
  const ref = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const el = ref.current
    if (!el || !fresh || prefersReducedMotion()) return
    animate(el, {
      scale: [0.96, 1],
      translateY: [8, 0],
      opacity: [0, 1],
      ease: createSpring({ stiffness: 500, damping: 24 }),
    })
  }, [fresh])
  return ref
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
  agentId?: string | undefined
  onOpenFile?: ((file: UploadedFile) => void) | undefined
  onShowInFinder?: ((path: string) => void) | undefined
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
 * Discrete delete affordance shown beside the copy button beneath a message.
 *
 * On touch there is no hover, so the button sits at a steady readable opacity
 * (the desktop twin fades it in on hover) and uses an active-press tint for
 * feedback; it still shows the danger colour to signal destructiveness.
 */
function DeleteButton({ align, onDelete }: { align: "start" | "end"; onDelete: () => void }) {
  return (
    <button
      type="button"
      onClick={onDelete}
      aria-label="Delete message"
      className={cn(
        // Match CopyButton's box metrics EXACTLY (gap-1, px-1 py-0.5, text-[10px],
        // size-3 icon) so the two buttons + the timestamp span render at the same
        // height — otherwise the action row's `items-center` fights each child's
        // `self-*` and they visibly stagger (T613). No hover on touch: steady
        // opacity with an active-press danger tint.
        "flex items-center gap-1 rounded-md px-1 py-0.5 text-[10px] transition-colors",
        "text-muted-foreground/70 outline-none active:text-(--danger)",
        align === "end" ? "self-end" : "self-start",
      )}
    >
      <Trash2 className="size-3" />
      <span>Delete</span>
    </button>
  )
}

function UserMessage({ msg, agentId, onOpenFile, onShowInFinder, onDelete, fresh }: MessageProps) {
  const bubbleRef = useBubblePop(fresh)
  return (
    <div className="rise flex flex-col items-end gap-1 py-2">
      {/* wider bubble than desktop (78%) — a phone needs the horizontal room */}
      <div
        ref={bubbleRef}
        className="card-shadow max-w-[85%] rounded-2xl rounded-br-md bg-(--signal) px-3.5 py-2 text-[13px] leading-relaxed text-(--primary-foreground)"
      >
        <MessageBody
          text={msg.text ?? ""}
          variant="onAccent"
          agentId={agentId}
          onOpenFile={onOpenFile}
          onShowInFinder={onShowInFinder}
        />
      </div>
      <span className="flex items-center gap-1 pr-1 text-[10px] text-muted-foreground/60">
        <User className="size-2.5" />
        {msg.ts}
      </span>
      <div className="flex items-center gap-2">
        <CopyButton text={msg.text ?? ""} align="end" label="Copy message" />
        {onDelete && <DeleteButton align="end" onDelete={onDelete} />}
      </div>
    </div>
  )
}

function AssistantMessage({ msg, agentId, onOpenFile, onShowInFinder, onDelete, fresh }: MessageProps) {
  const bubbleRef = useBubblePop(fresh)
  return (
    <div className="rise flex flex-col gap-1.5 py-2">
      {/* No author header on mobile (T611) — the orange mark + "Context Pilot"
          label were pure vertical-space cost. The assistant bubble is already
          identifiable by its left alignment; the timestamp moves down into the
          action row beneath the message. */}
      {/* wider than desktop (88%) + a slimmer indent to reclaim phone width */}
      <div
        ref={bubbleRef}
        className="max-w-[92%] pl-1 text-[13.5px] leading-relaxed text-foreground/90"
      >
        <MessageBody
          text={msg.text ?? ""}
          variant="default"
          agentId={agentId}
          onOpenFile={onOpenFile}
          onShowInFinder={onShowInFinder}
        />
        {msg.streaming && (
          <span className="cursor-blink ml-0.5 inline-block h-3.5 w-[7px] translate-y-0.5 bg-(--signal)" />
        )}
      </div>
      {!msg.streaming && (
        <div className="flex items-center gap-2 pl-1">
          <span className="text-[10px] leading-none text-muted-foreground/60">{msg.ts}</span>
          <CopyButton text={msg.text ?? ""} align="start" label="Copy message" />
          {onDelete && <DeleteButton align="start" onDelete={onDelete} />}
        </div>
      )}
    </div>
  )
}

function ToolMessage({ msg }: { msg: ChatMessage }) {
  if (!msg.tool) return null
  const t = msg.tool
  return (
    <div className="rise py-2 pl-1">
      <div
        className={cn(
          "card-shadow max-w-[92%] overflow-hidden rounded-xl border bg-card",
          t.isError ? "border-(--danger)/50" : "border-border",
        )}
      >
        {/* header */}
        <div className="flex items-center gap-2 border-b border-border bg-muted/50 px-3 py-1.5">
          <Terminal className="size-3.5 text-(--interactive)" />
          <span className="text-[12px] font-semibold text-foreground/90">{t.name}</span>
          <span className="truncate text-[11px] text-muted-foreground">{t.intent}</span>
          <ChevronDown className="ml-auto size-3.5 text-muted-foreground/50" />
        </div>
        {/* params */}
        <div className="px-3 py-2">
          {Object.entries(t.params ?? {}).map(([k, v]) => (
            <div key={k} className="flex gap-2 font-mono text-[11px] leading-relaxed">
              <span className="shrink-0 text-muted-foreground/70">{k}</span>
              <span className="truncate text-foreground/75">{v}</span>
            </div>
          ))}
          {t.result && (
            <pre
              className={cn(
                "mt-2 overflow-x-auto rounded-md bg-muted/60 px-2.5 py-1.5 font-mono text-[10.5px] leading-relaxed whitespace-pre-wrap",
                t.isError ? "text-(--danger)" : "text-muted-foreground",
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
