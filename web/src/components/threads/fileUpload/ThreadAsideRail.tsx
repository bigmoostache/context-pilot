import { PanelRightOpen } from "lucide-react"
import { ThreadAside } from "./ThreadAside"
import type { useThreadAside } from "./useThreadAside"
import type { ThreadFile } from "./FileSidebar"
import type { ThreadTask } from "@/lib/types"
import { HintBadge } from "@/components/shell/chrome/HintBadge"
import { useModifierShortcuts } from "@/lib/support/a11y"

/**
 * The thread conversation's right rail (T662 aside) plus its show/hide chrome
 * (T677) — extracted from {@link ThreadConversation} so that render body stays
 * within the 500-line file budget.
 *
 * Renders a fragment of two siblings, both meant to live directly inside the
 * conversation's `relative` `<main>`:
 *
 *  1. The slidable rail: {@link ThreadAside} wrapped in a `flex` slide wrapper
 *     (the flex is load-bearing — a plain block collapses the aside's
 *     flex-stretch height). When hidden it slides off the RIGHT edge via a
 *     transitioned negative right-margin — the mirror image of the left thread
 *     sidebar's collapse, clipped by `<main>`'s `overflow-hidden`. The offset is
 *     the aside width (`--sidebar-w`) plus its `mr-2` gutter.
 *  2. The discrete floating reopen button, shown only while hidden — no
 *     background, positioned against `<main>` (NOT the slid-away rail, which is
 *     off-screen). It is the inverted-direction counterpart of the sidebar's
 *     reopen affordance.
 *
 * The whole rail exists only when there is something to show (files OR tasks);
 * with neither, this renders nothing and the conversation takes the full width.
 */
export function ThreadAsideRail({
  agentId,
  files,
  tasks,
  aside,
  leftRailHidden,
}: {
  agentId: string
  files: ThreadFile[]
  tasks: ThreadTask[]
  aside: ReturnType<typeof useThreadAside>
  /** Whether the left thread-list rail is hidden — forwarded to {@link ThreadAside}
   *  so a file preview widens to half the viewport when it is (T680). */
  leftRailHidden: boolean
}) {
  // ⌘/Ctrl+H toggles the whole details rail — the exact header pattern
  // (useModifierShortcuts + a HintBadge that reveals the "H" while the modifier
  // is held). Hidden → show; shown → clear any preview and hide (mirrors the
  // tab bar's own hide button). Bound BEFORE the early return so the hook is
  // never conditional (rules-of-hooks); it's inert with no aside on screen.
  const modHeld = useModifierShortcuts({
    h: () => {
      if (aside.hidden) {
        aside.setHidden(false)
      } else {
        aside.setFile(null)
        aside.setHidden(true)
      }
    },
  })

  const hasAside = files.length > 0 || tasks.length > 0
  if (!hasAside) return null

  return (
    <>
      <div
        className="flex shrink-0 transition-[margin-right] duration-300 ease-[cubic-bezier(.16,1,.3,1)] motion-reduce:transition-none"
        style={{ marginRight: aside.hidden ? "calc(-1 * (var(--sidebar-w) + 0.5rem))" : 0 }}
      >
        <ThreadAside
          files={files}
          tasks={tasks}
          agentId={agentId}
          tab={aside.tab}
          onTabChange={aside.setTab}
          selectedFile={aside.file}
          onSelectFile={aside.setFile}
          leftRailHidden={leftRailHidden}
          hintShown={modHeld}
          onHide={() => {
            aside.setFile(null)
            aside.setHidden(true)
          }}
        />
      </div>

      {aside.hidden && (
        <button
          type="button"
          onClick={() => aside.setHidden(false)}
          aria-label="Show details rail"
          title="Show details"
          className="absolute top-3 right-3 z-10 flex size-7 items-center justify-center rounded-md text-muted-foreground/60 transition-colors hover:text-foreground"
        >
          <PanelRightOpen className="size-4" />
          <HintBadge label="H" shown={modHeld} />
        </button>
      )}
    </>
  )
}
