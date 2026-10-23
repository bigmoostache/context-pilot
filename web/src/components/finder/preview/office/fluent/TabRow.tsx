import { useEffect, useRef, useState } from "react"
import type { CSSProperties, ReactNode } from "react"
import { Search16Regular } from "@fluentui/react-icons"
import type { Cmd } from "./tokens"

/** One ribbon tab: `ribbon` is the card content shown while the tab is active. */
export interface OfficeTab {
  id: string
  label: string
  ribbon: ReactNode
}

/** A command reachable from the title-bar search box. */
export interface SearchItem {
  label: string
  keywords?: readonly string[] | undefined
  cmd: Cmd
}

/**
 * Office tab strip: active tab = 600 weight + 3 px accent underline. Click
 * switches, ←/→ move between tabs, double-click toggles the ribbon collapse.
 */
export function TabRow({
  tabs,
  active,
  onSelect,
  onToggleCollapse,
  right,
}: {
  tabs: readonly { id: string; label: string }[]
  active: string
  onSelect: (id: string) => void
  onToggleCollapse: () => void
  right?: ReactNode
}) {
  const tabsRef = useRef(new Map<string, HTMLButtonElement>())
  const move = (delta: number) => {
    const i = tabs.findIndex((t) => t.id === active)
    const next = tabs[(i + delta + tabs.length) % tabs.length]
    if (!next) return
    onSelect(next.id)
    tabsRef.current.get(next.id)?.focus()
  }
  return (
    <div style={tabRowStyle}>
      <div role="tablist" aria-label="Ribbon tabs" style={{ display: "flex", height: "100%" }}>
        {tabs.map((t) => (
          <Tab
            key={t.id}
            label={t.label}
            selected={t.id === active}
            refFn={(el) => {
              if (el) tabsRef.current.set(t.id, el)
              else tabsRef.current.delete(t.id)
            }}
            onClick={() => onSelect(t.id)}
            onDoubleClick={onToggleCollapse}
            onArrow={move}
          />
        ))}
      </div>
      <span style={{ flex: 1 }} />
      {right}
    </div>
  )
}

function Tab({
  label,
  selected,
  refFn,
  onClick,
  onDoubleClick,
  onArrow,
}: {
  label: string
  selected: boolean
  refFn: (el: HTMLButtonElement | null) => void
  onClick: () => void
  onDoubleClick: () => void
  onArrow: (delta: number) => void
}) {
  const [hover, setHover] = useState(false)
  return (
    <button
      ref={refFn}
      type="button"
      role="tab"
      aria-selected={selected}
      tabIndex={selected ? 0 : -1}
      onClick={onClick}
      onDoubleClick={onDoubleClick}
      onKeyDown={(e) => {
        if (e.key === "ArrowRight") onArrow(1)
        else if (e.key === "ArrowLeft") onArrow(-1)
      }}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        ...tabStyle,
        fontWeight: selected ? 600 : 400,
        color: selected ? "var(--of-ink)" : "var(--of-ink-tab)",
        background: hover ? "var(--of-hover)" : "transparent",
      }}
    >
      {label}
      {selected ? <span aria-hidden style={underlineStyle} /> : null}
    </button>
  )
}

/**
 * Title-bar command search. Typing filters `items` by label/keywords, Enter
 * runs the highlighted match, Esc clears. Option+Q (Alt+Q) focuses it, except
 * while focus is in a text field or the engine's editable surface.
 */
export function SearchBox({ items }: { items: readonly SearchItem[] }) {
  const [q, setQ] = useState("")
  const [hi, setHi] = useState(0)
  const ref = useRef<HTMLInputElement>(null)
  const matches = filterItems(items, q)

  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (!e.altKey || e.code !== "KeyQ" || isEditing(document.activeElement)) return
      e.preventDefault()
      ref.current?.focus()
    }
    document.addEventListener("keydown", key)
    return () => document.removeEventListener("keydown", key)
  }, [])

  const run = (item: SearchItem | undefined) => {
    if (!item?.cmd.state.enabled) return
    void item.cmd.execute()
    setQ("")
    ref.current?.blur()
  }

  return (
    <div style={{ position: "relative", width: 400 }}>
      <div style={searchStyle}>
        <Search16Regular style={{ color: "var(--of-ink-muted)", flex: "none" }} />
        <input
          ref={ref}
          type="search"
          aria-label="Search commands (Option+Q)"
          placeholder="Search (Option+Q)"
          value={q}
          onChange={(e) => {
            setQ(e.currentTarget.value)
            setHi(0)
          }}
          onKeyDown={(e) => {
            switch (e.key) {
              case "Enter": {
                run(matches[hi])
                break
              }
              case "Escape": {
                setQ("")
                break
              }
              case "ArrowDown": {
                e.preventDefault()
                setHi(Math.min(hi + 1, matches.length - 1))
                break
              }
              case "ArrowUp": {
                e.preventDefault()
                setHi(Math.max(hi - 1, 0))
                break
              }
              default:
            }
          }}
          style={searchInputStyle}
        />
      </div>
      {q && matches.length > 0 ? (
        <div role="listbox" aria-label="Commands" style={resultsStyle}>
          {matches.map((m, i) => (
            <button
              key={m.label}
              type="button"
              role="option"
              aria-selected={i === hi}
              disabled={!m.cmd.state.enabled}
              onPointerDown={(e) => e.preventDefault()}
              onClick={() => run(m)}
              style={{
                ...resultStyle,
                background: i === hi ? "var(--of-hover)" : "transparent",
                opacity: m.cmd.state.enabled ? 1 : 0.4,
              }}
            >
              {m.label}
            </button>
          ))}
        </div>
      ) : null}
    </div>
  )
}

function filterItems(items: readonly SearchItem[], q: string): SearchItem[] {
  const needle = q.trim().toLowerCase()
  if (!needle) return []
  return items
    .filter((it) =>
      [it.label, ...(it.keywords ?? [])].some((s) => s.toLowerCase().includes(needle)),
    )
    .slice(0, 8)
}

/** True when focus is in a field or contenteditable (the engine owns the keys). */
function isEditing(el: Element | null): boolean {
  if (!(el instanceof HTMLElement)) return false
  return el.isContentEditable || el.tagName === "INPUT" || el.tagName === "TEXTAREA"
}

const tabRowStyle: CSSProperties = {
  height: "var(--of-tabs-h)",
  flex: "none",
  display: "flex",
  alignItems: "center",
  padding: "0 8px",
  background: "var(--of-chrome)",
  fontFamily: "var(--of-font)",
}

/** Tab button from the row's top: Word web's is 31 px with the label 3 px above the row centre. */
const tabStyle: CSSProperties = {
  position: "relative",
  height: "var(--of-tab-h)",
  alignSelf: "flex-start",
  padding: "0 var(--of-tab-pad)",
  border: "none",
  borderRadius: 4,
  font: "inherit",
  fontSize: 14,
  cursor: "pointer",
}

const underlineStyle: CSSProperties = {
  position: "absolute",
  left: "var(--of-tab-pad)",
  right: "var(--of-tab-pad)",
  bottom: "var(--of-tab-line-bottom)",
  height: "var(--of-tab-line)",
  borderRadius: 2,
  background: "var(--of-accent)",
}

const searchStyle: CSSProperties = {
  height: 32,
  display: "flex",
  alignItems: "center",
  gap: 8,
  padding: "0 10px",
  background: "var(--of-search)",
  border: "1px solid var(--of-line)",
  borderRadius: 4,
  boxSizing: "border-box",
}

const searchInputStyle: CSSProperties = {
  flex: 1,
  minWidth: 0,
  border: "none",
  outline: "none",
  background: "transparent",
  color: "var(--of-ink)",
  font: "inherit",
  fontSize: 14,
}

const resultsStyle: CSSProperties = {
  position: "absolute",
  top: "calc(100% + 4px)",
  left: 0,
  right: 0,
  padding: 4,
  background: "var(--of-surface)",
  borderRadius: "var(--of-radius)",
  boxShadow: "var(--of-card-shadow)",
  zIndex: 30,
}

const resultStyle: CSSProperties = {
  width: "100%",
  minHeight: 32,
  display: "flex",
  alignItems: "center",
  padding: "0 10px",
  border: "none",
  borderRadius: 4,
  color: "var(--of-ink)",
  font: "inherit",
  fontSize: 14,
  textAlign: "left",
  cursor: "pointer",
}
