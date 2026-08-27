import { Moon, Sun } from "lucide-react"
import { useTheme } from "@/lib/providers/theme"
import { useModifierShortcuts } from "@/lib/support/a11y"
import { HintBadge } from "../chrome/HintBadge"
import { cn } from "@/lib/utils"

/**
 * The letter that toggles the palette while ⌘/Ctrl is held.
 *
 * NOT `T`, which is what the control's own name would suggest. ⌘T is in the
 * browser's RESERVED set (new tab) alongside ⌘N and ⌘W: the keydown is consumed
 * before the page sees it and `preventDefault` is ignored on it, so a literal
 * binding would open a tab AND flip the theme in the tab left behind — strictly
 * worse than no shortcut. `D` (dark) is free and interceptable.
 */
const THEME_KEY = "d"

/**
 * macOS-style segmented light/dark switch. Two pills (sun · moon); the active
 * palette is highlighted. Reads/writes the global theme context.
 *
 * ⌘/Ctrl + D flips it. THE HINT BADGE SITS ON THE INACTIVE SEGMENT — the one
 * the key would move you TO, not the one you are on: it is a signpost, and a
 * signpost naming where you already stand tells you nothing. Both segments
 * render one, so flipping the palette while the modifier is held crossfades the
 * badge across rather than making it blink out and back.
 *
 * The binding lives here rather than in the shell for the reason
 * {@link useModifierShortcuts} spells out: the listener mounts and unmounts with
 * the control, so "bound exactly while the switch is on screen" is true by
 * construction instead of by a second visibility condition that could drift.
 *
 * @param vertical Stack the two pills instead of laying them side by side —
 *                 the left rail is 56px wide and a horizontal pair pushed the
 *                 chrome past it.
 */
export function ThemeToggle({ vertical = false }: { vertical?: boolean }) {
  const { theme, setTheme } = useTheme()
  const dark = theme === "dark"
  const modHeld = useModifierShortcuts({ [THEME_KEY]: () => setTheme(dark ? "light" : "dark") })

  return (
    <div
      className={cn(
        // The second deliberate exception to "no borders" (the other is the
        // view-tab group): the segmented switch needs an outline to read as one
        // control with two halves. `--border-strong` opts back in.
        "flex items-center gap-0.5 rounded-full border border-(--border-strong) bg-muted/60",
        // `p-1` in the rail, not `p-0.5`: 32px segment + 4px padding either side
        // = a 40px-wide group, which is exactly the width of the view-tab group
        // stacked above it (`w-full` inside the rail's 40px content box). At
        // `p-0.5` the group came out 36px and sat visibly narrower than its
        // neighbour. Horizontal keeps the tighter padding.
        vertical ? "flex-col p-1" : "p-0.5",
      )}
    >
      <Seg
        active={!dark}
        hintShown={modHeld && dark}
        onClick={() => setTheme("light")}
        label="Light"
      >
        <Sun className="size-4" />
      </Seg>
      <Seg active={dark} hintShown={modHeld && !dark} onClick={() => setTheme("dark")} label="Dark">
        <Moon className="size-4" />
      </Seg>
    </div>
  )
}

function Seg({
  active,
  hintShown,
  onClick,
  label,
  children,
}: {
  active: boolean
  /** Reveal the ⌘/Ctrl shortcut badge on THIS segment. True only on the
   *  inactive one — see {@link ThemeToggle}. */
  hintShown: boolean
  onClick: () => void
  label: string
  children: React.ReactNode
}) {
  return (
    <button
      type="button"
      aria-label={label}
      aria-pressed={active}
      onClick={onClick}
      className={cn(
        // `size-8` matches every other button in the header rail (the view
        // tabs, the agent switcher's glyph). The group is what used to be
        // button-sized; it is the two SEGMENTS that have to be, since they are
        // the things being clicked.
        //
        // `relative` is what the badge positions against.
        "relative flex size-8 items-center justify-center rounded-full transition-all",
        active
          ? "card-shadow bg-card text-foreground"
          : "text-muted-foreground hover:text-foreground",
      )}
    >
      {children}
      <HintBadge label={THEME_KEY.toUpperCase()} shown={hintShown} />
    </button>
  )
}
