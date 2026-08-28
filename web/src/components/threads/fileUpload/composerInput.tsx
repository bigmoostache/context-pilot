import { useRef } from "react"
import { ArrowUp, Paperclip, Loader2, Clock, Pause } from "lucide-react"
import { Tip } from "@/components/ui/tip"
import type { Banner } from "@/lib/support/threadMessages"

/**
 * The composer's input row: the file-picker + paperclip, the auto-growing
 * textarea, and the send button. Extracted from {@link ThreadComposer} so the
 * outer component stays within the P8 complexity budget; owns its own hidden
 * file-input ref. Receives the textarea's ref/value/handlers from the parent's
 * `useComposer` hook and passes `ref={textareaRef}` as a bare identifier
 * (the react-hooks/refs pass allows that but rejects a member-access read).
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
  return (
    // Linear-style two-row composer (T692, UI only): the textarea spans the full
    // width on top, and a bottom action row holds the attach control (left) and
    // the circular submit (pushed right). Behaviour is unchanged — same refs,
    // handlers, autogrow and props; only the layout was restructured.
    <div className="card-shadow flex flex-col gap-1.5 rounded-2xl border border-border bg-surface-2 px-3 py-2.5 focus-within:border-(--signal)/60">
      <input
        ref={fileInputRef}
        type="file"
        multiple
        className="hidden"
        onChange={(e) => {
          const files = [...(e.target.files ?? [])]
          if (files.length > 0) void onAttach?.(files)
          // Reset so picking the same file again re-fires onChange.
          e.target.value = ""
        }}
      />
      <textarea
        ref={textareaRef}
        autoFocus
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
        placeholder="Reply to this thread…"
        rows={1}
        className="max-h-[200px] min-h-[24px] w-full resize-none bg-transparent px-1 pt-1 text-[13.5px] leading-relaxed text-foreground/90 outline-none placeholder:text-muted-foreground/60"
      />
      {/* BOTH BUTTONS ARE WRAPPED IN `Tip`, AND THAT CHANGES THE LAYOUT: Tip
          renders its own trigger <span> around the child, so the SPAN — not the
          button — is what this flex row lays out. Two consequences are handled
          on `triggerClassName` rather than on the buttons:
            * `inline-flex`, or the span is an inline box and the 28px button
              inside it is mis-sized;
            * `ml-auto` MOVES onto the send trigger. It used to sit on the send
              button, which was the flex item; left there it would now be on a
              child of the flex item and do nothing, and send would slide left
              until it touched the paperclip.
          The native `title=` attributes are gone with the same edit — leaving
          them would show the browser's tooltip on top of ours. */}
      <div className="flex items-center gap-1">
        {/* Colour-only hover, no fill. This sits INSIDE the composer pill,
            which is itself a filled surface — a second filled rectangle on
            hover reads as a box inside a box. The `disabled:hover:bg-transparent`
            that used to sit here went with the fill: there is no longer a
            background to cancel. */}
        <Tip
          title="Attach files"
          body="Upload files into this thread for the agent to read. Pick several at once, or paste an image straight into the box."
          triggerClassName="inline-flex"
        >
          <button
            onClick={() => fileInputRef.current?.click()}
            disabled={!onAttach}
            className="flex size-7 items-center justify-center rounded-md text-muted-foreground/60 transition-colors hover:text-(--interactive) disabled:cursor-default disabled:opacity-40 disabled:hover:text-muted-foreground/60"
          >
            <Paperclip className="size-4" />
          </button>
        </Tip>
        {/* The body states the REAL Enter rule, which is not the usual one:
            `resolveEnter` sends only when the caret is at the end AND the last
            line is blank, so a first Enter after text opens a new line and the
            second one sends. Documenting it as plain "Enter to send" would be
            wrong, and a tooltip that misstates a keybinding trains the wrong
            reflex. Wording follows the state, since the trigger span still
            hovers while the button itself is disabled. */}
        <Tip
          title="Send"
          body={
            sendable
              ? "Or press Enter on an empty last line — after typing, that means Enter twice. Shift+Enter always inserts a newline."
              : "Nothing to send yet — type a message first."
          }
          triggerClassName="ml-auto inline-flex"
        >
          <button
            onClick={onSubmit}
            disabled={!sendable}
            className="flex size-7 items-center justify-center rounded-full bg-(--signal) text-(--primary-foreground) transition-[filter] hover:brightness-105 disabled:opacity-40 disabled:hover:brightness-100"
          >
            <ArrowUp className="size-4" strokeWidth={2.5} />
          </button>
        </Tip>
      </div>
    </div>
  )
}

/** The turn-status banner element, or null (see `resolveComposerBanner`). */
export function ComposerBanner({ banner }: { banner: Banner }) {
  return (
    <div
      className={`mb-2 flex cursor-default items-center justify-center gap-2 rounded-xl border px-4 py-1.5 text-[13px] font-medium select-none ${banner.paused ? "border-amber-500/40 bg-amber-500/10 text-amber-600 dark:text-amber-400" : "border-border bg-muted/40 text-muted-foreground"}`}
    >
      {banner.paused ? (
        <Pause className="size-4" />
      ) : banner.working ? (
        <Loader2 className="size-4 animate-spin" style={{ color: banner.color }} />
      ) : (
        <Clock className="size-4" />
      )}
      <span>{banner.text}</span>
    </div>
  )
}
