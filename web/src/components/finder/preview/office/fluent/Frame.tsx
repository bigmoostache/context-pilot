import { createContext, use, useEffect, useMemo, useState } from "react"
import type { CSSProperties, ReactNode } from "react"
import { createPortal } from "react-dom"
import { ArrowLeft20Regular } from "@fluentui/react-icons"
import { officeVars, type OfficeApp } from "./tokens"
import { SearchBox, TabRow, type OfficeTab, type SearchItem } from "./TabRow"

/** DOM slots rendered by the shell (outside the engine), filled by the ribbon. */
interface Slots {
  search: HTMLElement | null
  status: HTMLElement | null
  setSearch: (el: HTMLElement | null) => void
}

const SlotsContext = createContext<Slots | null>(null)

/**
 * Root of an Office lookalike: sets the app's `--of-*` vars, lays out title
 * bar → engine → status bar, and owns the status slot that `OfficeRibbon`
 * portals into (so the status bar keeps engine command access).
 * `bodyStyle` carries app CSS vars for engine-drawn areas (canvas, panes).
 */
export function OfficeShell({
  app,
  bodyStyle,
  children,
}: {
  app: OfficeApp
  bodyStyle?: CSSProperties | undefined
  children: ReactNode
}) {
  const [search, setSearch] = useState<HTMLElement | null>(null)
  const [status, setStatus] = useState<HTMLElement | null>(null)
  const slots = useMemo(() => ({ search, status, setSearch }), [search, status])
  return (
    <SlotsContext value={slots}>
      <div style={{ ...officeVars(app), ...shellStyle, ...bodyStyle }}>
        <div style={{ flex: 1, minHeight: 0, display: "flex", flexDirection: "column" }}>
          {children}
        </div>
        <div ref={setStatus} style={{ flex: "none" }} />
      </div>
    </SlotsContext>
  )
}

/** Save state shown as a chip after the document title. */
export type TitleSaveState = "idle" | "saving" | "saved" | "error" | "conflict"

const SAVE_LABEL: Record<TitleSaveState, string> = {
  idle: "",
  saving: "Saving…",
  saved: "Saved",
  error: "Save failed",
  conflict: "Changed on disk",
}

/**
 * Title bar (48 px): app logo, document name, save chip, and a centred slot
 * that holds the command search when the ribbon is given `commands`.
 * No waffle, settings or account controls (removed by user decision).
 */
export function OfficeTitleBar({
  title,
  logo,
  saveState = "idle",
}: {
  title: string
  logo: ReactNode
  saveState?: TitleSaveState
}) {
  const slots = use(SlotsContext)
  const chip = SAVE_LABEL[saveState]
  return (
    <div style={titleBarStyle}>
      <div style={{ display: "flex", alignItems: "center", gap: 10, minWidth: 0, flex: 1 }}>
        <span style={{ display: "inline-flex", flex: "none" }}>{logo}</span>
        <span style={titleTextStyle}>{title}</span>
        {chip ? (
          <span
            role="status"
            style={{
              fontSize: 12,
              color: saveState === "error" || saveState === "conflict" ? "#c50f1f" : "inherit",
            }}
          >
            {chip}
          </span>
        ) : null}
      </div>
      <div ref={slots?.setSearch} style={{ flex: "none", display: "flex" }} />
      <span style={{ flex: 1 }} />
    </div>
  )
}

/**
 * The tab row + ribbon card. Pass it as the engine's `toolbar` so its
 * controls resolve engine commands; it portals `commands` (search) into the
 * title bar and `status` into the shell's status slot. `layout` = "classic"
 * (two-row card, Word) or "single" (one-line card, Excel/PowerPoint web).
 * `backstage` adds a File tab that replaces the ribbon with a full panel.
 */
export function OfficeRibbon({
  layout,
  tabs,
  defaultTab,
  backstage,
  tabsRight,
  belowRibbon,
  commands,
  status,
}: {
  layout: "classic" | "single"
  tabs: readonly OfficeTab[]
  defaultTab?: string | undefined
  backstage?: ReactNode
  tabsRight?: ReactNode
  belowRibbon?: ReactNode
  commands?: readonly SearchItem[] | undefined
  status?: ReactNode
}) {
  const [active, setActive] = useState(defaultTab ?? tabs[0]?.id ?? "")
  const [collapsed, setCollapsed] = useState(false)
  const [fileOpen, setFileOpen] = useState(false)
  const tabList = backstage ? [{ id: FILE_TAB, label: "File" }, ...tabs] : tabs
  const current = tabs.find((t) => t.id === active) ?? tabs[0]

  const select = (id: string) => {
    if (id === FILE_TAB) {
      setFileOpen(true)
      return
    }
    setActive(id)
    setCollapsed(false)
  }

  return (
    <div style={{ flex: "none", fontFamily: "var(--of-font)", position: "relative", zIndex: 1 }}>
      <TabRow
        tabs={tabList}
        active={current?.id ?? ""}
        onSelect={select}
        onToggleCollapse={() => setCollapsed(!collapsed)}
        right={tabsRight}
      />
      {collapsed || !current ? null : <RibbonCard layout={layout} tab={current} />}
      {belowRibbon}
      {fileOpen && backstage ? (
        <Backstage onClose={() => setFileOpen(false)}>{backstage}</Backstage>
      ) : null}
      <Portals commands={commands} status={status} />
    </div>
  )
}

const FILE_TAB = "__file"

/** The white ribbon card on the chrome strip, holding the active tab's groups. */
function RibbonCard({ layout, tab }: { layout: "classic" | "single"; tab: OfficeTab }) {
  const classic = layout === "classic"
  return (
    <div style={stripStyle}>
      <div
        role="toolbar"
        aria-label={`${tab.label} ribbon`}
        style={{
          ...cardStyle,
          alignItems: classic ? "stretch" : "center",
          padding: classic ? "4px 8px" : "0 8px",
        }}
      >
        {tab.ribbon}
      </div>
    </div>
  )
}

/** Renders search and status into the shell's slots, outside the ribbon's DOM. */
function Portals({
  commands,
  status,
}: {
  commands?: readonly SearchItem[] | undefined
  status?: ReactNode
}) {
  const slots = use(SlotsContext)
  return (
    <>
      {commands && slots?.search
        ? createPortal(<SearchBox items={commands} />, slots.search)
        : null}
      {status && slots?.status ? createPortal(status, slots.status) : null}
    </>
  )
}

/** Full-height File panel over the ribbon and body; Esc or the back arrow closes it. */
function Backstage({ onClose, children }: { onClose: () => void; children: ReactNode }) {
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose()
    }
    document.addEventListener("keydown", key)
    return () => document.removeEventListener("keydown", key)
  }, [onClose])
  return (
    <div role="dialog" aria-label="File" style={backstageStyle}>
      <button
        type="button"
        aria-label="Back"
        title="Back"
        ref={(el) => el?.focus()}
        onClick={onClose}
        style={backStyle}
      >
        <ArrowLeft20Regular />
      </button>
      <div style={{ flex: 1, minWidth: 0, overflow: "auto" }}>{children}</div>
    </div>
  )
}

const shellStyle: CSSProperties = {
  height: "100%",
  minHeight: 0,
  display: "flex",
  flexDirection: "column",
  background: "var(--of-chrome)",
  color: "var(--of-ink)",
}

const titleBarStyle: CSSProperties = {
  height: "var(--of-title-h)",
  flex: "none",
  display: "flex",
  alignItems: "center",
  gap: 12,
  padding: "0 16px",
  background: "var(--of-chrome)",
  fontFamily: "var(--of-font)",
  fontSize: 14,
}

const titleTextStyle: CSSProperties = {
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  fontWeight: 600,
}

const stripStyle: CSSProperties = {
  height: "var(--of-ribbon-h)",
  display: "flex",
  padding: "0 8px",
  background: "var(--of-chrome)",
}

const cardStyle: CSSProperties = {
  flex: 1,
  minWidth: 0,
  display: "flex",
  gap: 2,
  overflowX: "auto",
  background: "var(--of-surface)",
  borderRadius: "var(--of-radius)",
  boxShadow: "var(--of-card-shadow)",
  boxSizing: "border-box",
}

const backstageStyle: CSSProperties = {
  position: "fixed",
  inset: 0,
  zIndex: 40,
  display: "flex",
  gap: 16,
  padding: 16,
  background: "var(--of-surface)",
  outline: "none",
}

const backStyle: CSSProperties = {
  width: 40,
  height: 40,
  flex: "none",
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  border: "none",
  borderRadius: 4,
  background: "transparent",
  color: "var(--of-ink)",
  cursor: "pointer",
}
