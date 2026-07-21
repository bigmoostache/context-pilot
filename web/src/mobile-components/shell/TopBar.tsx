import { Menu } from "lucide-react"

interface TopBarProps {
  /** Text shown centred in the bar — the active view's label. */
  title: string
  /** Fired when the hamburger is tapped (opens the nav drawer / mission control). */
  onMenu: () => void
}

/**
 * Mobile top bar — the divergent twin of `components/shell/TopBar`.
 *
 * The desktop bar packs a dense control cluster (app mark, agent switcher,
 * per-agent view tabs, action icons) into one horizontal strip — unusable at
 * phone width. On mobile that surface is split: primary view switching moves to
 * a thumb-reachable bottom `TabBar` (see `Root`), and the secondary actions move
 * behind a hamburger drawer. So this bar is deliberately minimal: a menu button
 * plus the current view's title.
 *
 * This is the FIRST hand-authored (marker-less) mobile twin — it shares the
 * mirror PATH with its desktop source but not the export signature (path-parity
 * is the mirror contract, design §5). It imports nothing from the component tree
 * (only `lucide-react`), so the leak guard is trivially satisfied.
 */
export function TopBar({ title, onMenu }: TopBarProps) {
  return (
    <header className="flex h-12 shrink-0 items-center gap-3 border-b border-border px-4">
      <button
        onClick={onMenu}
        aria-label="Open menu"
        className="flex size-8 items-center justify-center rounded-md text-foreground/80 transition-colors hover:bg-muted/60"
      >
        <Menu className="size-5" />
      </button>
      <span className="text-[15px] font-semibold tracking-tight">{title}</span>
    </header>
  )
}
