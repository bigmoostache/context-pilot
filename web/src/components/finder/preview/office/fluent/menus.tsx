import { createContext, use, useEffect, useRef, useState } from "react"
import type {
  CSSProperties,
  KeyboardEvent as ReactKeyboardEvent,
  ReactNode,
  RefObject,
} from "react"
import {
  Checkmark16Regular,
  ChevronDown12Regular,
  ChevronRight16Regular,
} from "@fluentui/react-icons"
import { useBelowAnchor } from "./anchor"
import { STANDARD_COLORS, THEME_COLORS, type Cmd } from "./tokens"

/** Closes the whole open menu tree; set by the opener (DropBtn/SplitBtn). */
const CloseContext = createContext<() => void>(() => {
  // No opener above: nothing to close.
})

/**
 * Popover under its opener. Closes on a pointerdown outside `anchorRef`; the
 * opener's `onClose` hands focus back to it.
 */
function Popover({
  anchorRef,
  onClose,
  children,
}: {
  anchorRef: RefObject<HTMLElement | null>
  onClose: () => void
  children: ReactNode
}) {
  const below = useBelowAnchor(anchorRef, true)
  useEffect(() => {
    const down = (e: PointerEvent) => {
      if (anchorRef.current && !anchorRef.current.contains(e.target as Node)) onClose()
    }
    document.addEventListener("pointerdown", down)
    return () => document.removeEventListener("pointerdown", down)
  }, [anchorRef, onClose])
  if (!below) return null
  return (
    <CloseContext value={onClose}>
      <div style={{ ...popoverStyle, position: "fixed", top: below.top, left: below.left }}>
        {children}
      </div>
    </CloseContext>
  )
}

/** Index the key moves focus to among `count` items, or null when it isn't a nav key. */
function navTarget(key: string, i: number, count: number): number | null {
  switch (key) {
    case "ArrowDown": {
      return (i + 1) % count
    }
    case "ArrowUp": {
      return (i - 1 + count) % count
    }
    case "Home": {
      return 0
    }
    case "End": {
      return count - 1
    }
    default: {
      return null
    }
  }
}

/** ↑/↓/Home/End move focus between the menu's items; Esc closes the menu. */
function menuKeys(e: ReactKeyboardEvent<HTMLDivElement>, close: () => void) {
  if (e.key === "Escape") {
    e.preventDefault()
    e.stopPropagation()
    close()
    return
  }
  const items = [
    ...e.currentTarget.querySelectorAll<HTMLElement>(
      ":scope > [role^=menuitem]:not([disabled]), :scope > div > [role=menuitem]",
    ),
  ]
  const target = navTarget(
    e.key,
    items.indexOf(document.activeElement as HTMLElement),
    items.length,
  )
  if (target === null) return
  e.preventDefault()
  items[target]?.focus()
}

/** A vertical menu; its first enabled item takes focus when it opens. */
export function Menu({ label, children }: { label: string; children: ReactNode }) {
  const close = use(CloseContext)
  return (
    <div
      role="menu"
      aria-label={label}
      tabIndex={-1}
      ref={(el) => el?.querySelector<HTMLElement>("[role^=menuitem]:not([disabled])")?.focus()}
      onKeyDown={(e) => menuKeys(e, close)}
      style={{ display: "flex", flexDirection: "column", minWidth: 180, outline: "none" }}
    >
      {children}
    </div>
  )
}

/**
 * One menu entry, bound to `cmd` (enabled/checked from its state) or to
 * `onSelect`. Selecting closes the whole menu tree.
 */
export function MenuItem<A>({
  label,
  icon,
  cmd,
  args,
  onSelect,
  checked,
  shortcut,
}: {
  label: string
  icon?: ReactNode
  cmd?: Cmd<unknown, A> | undefined
  args?: A | undefined
  onSelect?: (() => void) | undefined
  checked?: boolean | undefined
  shortcut?: string | undefined
}) {
  const close = use(CloseContext)
  const enabled = cmd ? cmd.state.enabled : true
  const on = checked ?? cmd?.state.active
  return (
    <button
      type="button"
      role={on === undefined ? "menuitem" : "menuitemcheckbox"}
      aria-checked={on}
      disabled={!enabled}
      onClick={() => {
        close()
        if (cmd) void cmd.execute(args as A)
        else onSelect?.()
      }}
      style={{ ...itemStyle, opacity: enabled ? 1 : 0.4 }}
    >
      <span style={iconSlot}>{on ? <Checkmark16Regular /> : icon}</span>
      <span style={{ flex: 1 }}>{label}</span>
      {shortcut ? (
        <span style={{ color: "var(--of-ink-muted)", fontSize: 12 }}>{shortcut}</span>
      ) : null}
    </button>
  )
}

export function MenuSeparator() {
  return (
    <div role="separator" style={{ height: 1, margin: "4px 0", background: "var(--of-line)" }} />
  )
}

/** Submenu entry: → or click opens it on the right, ← or Esc closes it. */
export function MenuSub({
  label,
  icon,
  children,
}: {
  label: string
  icon?: ReactNode
  children: ReactNode
}) {
  const [open, setOpen] = useState(false)
  const boxRef = useRef<HTMLDivElement>(null)
  const closeSub = () => {
    setOpen(false)
    boxRef.current?.querySelector<HTMLElement>("[role=menuitem]")?.focus()
  }
  return (
    <div ref={boxRef} style={{ position: "relative" }}>
      <button
        type="button"
        role="menuitem"
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen(!open)}
        onKeyDown={(e) => {
          if (e.key !== "ArrowRight") return
          e.preventDefault()
          setOpen(true)
        }}
        style={itemStyle}
      >
        <span style={iconSlot}>{icon}</span>
        <span style={{ flex: 1 }}>{label}</span>
        <ChevronRight16Regular />
      </button>
      {open ? (
        <div
          role="presentation"
          onKeyDown={(e) => {
            if (e.key !== "ArrowLeft" && e.key !== "Escape") return
            e.preventDefault()
            e.stopPropagation()
            closeSub()
          }}
          style={{ ...popoverStyle, top: -4, left: "100%" }}
        >
          {children}
        </div>
      ) : null}
    </div>
  )
}

/** Opener state shared by DropBtn and SplitBtn: open flag, anchor, focus return. */
function useOpener() {
  const [open, setOpen] = useState(false)
  const anchorRef = useRef<HTMLSpanElement>(null)
  const close = () => {
    setOpen(false)
    anchorRef.current?.querySelector<HTMLElement>("button")?.focus()
  }
  return [open, setOpen, anchorRef, close] as const
}

/** Button that only opens `menu` (Line Spacing, Breaks…). */
export function DropBtn({
  label,
  icon,
  text,
  menu,
}: {
  label: string
  icon: ReactNode
  text?: string | undefined
  menu: ReactNode
}) {
  const [open, setOpen, anchorRef, close] = useOpener()
  return (
    <span ref={anchorRef} style={anchorStyle}>
      <button
        type="button"
        title={label}
        aria-label={label}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen(!open)}
        style={{ ...btnStyle, background: open ? "var(--of-pressed)" : "transparent" }}
      >
        {icon}
        {text ? <span style={{ fontSize: 14 }}>{text}</span> : null}
        <ChevronDown12Regular />
      </button>
      {open ? (
        <Popover anchorRef={anchorRef} onClose={close}>
          {menu}
        </Popover>
      ) : null}
    </span>
  )
}

/**
 * Split button: the main half runs `cmd` with `args` (the last pick, held by
 * the caller), the chevron opens `menu` (colour picker, gallery…).
 */
export function SplitBtn<A>({
  cmd,
  args,
  icon,
  menu,
}: {
  cmd: Cmd<unknown, A>
  args: A
  icon: ReactNode
  menu: ReactNode
}) {
  const [open, setOpen, anchorRef, close] = useOpener()
  const enabled = cmd.state.enabled
  const opacity = enabled ? 1 : 0.4
  return (
    <span ref={anchorRef} style={anchorStyle}>
      <button
        type="button"
        title={cmd.label}
        aria-label={cmd.label}
        disabled={!enabled}
        onClick={() => void cmd.execute(args)}
        style={{ ...btnStyle, padding: 0, width: 28, opacity }}
      >
        {icon}
      </button>
      <button
        type="button"
        title={`${cmd.label} options`}
        aria-label={`${cmd.label} options`}
        aria-haspopup="menu"
        aria-expanded={open}
        disabled={!enabled}
        onClick={() => setOpen(!open)}
        style={{
          ...btnStyle,
          padding: 0,
          width: 16,
          opacity,
          background: open ? "var(--of-pressed)" : "transparent",
        }}
      >
        <ChevronDown12Regular />
      </button>
      {open ? (
        <Popover anchorRef={anchorRef} onClose={close}>
          {menu}
        </Popover>
      ) : null}
    </span>
  )
}

/**
 * Colour grid (rows of swatches, optional "none" entry on top). Picking calls
 * `onPick(hex)` with RGB hex without `#`, or `null` for none, and closes.
 */
export function ColorPicker({
  label,
  onPick,
  colors = [THEME_COLORS, STANDARD_COLORS],
  noneLabel,
}: {
  label: string
  onPick: (hex: string | null) => void
  colors?: readonly (readonly string[])[]
  noneLabel?: string | undefined
}) {
  const close = use(CloseContext)
  const pick = (hex: string | null) => {
    close()
    onPick(hex)
  }
  const first = colors[0]?.[0]
  return (
    <div
      role="menu"
      aria-label={label}
      style={{ display: "flex", flexDirection: "column", gap: 6, padding: 4 }}
    >
      {noneLabel ? (
        <button type="button" role="menuitem" onClick={() => pick(null)} style={itemStyle}>
          <span style={iconSlot} />
          {noneLabel}
        </button>
      ) : null}
      {colors.map((row) => (
        <div key={row.join(",")} style={{ display: "flex", gap: 4 }}>
          {row.map((hex) => (
            <button
              key={hex}
              type="button"
              role="menuitem"
              title={`#${hex}`}
              aria-label={`#${hex}`}
              ref={(el) => {
                if (hex === first) el?.focus()
              }}
              onClick={() => pick(hex)}
              style={{ ...swatchStyle, background: `#${hex}` }}
            />
          ))}
        </div>
      ))}
    </div>
  )
}

/** Grid of large previews (styles, layouts); `value` marks the current one. */
export function Gallery<V extends string>({
  label,
  items,
  value,
  onPick,
  columns = 4,
}: {
  label: string
  items: readonly { value: V; label: string; preview: ReactNode }[]
  value?: V | undefined
  onPick: (value: V) => void
  columns?: number
}) {
  const close = use(CloseContext)
  return (
    <div
      role="listbox"
      aria-label={label}
      style={{
        display: "grid",
        gridTemplateColumns: `repeat(${String(columns)}, auto)`,
        gap: 4,
        padding: 4,
      }}
    >
      {items.map((it) => (
        <button
          key={it.value}
          type="button"
          role="option"
          aria-selected={it.value === value}
          title={it.label}
          onClick={() => {
            close()
            onPick(it.value)
          }}
          style={{
            ...galleryItemStyle,
            background: it.value === value ? "var(--of-pressed)" : "var(--of-surface)",
          }}
        >
          {it.preview}
        </button>
      ))}
    </div>
  )
}

const anchorStyle: CSSProperties = { position: "relative", display: "inline-flex", flex: "none" }

const popoverStyle: CSSProperties = {
  position: "absolute",
  zIndex: 30,
  padding: 4,
  background: "var(--of-surface)",
  borderRadius: "var(--of-radius)",
  boxShadow: "var(--of-card-shadow)",
  fontFamily: "var(--of-font)",
}

const itemStyle: CSSProperties = {
  width: "100%",
  minHeight: 32,
  display: "flex",
  alignItems: "center",
  gap: 8,
  padding: "0 8px",
  border: "none",
  borderRadius: 4,
  background: "transparent",
  color: "var(--of-ink)",
  font: "inherit",
  fontSize: 14,
  textAlign: "left",
  whiteSpace: "nowrap",
  cursor: "pointer",
}

const iconSlot: CSSProperties = { width: 20, display: "inline-flex", justifyContent: "center" }

const btnStyle: CSSProperties = {
  height: "var(--of-btn-h)",
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  gap: 4,
  padding: "0 6px",
  border: "none",
  borderRadius: 4,
  background: "transparent",
  color: "var(--of-ink)",
  font: "inherit",
  cursor: "pointer",
}

const swatchStyle: CSSProperties = {
  width: 18,
  height: 18,
  padding: 0,
  border: "1px solid var(--of-line)",
  borderRadius: 2,
  cursor: "pointer",
}

const galleryItemStyle: CSSProperties = {
  minWidth: 72,
  minHeight: 48,
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  padding: 4,
  border: "1px solid var(--of-line)",
  borderRadius: 4,
  color: "var(--of-ink)",
  font: "inherit",
  cursor: "pointer",
}
