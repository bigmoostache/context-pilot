import { useEffect, useRef } from "react"
import { animate, createSpring } from "animejs"
import { prefersReducedMotion } from "@/lib/utils"
import { ArrowUp, Plus, Loader2, Clock, Pause } from "lucide-react"

/** The turn-status banner shown above the composer input, or null. */
export interface Banner {
  working: boolean
  paused: boolean
  color: string | undefined
  text: string
}

/**
 * The composer's input row — mobile-tuned twin. Same structure as desktop
 * (paperclip + auto-grow textarea + send) with touch-first sizing: 16px textarea
 * font (below 16px iOS Safari auto-zooms the viewport on focus), and 36px tap
 * targets for the paperclip + send.
 */
export function ComposerInputRow({
  textareaRef,
  text,
  sendable,
  onChange,
  onSelect,
  onKeyDown,
  onSubmit,
  onAttach,
}: {
  textareaRef: React.RefObject<HTMLTextAreaElement | null>
  text: string
  sendable: boolean
  onChange: (e: React.ChangeEvent<HTMLTextAreaElement>) => void
  onSelect: (e: React.SyntheticEvent<HTMLTextAreaElement>) => void
  onKeyDown: (e: React.KeyboardEvent<HTMLTextAreaElement>) => void
  onSubmit: () => void
  onAttach: ((files: File[]) => void | Promise<void>) | undefined
}) {
  const fileInputRef = useRef<HTMLInputElement>(null)
  // #4 Send-button pop (anime.js): spring the send button in when it becomes
  // available (iMessage pops it, not a hard cut). Conditionally rendered, so
  // this fires when `sendable` flips true. Reduced-motion skips it.
  const sendBtnRef = useRef<HTMLButtonElement>(null)
  useEffect(() => {
    const btn = sendBtnRef.current
    if (!btn || !sendable || prefersReducedMotion()) return
    animate(btn, {
      scale: [0, 1],
      opacity: [0, 1],
      ease: createSpring({ stiffness: 600, damping: 20 }),
    })
  }, [sendable])
  return (
    // iMessage-style row: standalone round attach button on the LEFT, then a
    // single rounded pill holding the textarea with send tucked inside its edge.
    <div className="flex items-end gap-2">
      <input
        ref={fileInputRef}
        type="file"
        multiple
        className="hidden"
        onChange={(e) => {
          const files = [...(e.target.files ?? [])]
          if (files.length > 0) void onAttach?.(files)
          e.target.value = ""
        }}
      />
      {/* Standalone attach affordance (like iMessage's +), outside the pill. */}
      <button
        onClick={() => fileInputRef.current?.click()}
        disabled={!onAttach}
        title="Attach files"
        aria-label="Attach files"
        className="flex size-9 shrink-0 items-center justify-center rounded-full bg-card/60 text-muted-foreground/70 backdrop-blur-[3px] transition-colors active:bg-muted active:text-(--interactive) disabled:cursor-default disabled:opacity-40"
      >
        <Plus className="size-5.5" strokeWidth={2.25} />
      </button>

      {/* The input pill — thin border, subtle fill, fully rounded. Send button
          lives INSIDE the pill's right edge (iMessage convention). */}
      <div className="flex min-w-0 flex-1 items-end gap-1 rounded-[1.35rem] border border-border bg-card/70 py-1 pr-1 pl-3.5 backdrop-blur-[3px] focus-within:border-(--signal)/60">
        <textarea
          ref={textareaRef}
          value={text}
          onChange={onChange}
          onSelect={onSelect}
          onKeyDown={onKeyDown}
          onPaste={(e) => {
            const items = [...e.clipboardData.items]
            const images = items
              .filter((i) => i.kind === "file" && i.type.startsWith("image/"))
              .map((i) => i.getAsFile())
              .filter((f): f is File => f !== null)
            if (images.length > 0 && onAttach) {
              e.preventDefault()
              void onAttach(images)
            }
          }}
          placeholder="Message…"
          rows={1}
          className="max-h-[200px] min-h-[30px] min-w-0 flex-1 resize-none self-center bg-transparent py-1 text-[16px] leading-snug text-foreground/90 outline-none placeholder:text-muted-foreground/50"
        />
        {/* Send appears only when there's something to send (empty pill stays
            clean). */}
        {sendable && (
          <button
            ref={sendBtnRef}
            onClick={onSubmit}
            aria-label="Send message"
            className="mb-0.5 flex size-8 shrink-0 items-center justify-center rounded-full bg-(--signal) text-(--primary-foreground) transition-[filter] active:brightness-110"
          >
            <ArrowUp className="size-4.5" strokeWidth={2.75} />
          </button>
        )}
      </div>
    </div>
  )
}

/** The turn-status banner element, or null (see `resolveComposerBanner`). */
export function ComposerBanner({ banner }: { banner: Banner }) {
  return (
    <div
      className={`mb-2 flex items-center justify-center gap-2 rounded-xl px-3 py-1.5 text-[12px] ${banner.paused ? "bg-amber-500/10 text-amber-600 dark:text-amber-400" : "bg-muted/40 text-muted-foreground"}`}
    >
      {banner.paused ? (
        <Pause className="size-3.5" />
      ) : banner.working ? (
        <Loader2 className="size-3.5 animate-spin" style={{ color: banner.color }} />
      ) : (
        <Clock className="size-3.5" />
      )}
      <span>{banner.text}</span>
    </div>
  )
}
