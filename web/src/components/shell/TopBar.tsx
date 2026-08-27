import { useRef, useState } from "react"
import {
  MessagesSquare,
  FolderTree,
  BarChart3,
  Plus,
  Search,
  SlidersHorizontal,
} from "lucide-react"
import { ThemeToggle } from "./widgets/ThemeToggle"
import { AgentSwitcher } from "./widgets/AgentSwitcher"
import { UsageButton } from "./widgets/UsageButton"
import { HintBadge } from "./chrome/HintBadge"
import { ConfigModal } from "./config/ConfigModal"
import { ProfileModal } from "./widgets/ProfileModal"
import { UserMenu } from "./widgets/UserMenu"
import { UsersDialog } from "@/components/auth/UsersDialog"
import { Tip } from "@/components/ui/tip"
import { useDevMode } from "@/lib/providers/toggles/devMode"
import { useModifierShortcuts } from "@/lib/support/a11y"
import type { Agent, ViewMode } from "@/lib/types"
import { cn } from "@/lib/utils"

interface TopBarProps {
  view: ViewMode
  onViewChange: (v: ViewMode) => void
  activeAgentId: string
  onSwitchAgent: (id: string) => void
  agents: Agent[]
  /** Whether the threads view's list rail is open — drives the Threads tab's
   *  `aria-expanded` and its tooltip wording while that view is active. */
  threadsRailOpen: boolean
  /** Collapse ↔ expand the threads list rail. Fired by clicking the Threads
   *  tab when threads is ALREADY the active view. */
  onToggleThreadsRail: () => void
  /** Open the New Thread dialog. Rail-hosted, so shown only on the threads view. */
  onNewThread: () => void
  /** Open the thread search palette. Same gating as {@link onNewThread}. */
  onSearchThreads: () => void
  /** Whether the settings view's category rail is open — drives the Settings
   *  tab's `aria-expanded` and its tooltip wording while that view is active. */
  settingsRailOpen: boolean
  /** Collapse ↔ expand the settings category rail. Fired by clicking the
   *  Settings tab when settings is ALREADY the active view. */
  onToggleSettingsRail: () => void
  /** Whether the finder view's explorer rail is open — drives the Finder tab's
   *  `aria-expanded` and its tooltip wording while that view is active. */
  finderRailOpen: boolean
  /** Collapse ↔ expand the finder explorer rail. Fired by clicking the Finder
   *  tab when finder is ALREADY the active view. */
  onToggleFinderRail: () => void
}

/** Slim macOS-style side rail — app mark (→ fleet), workspace switcher,
 *  per-agent view tabs (Threads · Finder), theme, usage, account. Vertical:
 *  it hugs the left edge of the window, full height, chrome stacked top to
 *  bottom with the account cluster pinned to the floor. */
export function TopBar({
  view,
  onViewChange,
  activeAgentId,
  onSwitchAgent,
  agents,
  threadsRailOpen,
  onToggleThreadsRail,
  onNewThread,
  onSearchThreads,
  settingsRailOpen,
  onToggleSettingsRail,
  finderRailOpen,
  onToggleFinderRail,
}: TopBarProps) {
  const activeAgent = agents.find((a) => a.id === activeAgentId) ?? agents[0]
  // OAuth usage/login widget applies ONLY to the OAuth providers (Bearer token
  // via vault "claude_oauth"). The `anthropic` provider authenticates by
  // x-api-key (ANTHROPIC_API_KEY) and has no OAuth login, so it's excluded.
  const isClaudeOAuth =
    activeAgent?.provider === "claudecode" || activeAgent?.provider === "claudecodev2"
  const inFleet = view === "fleet"
  const { devMode } = useDevMode()
  const [configOpen, setConfigOpen] = useState(false)
  const [profileOpen, setProfileOpen] = useState(false)
  const [usersOpen, setUsersOpen] = useState(false)

  // ⌘/Ctrl+A opens the workspace switcher (T646): the ref lets the shortcut
  // click the dropdown trigger, so key and click are one action. The `a`
  // binding yields inside text fields (selectAllWouldBeNoop), so Select-All is
  // never stolen; bound in the always-mounted bar so it works from every view.
  const switcherRef = useRef<HTMLButtonElement>(null)
  useModifierShortcuts({ a: () => switcherRef.current?.click() })

  return (
    <>
      {/* No border, no fill: the rail sits directly on `--background` (the old
          `.vibrancy` + `border-r` are gone, so nothing is painted behind it and
          the dropped backdrop-filter costs no depth). `p-2` insets all four
          sides; `gap-3` is separate rhythm BETWEEN items, not an edge inset. */}
      <header className="flex w-14 shrink-0 flex-col items-center gap-3 p-2">
        <Tip
          title="Workspace"
          body="The agent you're working in — one agent, one folder. Switch or manage from here."
          side="right"
        >
          <AgentSwitcher
            rail
            triggerRef={switcherRef}
            agents={agents}
            activeId={inFleet ? undefined : activeAgentId}
            onManageAgents={() => onViewChange("fleet")}
            onSwitch={
              inFleet
                ? (id) => {
                    onSwitchAgent(id)
                    onViewChange("threads")
                  }
                : onSwitchAgent
            }
          />
        </Tip>

        {!inFleet && (
          <ViewTabs
            view={view}
            onViewChange={onViewChange}
            devMode={devMode}
            threadsRailOpen={threadsRailOpen}
            onToggleThreadsRail={onToggleThreadsRail}
            settingsRailOpen={settingsRailOpen}
            onToggleSettingsRail={onToggleSettingsRail}
            finderRailOpen={finderRailOpen}
            onToggleFinderRail={onToggleFinderRail}
          />
        )}

        {/* The threads view's own list actions. Gated on the threads view being
            SELECTED: they act on the thread list, so on Finder or Costs they
            would be controls for a surface that is not on screen. */}
        {!inFleet && view === "threads" && (
          <ThreadActions onNewThread={onNewThread} onSearchThreads={onSearchThreads} />
        )}

        <TopBarActions
          isClaudeOAuth={isClaudeOAuth}
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

/** Right-side controls cluster: theme toggle, agent-config gear, Claude Usage
 *  button, and the account avatar menu. Extracted from {@link TopBar} so both
 *  components stay within the P8 complexity budget. */
function TopBarActions({
  isClaudeOAuth,
  setConfigOpen,
  setProfileOpen,
  setUsersOpen,
}: {
  isClaudeOAuth: boolean
  setConfigOpen: (v: boolean) => void
  setProfileOpen: (v: boolean) => void
  setUsersOpen: (v: boolean) => void
}) {
  return (
    <div className="mt-auto flex flex-col items-center gap-3">
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
 * The threads view's list actions — New thread · Search — as a standalone
 * cluster under the view tabs.
 *
 * DELIBERATELY UNLIKE {@link ViewTabs} ABOVE IT. That is a segmented selector,
 * so it carries a border and a fill to read as one control with a chosen
 * member. These two are independent commands with no selected state, so a
 * frame around them would claim a relationship they do not have. They get no
 * border and no fill — only a hover wash, the same affordance every other bare
 * glyph in the rail uses.
 *
 * They live here rather than in the thread list because the list can be
 * collapsed (the Threads tab hides it), and an action that disappears with the
 * panel it acts on is an action the user has to restore the panel to reach.
 *
 * The ⌘/Ctrl shortcuts are bound HERE rather than in the shell, so the key
 * listener mounts and unmounts with the buttons — see {@link
 * useModifierShortcuts}, which explains why that matters.
 */
function ThreadActions({
  onNewThread,
  onSearchThreads,
}: {
  onNewThread: () => void
  onSearchThreads: () => void
}) {
  const modHeld = useModifierShortcuts({ c: onNewThread, p: onSearchThreads })

  return (
    <div className="flex w-full flex-col items-center gap-0.5">
      <Tip title="New thread" body="Start a fresh conversation or task in this agent." side="right">
        <RailAction
          label="New thread"
          icon={Plus}
          hint="C"
          hintShown={modHeld}
          onClick={onNewThread}
        />
      </Tip>
      <Tip
        title="Search threads"
        body="Find a thread by name, or search across every message in this agent."
        side="right"
      >
        <RailAction
          label="Search threads"
          icon={Search}
          hint="P"
          hintShown={modHeld}
          onClick={onSearchThreads}
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
 * Build the three view-tab click handlers AND bind them as ⌘/Ctrl shortcuts —
 * one definition for both, so a shortcut is "as if you clicked the tab". Each
 * is DUAL-PURPOSE like the tab click: off the view navigate there, on the view
 * toggle that view's rail (L/K/I stay live on their own view). L not C drives
 * Threads — C is the new-thread shortcut in sibling {@link ThreadActions}; I not
 * S drives Settings (⌘S is the browser's Save; the user asked for ⌘I, T639).
 * All bound, so one `modHeld` gates every hint. Args one object (max-params 4);
 * `useModifierShortcuts` reads the map via latest-ref so rebuilds don't re-add
 * the listener.
 */
function useViewShortcuts(a: {
  view: ViewMode
  onViewChange: (v: ViewMode) => void
  onToggleThreadsRail: () => void
  onToggleFinderRail: () => void
  onToggleSettingsRail: () => void
}): {
  modHeld: boolean
  threadsClick: () => void
  finderClick: () => void
  settingsClick: () => void
} {
  const threadsClick =
    a.view === "threads" ? a.onToggleThreadsRail : () => a.onViewChange("threads")
  const finderClick = a.view === "finder" ? a.onToggleFinderRail : () => a.onViewChange("finder")
  const settingsClick =
    a.view === "settings" ? a.onToggleSettingsRail : () => a.onViewChange("settings")

  const modHeld = useModifierShortcuts({ l: threadsClick, k: finderClick, i: settingsClick })

  return { modHeld, threadsClick, finderClick, settingsClick }
}

/** Per-agent view switcher (Threads · Finder · Costs). Costs
 *  is dev-mode only. Extracted from {@link TopBar} so its tab cluster + the
 *  `devMode` gate don't count against the bar's complexity budget. Each tab
 *  carries a tooltip since the names aren't obvious to a first-time user.
 *
 *  The Threads tab is DUAL-PURPOSE: it switches to the threads view, and once
 *  that view is already active, a further click collapses ↔ expands its list
 *  rail. This is the activity-bar idiom (macOS, VS Code) — clicking the icon of
 *  the panel you are already in hides it — and it replaces the two dedicated
 *  show/hide sidebar buttons that used to sit inside the view itself.
 */
function ViewTabs({
  view,
  onViewChange,
  devMode,
  threadsRailOpen,
  onToggleThreadsRail,
  settingsRailOpen,
  onToggleSettingsRail,
  finderRailOpen,
  onToggleFinderRail,
}: {
  view: ViewMode
  onViewChange: (v: ViewMode) => void
  devMode: boolean
  threadsRailOpen: boolean
  onToggleThreadsRail: () => void
  settingsRailOpen: boolean
  onToggleSettingsRail: () => void
  finderRailOpen: boolean
  onToggleFinderRail: () => void
}) {
  const onThreads = view === "threads"
  const onSettings = view === "settings"
  const onFinder = view === "finder"

  // ⌘/Ctrl shortcuts (L Threads · K Finder · I Settings) share ONE handler each
  // with the tab click — the hook builds them, so a shortcut and a click are
  // the same action. All three stay bound on their own view (toggle the rail).
  const { modHeld, threadsClick, finderClick, settingsClick } = useViewShortcuts({
    view,
    onViewChange,
    onToggleThreadsRail,
    onToggleFinderRail,
    onToggleSettingsRail,
  })

  return (
    // One of the two deliberate exceptions to "no borders" (the other is the
    // light/dark toggle): the pill group needs its own outline to read as a
    // segmented control rather than three loose glyphs. `--border-strong` is
    // the opt-in token that survives the transparent `--border`.
    <div className="flex w-full flex-col items-center gap-0.5 rounded-lg border border-(--border-strong) bg-muted/60 p-0.5">
      <Tip
        title="Threads"
        body={
          onThreads
            ? `Click again to ${threadsRailOpen ? "hide" : "show"} the thread list.`
            : "Chat with this agent. Each thread is a separate conversation or task it can run in parallel."
        }
        side="right"
      >
        <ViewTab
          active={onThreads}
          // Same handler as the ⌘/Ctrl+L shortcut: navigate here, or toggle the
          // list rail once already here.
          onClick={threadsClick}
          // Only meaningful while this tab controls the rail; on the other
          // views the button is a plain navigation control.
          expanded={onThreads ? threadsRailOpen : undefined}
          icon={MessagesSquare}
          label="Threads"
          // L (not C — C is the new-thread shortcut in ThreadActions).
          hint="L"
          hintShown={modHeld}
        />
      </Tip>
      <Tip
        title="Finder"
        body={
          onFinder
            ? `Click again to ${finderRailOpen ? "hide" : "show"} the explorer.`
            : "Browse this agent's files — the project folder it lives in and is confined to."
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
      {/* LAST in the group, deliberately: this is the agent's configuration,
          reached once — not a surface the user lives in like Threads or
          Finder. Same dual-purpose click as the Threads tab above, so the
          two behave identically and the idiom only has to be learnt once. */}
      <Tip
        title="Settings"
        body={
          onSettings
            ? `Click again to ${settingsRailOpen ? "hide" : "show"} the category list.`
            : "Configure this agent — its identity, its model, and its service vitals."
        }
        side="right"
      >
        <ViewTab
          active={onSettings}
          onClick={settingsClick}
          expanded={onSettings ? settingsRailOpen : undefined}
          icon={SlidersHorizontal}
          label="Settings"
          // I navigates to Settings, or toggles the category rail once here
          // (⌘S is the browser Save shortcut; the user chose ⌘I, T639).
          hint="I"
          hintShown={modHeld}
        />
      </Tip>
    </div>
  )
}
