import { useEffect, useId, useRef, useState } from "react"
import type { CSSProperties, ReactNode } from "react"
import { ChevronDown12Regular } from "@fluentui/react-icons"
import type { Cmd } from "./tokens"

/**
 * One ribbon command button. Small = 32×32 icon (single-line ribbon and the
 * classic two-row grid); large = 56 px wide icon over text (Paste). Enabled and
 * pressed come from `cmd.state`, so the button never drifts from the engine;
 * `aria-pressed` is only set for commands that report `active` (toggles).
 */
export function Btn<A>({
  cmd,
  args,
  icon,
  size = "sm",
  text,
  shortcut,
}: {
  cmd: Cmd<unknown, A>
  args?: A | undefined
  icon: ReactNode
  size?: "sm" | "lg"
  text?: string | undefined
  shortcut?: string | undefined
}) {
  const { enabled, active } = cmd.state
  const title = shortcut ? `${cmd.label} (${shortcut})` : cmd.label
  const lg = size === "lg"
  return (
    <HoverButton
      title={title}
      enabled={enabled}
      pressed={active === true}
      ariaPressed={active}
      onClick={() => void cmd.execute(args as A)}
      style={{
        height: lg ? 66 : 32,
        minWidth: lg ? 56 : 32,
        flexDirection: lg ? "column" : "row",
        gap: lg ? 2 : 4,
        padding: text && !lg ? "0 8px" : 0,
      }}
    >
      {icon}
      {text ? <span style={{ fontSize: lg ? 12 : 14 }}>{text}</span> : null}
    </HoverButton>
  )
}

/** Vertical hairline between ribbon groups. */
export function Sep() {
  return (
    <span
      aria-hidden
      style={{
        width: 1,
        height: 20,
        flex: "none",
        background: "var(--of-line)",
        margin: "0 6px",
      }}
    />
  )
}

/**
 * A named cluster of controls. `rows` lays them out in the classic two-row
 * grid (Word Home); without it they sit on one line. A separator follows
 * unless `last`.
 */
export function RibbonGroup({
  label,
  rows,
  last = false,
  children,
}: {
  label: string
  rows?: readonly [ReactNode, ReactNode] | undefined
  last?: boolean
  children?: ReactNode
}) {
  return (
    <>
      <div
        role="group"
        aria-label={label}
        style={{ display: "flex", alignItems: "center", gap: 2, flex: "none" }}
      >
        {children}
        {rows ? (
          <div style={{ display: "flex", flexDirection: "column", gap: 2 }}>
            <div style={rowStyle}>{rows[0]}</div>
            <div style={rowStyle}>{rows[1]}</div>
          </div>
        ) : null}
      </div>
      {last ? null : <Sep />}
    </>
  )
}

const rowStyle: CSSProperties = { display: "flex", alignItems: "center", gap: 2 }

/**
 * Typeable combo box (font name, font size, Excel name box). Shows the
 * command's current value; Enter commits the typed text, Esc reverts, ↓ or the
 * chevron opens the option list. Commit goes to `cmd.execute(toArgs(text))`
 * or, without a command, to `onCommit(text)`.
 */
export function Combo<V extends string | number, A>({
  label,
  width,
  options,
  cmd,
  toArgs,
  onCommit,
  value,
  renderOption,
}: {
  label: string
  width: number
  options: readonly V[]
  cmd?: Cmd<V, A> | undefined
  toArgs?: ((text: string) => A) | undefined
  onCommit?: ((text: string) => void) | undefined
  value?: V | undefined
  renderOption?: ((option: V) => ReactNode) | undefined
}) {
  const current = String(cmd?.state.value ?? value ?? "")
  const [draft, setDraft] = useState<string | null>(null)
  const [open, setOpen] = useState(false)
  const ref = useRef<HTMLDivElement>(null)
  const listId = useId()
  const enabled = cmd ? cmd.state.enabled : true

  const commit = (text: string) => {
    setDraft(null)
    setOpen(false)
    const t = text.trim()
    if (!t || t === current) return
    if (cmd && toArgs) void cmd.execute(toArgs(t))
    else onCommit?.(t)
  }

  useEffect(() => {
    if (!open) return
    const down = (e: PointerEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false)
    }
    document.addEventListener("pointerdown", down)
    return () => document.removeEventListener("pointerdown", down)
  }, [open])

  return (
    <div ref={ref} style={{ position: "relative", flex: "none", width }}>
      <div style={{ ...comboStyle, opacity: enabled ? 1 : 0.4 }}>
        <input
          role="combobox"
          aria-label={label}
          aria-controls={listId}
          aria-expanded={open}
          title={label}
          disabled={!enabled}
          value={draft ?? current}
          onChange={(e) => setDraft(e.currentTarget.value)}
          onFocus={(e) => e.currentTarget.select()}
          onBlur={() => setDraft(null)}
          onKeyDown={(e) => {
            switch (e.key) {
              case "Enter": {
                e.preventDefault()
                commit(draft ?? current)
                break
              }
              case "Escape": {
                setDraft(null)
                setOpen(false)
                break
              }
              case "ArrowDown": {
                e.preventDefault()
                setOpen(true)
                break
              }
              default:
            }
          }}
          style={inputStyle}
        />
        <button
          type="button"
          tabIndex={-1}
          aria-label={`${label} options`}
          disabled={!enabled}
          onClick={() => setOpen(!open)}
          style={chevronStyle}
        >
          <ChevronDown12Regular />
        </button>
      </div>
      {open ? (
        <div id={listId} role="listbox" aria-label={label} style={listStyle}>
          {options.map((o) => (
            <button
              key={String(o)}
              type="button"
              role="option"
              aria-selected={String(o) === current}
              onClick={() => commit(String(o))}
              style={{
                ...optionStyle,
                background: String(o) === current ? "var(--of-pressed)" : "transparent",
              }}
            >
              {renderOption ? renderOption(o) : String(o)}
            </button>
          ))}
        </div>
      ) : null}
    </div>
  )
}

/** Plain button with Office hover/pressed backgrounds (no CSS file needed). */
function HoverButton({
  title,
  enabled,
  pressed,
  ariaPressed,
  onClick,
  style,
  children,
}: {
  title: string
  enabled: boolean
  pressed: boolean
  ariaPressed?: boolean | undefined
  onClick: () => void
  style: CSSProperties
  children: ReactNode
}) {
  const [hover, setHover] = useState(false)
  let background = "transparent"
  if (pressed) background = "var(--of-pressed)"
  else if (hover && enabled) background = "var(--of-hover)"
  return (
    <button
      type="button"
      title={title}
      aria-label={title}
      aria-pressed={ariaPressed}
      disabled={!enabled}
      onClick={onClick}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      style={{
        display: "inline-flex",
        alignItems: "center",
        justifyContent: "center",
        flex: "none",
        border: "none",
        borderRadius: 4,
        color: "var(--of-ink)",
        font: "inherit",
        background,
        opacity: enabled ? 1 : 0.4,
        cursor: enabled ? "pointer" : "default",
        ...style,
      }}
    >
      {children}
    </button>
  )
}

const comboStyle: CSSProperties = {
  height: 28,
  display: "flex",
  alignItems: "center",
  background: "var(--of-surface)",
  border: "1px solid var(--of-line)",
  borderRadius: 4,
  boxSizing: "border-box",
}

const inputStyle: CSSProperties = {
  flex: 1,
  minWidth: 0,
  height: "100%",
  padding: "0 0 0 8px",
  border: "none",
  outline: "none",
  background: "transparent",
  color: "var(--of-ink)",
  font: "inherit",
  fontSize: 14,
}

const chevronStyle: CSSProperties = {
  width: 20,
  height: "100%",
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  border: "none",
  background: "transparent",
  color: "var(--of-ink-muted)",
  cursor: "pointer",
}

const listStyle: CSSProperties = {
  position: "absolute",
  top: "calc(100% + 2px)",
  left: 0,
  minWidth: "100%",
  maxHeight: 320,
  overflowY: "auto",
  padding: 4,
  background: "var(--of-surface)",
  borderRadius: "var(--of-radius)",
  boxShadow: "var(--of-card-shadow)",
  zIndex: 20,
}

const optionStyle: CSSProperties = {
  width: "100%",
  minHeight: 28,
  display: "flex",
  alignItems: "center",
  padding: "0 8px",
  border: "none",
  borderRadius: 4,
  color: "var(--of-ink)",
  font: "inherit",
  fontSize: 14,
  textAlign: "left",
  whiteSpace: "nowrap",
  cursor: "pointer",
}
