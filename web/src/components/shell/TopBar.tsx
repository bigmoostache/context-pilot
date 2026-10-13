import { useState } from "react"
import {
  MessagesSquare,
  FolderTree,
  BarChart3,
  Plus,
  Search,
  SlidersHorizontal,
} from "lucide-react"
import { ThemeToggle } from "./widgets/ThemeToggle"
import { FleetSwitcher } from "./widgets/FleetSwitcher"
import { UsageButton } from "./widgets/UsageButton"
import { HintBadge } from "./chrome/HintBadge"
import { ConfigModal } from "./config/ConfigModal"
import { ProfileModal } from "./widgets/ProfileModal"
import { UserMenu } from "./widgets/UserMenu"
import { UsersDialog } from "@/components/auth/UsersDialog"
import { Tip } from "@/components/ui/tip"
import { useDevMode } from "@/lib/providers/toggles/devMode"
import { useFeatures } from "@/lib/providers/toggles/features"
import { useModifierShortcuts } from "@/lib/support/a11y"
import type { Fleet, ViewMode } from "@/lib/types"
import { cn } from "@/lib/utils"

interface TopBarProps {
  view: ViewMode
  onViewChange: (v: ViewMode) => void
  activeFleetId: string
  onSwitchFleet: (id: string) => void
  fleets: Fleet[]
  /** Whether the agents view's list rail is open — drives the Agents tab's
   *  `aria-expanded` and its tooltip wording while that view is active. */
  agentsRailOpen: boolean
  /** Collapse ↔ expand the agents list rail. Fired by clicking the Agents
   *  tab when agents is ALREADY the active view. */
  onToggleAgentsRail: () => void
  /** Open the New Agent dialog. Rail-hosted, so shown only on the agents view. */
  onNewAgent: () => void
  /** Open the agent search palette. Same gating as {@link onNewAgent}. */
  onSearchAgents: () => void
  /** Open the focused agent's settings DIALOG. The rail owns the trigger, so
   *  the flag it flips has to live in the shell — see `manageOpen` in Root. */
  onManageFleet: () => void
  /** Whether the finder view's explorer rail is open — drives the Finder tab's
   *  `aria-expanded` and its tooltip wording while that view is active. */
  finderRailOpen: boolean
  /** Collapse ↔ expand the finder explorer rail. Fired by clicking the Finder
   *  tab when finder is ALREADY the active view. */
  onToggleFinderRail: () => void
}

/** Slim macOS-style side rail — app mark (→ fleet), workspace switcher,
 *  per-agent view tabs (Agents · Finder), theme, usage, account. Vertical: it
 *  hugs the window's left edge, full height, account cluster pinned to the floor. */
export function TopBar({
  view,
  onViewChange,
  activeFleetId,
  onSwitchFleet,
  fleets,
  agentsRailOpen,
  onToggleAgentsRail,
  onNewAgent,
  onSearchAgents,
  onManageFleet,
  finderRailOpen,
  onToggleFinderRail,
}: TopBarProps) {
  const activeFleet = fleets.find((a) => a.id === activeFleetId) ?? fleets[0]
  // OAuth usage/login widget: only for the OAuth providers (`anthropic` uses an
  // API key), and only where the deployment offers the subscription at all.
  const isClaudeOAuth =
    useFeatures().claude_oauth &&
    (activeFleet?.provider === "claudecode" || activeFleet?.provider === "claudecodev2")
  const inFleet = view === "fleet"
  const { devMode } = useDevMode()
  const [configOpen, setConfigOpen] = useState(false)
  const [profileOpen, setProfileOpen] = useState(false)
  const [usersOpen, setUsersOpen] = useState(false)

  // ⌘/Ctrl+A opens the workspace switcher (T646) via its CONTROLLED `open`
  // state (a synthetic click won't open a base-ui menu); `modHeld` reveals the
  // "A" HintBadge. `a` yields while a field holds text, fires when it's empty.
  const [switcherOpen, setSwitcherOpen] = useState(false)
  const switcherModHeld = useModifierShortcuts({ a: () => setSwitcherOpen(true) })

  return (
    <>
      {/* No border, no fill: the rail sits directly on `--background` (the old
          `.vibrancy` + `border-r` are gone). `p-2` insets all four sides;
          `gap-3` is separate rhythm BETWEEN items, not an edge inset. */}
      <header className="flex w-14 shrink-0 flex-col items-center gap-3 p-2">
        <Tip
          title="Workspace"
          body="The fleet you're working in — one fleet, one folder. Switch or manage from here."
          side="right"
        >
          <FleetSwitcher
            rail
            open={switcherOpen}
            onOpenChange={setSwitcherOpen}
            shortcutHint="A"
            hintShown={switcherModHeld}
            fleets={fleets}
            activeId={inFleet ? undefined : activeFleetId}
            onManageFleets={() => onViewChange("fleet")}
            onSwitch={
              inFleet
                ? (id) => {
                    onSwitchFleet(id)
                    onViewChange("agents")
                  }
                : onSwitchFleet
            }
          />
        </Tip>

        {!inFleet && (
          <ViewTabs
            view={view}
            onViewChange={onViewChange}
            devMode={devMode}
            agentsRailOpen={agentsRailOpen}
            onToggleAgentsRail={onToggleAgentsRail}
            finderRailOpen={finderRailOpen}
            onToggleFinderRail={onToggleFinderRail}
          />
        )}

        {/* The agents view's own list actions. Gated on the agents view being
            SELECTED: they act on the agent list, so on Finder or Costs they
            would be controls for a surface that is not on screen. */}
        {!inFleet && view === "agents" && (
          <AgentActions onNewAgent={onNewAgent} onSearchAgents={onSearchAgents} />
        )}

        <TopBarActions
          isClaudeOAuth={isClaudeOAuth}
          onManageFleet={onManageFleet}
          setConfigOpen={setConfigOpen}
          setProfileOpen={setProfileOpen}
          setUsersOpen={setUsersOpen}
        />
      </header>

      <ConfigModal open={configOpen} onClose={() => setConfigOpen(false)} />
      <ProfileModal open={profileOpen} onClose={() => setProfileOpen(false)} />
      <UsersDialog open={usersOpen} onClose={() => setUsersOpen(false)} />
    </>
  )
}

/** Bottom controls cluster: the agent settings dialog, theme toggle, Claude
 *  Usage button and the account menu. Extracted from {@link TopBar} for the P8
 *  complexity budget.
 *
 *  Agent settings lives HERE rather than in {@link ViewTabs} because it is not
 *  a view any more (T760): it opens a dialog, so it is a command, and commands
 *  in this rail are bare glyphs pinned to the floor. It sits directly ABOVE the
 *  theme toggle — the two are the app-level and agent-level ends of the rail,
 *  and the eye reaches for the appearance controls at the very bottom. */
function TopBarActions({
  isClaudeOAuth,
  onManageFleet,
  setConfigOpen,
  setProfileOpen,
  setUsersOpen,
}: {
  isClaudeOAuth: boolean
  onManageFleet: () => void
  setConfigOpen: (v: boolean) => void
  setProfileOpen: (v: boolean) => void
  setUsersOpen: (v: boolean) => void
}) {
  // ⌘/Ctrl+I opens the agent settings dialog. Bound HERE, beside the button, so
  // the key listener's lifetime matches the control it drives — the same
  // rule {@link AgentActions} follows for C and P.
  const modHeld = useModifierShortcuts({ i: onManageFleet })

  return (
    <div className="mt-auto flex flex-col items-center gap-3">
      <Tip title="Fleet settings" body="This fleet's name, model and service vitals." side="right">
        <RailAction
          label="Fleet settings"
          icon={SlidersHorizontal}
          hint="I"
          hintShown={modHeld}
          onClick={onManageFleet}
        />
      </Tip>
      <Tip title="Appearance" body="Switch between light and dark." side="right">
        <span className="inline-flex">
          <ThemeToggle vertical />
        </span>
      </Tip>
      {isClaudeOAuth && <UsageButton />}
      <Tip title="Account" body="Your profile, app settings, and sign-out." side="right">
        <UserMenu
          onOpenSettings={() => setConfigOpen(true)}
          onOpenProfile={() => setProfileOpen(true)}
          onOpenUsers={() => setUsersOpen(true)}
        />
      </Tip>
    </div>
  )
}

/**
 * The agents view's list actions — New agent · Search — as a standalone
 * cluster under the view tabs.
 *
 * DELIBERATELY UNLIKE {@link ViewTabs} ABOVE IT. That is a segmented selector,
 * so it carries a border and a fill to read as one control with a chosen
 * member. These two are independent commands with no selected state, so a
 * frame around them would claim a relationship they do not have. They get no
 * border and no fill — only a hover wash, the same affordance every other bare
 * glyph in the rail uses.
 *
 * They live here rather than in the agent list because the list can be
 * collapsed (the Agents tab hides it), and an action that disappears with the
 * panel it acts on is an action the user has to restore the panel to reach.
 *
 * The ⌘/Ctrl shortcuts are bound HERE rather than in the shell, so the key
 * listener mounts and unmounts with the buttons — see {@link
 * useModifierShortcuts}, which explains why that matters.
 */
function AgentActions({
  onNewAgent,
  onSearchAgents,
}: {
  onNewAgent: () => void
  onSearchAgents: () => void
}) {
  const modHeld = useModifierShortcuts({ c: onNewAgent, p: onSearchAgents })

  return (
    <div className="flex w-full flex-col items-center gap-0.5">
      <Tip title="New agent" body="Start a fresh conversation or task in this fleet." side="right">
        <RailAction
          label="New agent"
          icon={Plus}
          hint="C"
          hintShown={modHeld}
          onClick={onNewAgent}
        />
      </Tip>
      <Tip
        title="Search agents"
        body="Find a agent by name, or search across every message in this fleet."
        side="right"
      >
        <RailAction
          label="Search agents"
          icon={Search}
          hint="P"
          hintShown={modHeld}
          onClick={onSearchAgents}
        />
      </Tip>
    </div>
  )
}

/** A bare command button in the rail: no border, no fill, hover wash only.
 *  Same 32px square + 16px glyph metric as {@link ViewTab}, so the two groups
 *  line up down the rail even though only one of them is framed.
 *
 *  @param hint      The shortcut letter, shown bottom-right while ⌘/Ctrl is held.
 *  @param hintShown Whether to reveal it. A SEPARATE required prop rather than
 *                   an optional `hint`: `exactOptionalPropertyTypes` is on in
 *                   this tree, so an optional string would widen the type at
 *                   every call site for no gain. */
function RailAction({
  label,
  icon: Icon,
  hint,
  hintShown,
  onClick,
}: {
  label: string
  icon: typeof MessagesSquare
  hint: string
  hintShown: boolean
  onClick: () => void
}) {
  return (
    <button
      onClick={onClick}
      aria-label={label}
      // `relative` so the badge can be positioned against this button, and
      // `overflow-visible` is NOT needed — the badge is nudged inward rather
      // than hung outside, so it cannot collide with the neighbouring control.
      className="relative flex size-8 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
    >
      <Icon className="size-4" />
      <HintBadge label={hint} shown={hintShown} />
    </button>
  )
}

function ViewTab({
  active,
  onClick,
  icon: Icon,
  label,
  expanded,
  hint,
  hintShown = false,
}: {
  active: boolean
  onClick: () => void
  icon: typeof MessagesSquare
  label: string
  /** Disclosure state, for the tabs that ALSO collapse a panel when they are
   *  already active. Pass `undefined` on a tab that only switches views —
   *  claiming `aria-expanded` there would describe a control this button does
   *  not have. */
  expanded?: boolean | undefined
  /** The ⌘/Ctrl shortcut letter, shown bottom-right while the modifier is held
   *  AND this tab's shortcut is currently bound (see {@link ViewTabs}). Omit on
   *  a tab with no shortcut (Costs). */
  hint?: string | undefined
  /** Whether to reveal the hint badge right now. */
  hintShown?: boolean
}) {
  return (
    <button
      onClick={onClick}
      // Icon-only in the rail: at 56px wide there is no room for the label, and
      // every tab already carries a Tip naming it (see ViewTabs).
      aria-label={label}
      aria-expanded={expanded}
      className={cn(
        "relative flex size-8 items-center justify-center rounded-md transition-all",
        active
          ? "card-shadow bg-card text-foreground"
          : "text-muted-foreground hover:text-foreground",
      )}
    >
      <Icon className="size-4" />
      {hint && <HintBadge label={hint} shown={hintShown} />}
    </button>
  )
}

/**
 * Build the view-tab click handlers AND bind them as ⌘/Ctrl shortcuts — one
 * definition for both, so a shortcut is "as if you clicked the tab". Each is
 * DUAL-PURPOSE like the tab click: off the view navigate there, on the view
 * toggle that view's rail (L/K stay live on their own view). L not C drives
 * Agents — C is the new-agent shortcut in sibling {@link AgentActions}.
 * Settings is absent by design: it stopped being a view in T760 and is now the
 * ⌘I dialog trigger in {@link TopBarActions}, so it claims no slot here.
 * Args one object (max-params 4); `useModifierShortcuts` reads the map via
 * latest-ref so rebuilds don't re-add the listener.
 */
function useViewShortcuts(a: {
  view: ViewMode
  onViewChange: (v: ViewMode) => void
  onToggleAgentsRail: () => void
  onToggleFinderRail: () => void
}): {
  modHeld: boolean
  agentsClick: () => void
  finderClick: () => void
} {
  const agentsClick = a.view === "agents" ? a.onToggleAgentsRail : () => a.onViewChange("agents")
  const finderClick = a.view === "finder" ? a.onToggleFinderRail : () => a.onViewChange("finder")

  const modHeld = useModifierShortcuts({ l: agentsClick, k: finderClick })

  return { modHeld, agentsClick, finderClick }
}

/** Per-agent view switcher (Agents · Finder · Costs). Costs
 *  is dev-mode only. Extracted from {@link TopBar} so its tab cluster + the
 *  `devMode` gate don't count against the bar's complexity budget. Each tab
 *  carries a tooltip since the names aren't obvious to a first-time user.
 *
 *  The Agents tab is DUAL-PURPOSE: it switches to the agents view, and once
 *  that view is already active, a further click collapses ↔ expands its list
 *  rail. This is the activity-bar idiom (macOS, VS Code) — clicking the icon of
 *  the panel you are already in hides it — and it replaces the two dedicated
 *  show/hide sidebar buttons that used to sit inside the view itself.
 */
function ViewTabs({
  view,
  onViewChange,
  devMode,
  agentsRailOpen,
  onToggleAgentsRail,
  finderRailOpen,
  onToggleFinderRail,
}: {
  view: ViewMode
  onViewChange: (v: ViewMode) => void
  devMode: boolean
  agentsRailOpen: boolean
  onToggleAgentsRail: () => void
  finderRailOpen: boolean
  onToggleFinderRail: () => void
}) {
  const onAgents = view === "agents"
  const onFinder = view === "finder"

  // ⌘/Ctrl shortcuts (L Agents · K Finder) share ONE handler each with the tab
  // click — the hook builds them, so a shortcut and a click are the same action.
  // Both stay bound on their own view (toggle the rail).
  const { modHeld, agentsClick, finderClick } = useViewShortcuts({
    view,
    onViewChange,
    onToggleAgentsRail,
    onToggleFinderRail,
  })

  return (
    // One of the two deliberate exceptions to "no borders" (the other is the
    // light/dark toggle): the pill group needs its own outline to read as a
    // segmented control rather than three loose glyphs. `--border-strong` is
    // the opt-in token that survives the transparent `--border`.
    <div className="flex w-full flex-col items-center gap-0.5 rounded-lg border border-(--border-strong) bg-muted/60 p-0.5">
      <Tip
        title="Agents"
        body={
          onAgents
            ? `Click again to ${agentsRailOpen ? "hide" : "show"} the agent list.`
            : "Chat with this fleet. Each agent is a separate conversation or task it can run in parallel."
        }
        side="right"
      >
        <ViewTab
          active={onAgents}
          // Same handler as the ⌘/Ctrl+L shortcut: navigate here, or toggle the
          // list rail once already here.
          onClick={agentsClick}
          // Only meaningful while this tab controls the rail; on the other
          // views the button is a plain navigation control.
          expanded={onAgents ? agentsRailOpen : undefined}
          icon={MessagesSquare}
          label="Agents"
          // L (not C — C is the new-agent shortcut in AgentActions).
          hint="L"
          hintShown={modHeld}
        />
      </Tip>
      <Tip
        title="Finder"
        body={
          onFinder
            ? `Click again to ${finderRailOpen ? "hide" : "show"} the explorer.`
            : "Browse this fleet's files — the project folder it lives in and is confined to."
        }
        side="right"
      >
        <ViewTab
          active={onFinder}
          onClick={finderClick}
          expanded={onFinder ? finderRailOpen : undefined}
          icon={FolderTree}
          label="Finder"
          // K navigates to Finder, or toggles the explorer once already here.
          hint="K"
          hintShown={modHeld}
        />
      </Tip>
      {devMode && (
        <Tip
          title="Cost Analysis"
          body="Per-tick cache efficiency, culprit attribution, and spend breakdown charts."
          side="right"
        >
          <ViewTab
            active={view === "costs"}
            onClick={() => onViewChange("costs")}
            icon={BarChart3}
            label="Costs"
          />
        </Tip>
      )}
    </div>
  )
}
