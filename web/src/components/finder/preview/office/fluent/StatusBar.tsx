import { useEffect, useRef, useState } from "react"
import type { CSSProperties, ReactNode } from "react"
import { Add16Regular, Subtract16Regular, ZoomFit16Regular } from "@fluentui/react-icons"
import type { Cmd } from "./tokens"

/**
 * Office for the web status bar: `--of-status-h` tall, 12px muted text on the
 * chrome background, `left` items flush left and `right` items flush right.
 * Reads every colour from the `--of-*` vars set by `officeVars(app)`.
 */
export function StatusBar({ left, right }: { left?: ReactNode; right?: ReactNode }) {
  return (
    <div
      role="status"
      style={{
        height: "var(--of-status-h)",
        flex: "none",
        display: "flex",
        alignItems: "center",
        gap: 2,
        padding: "0 8px",
        background: "var(--of-chrome)",
        color: "var(--of-ink-muted)",
        fontFamily: "var(--of-font)",
        fontSize: 12,
        whiteSpace: "nowrap",
      }}
    >
      {left}
      <span style={{ flex: 1 }} />
      {right}
    </div>
  )
}

/**
 * One status-bar entry. Plain text when it has neither `cmd` nor `onSelect`;
 * otherwise a button. With `cmd`, enabled/pressed come from the command state,
 * so a toggle can never drift from the engine.
 */
export function StatusItem({
  text,
  icon,
  onSelect,
  cmd,
}: {
  text?: string
  icon?: ReactNode
  onSelect?: () => void
  cmd?: Cmd
}) {
  if (!cmd && !onSelect) return <span style={{ padding: "0 6px" }}>{text}</span>
  const label = text ?? cmd?.label ?? ""
  return (
    <StatusButton
      label={label}
      enabled={cmd ? cmd.state.enabled : true}
      active={cmd?.state.active === true}
      onClick={() => {
        if (cmd) void cmd.execute()
        else onSelect?.()
      }}
    >
      {icon}
      {text ? <span>{text}</span> : null}
    </StatusButton>
  )
}

/**
 * − / slider / + / percentage zoom control. `cmd.state.value` is the scale
 * (1 = 100 %); `toArgs` maps a scale, or `"fit"`, to the engine's arguments.
 * Clicking the percentage opens the presets list, Fit included when `fit`.
 */
export function Zoom<A>({
  cmd,
  toArgs,
  min = 0.25,
  max = 4,
  step = 0.1,
  presets = [0.5, 0.75, 1, 1.5, 2],
  fit = true,
}: {
  cmd: Cmd<number, A>
  toArgs: (scale: number | "fit") => A
  min?: number
  max?: number
  step?: number
  presets?: readonly number[]
  fit?: boolean
}) {
  const [open, setOpen] = useState(false)
  const scale = cmd.state.value ?? 1
  const enabled = cmd.state.enabled
  const apply = (s: number | "fit") => {
    const target = s === "fit" ? s : clamp(round(s), min, max)
    void cmd.execute(toArgs(target))
  }
  return (
    <span style={{ position: "relative", display: "inline-flex", alignItems: "center" }}>
      <StatusButton
        label="Zoom out"
        enabled={enabled && scale > min}
        onClick={() => apply(scale - step)}
      >
        <Subtract16Regular />
      </StatusButton>
      <input
        type="range"
        aria-label="Zoom"
        min={min}
        max={max}
        step={0.01}
        value={scale}
        disabled={!enabled}
        onChange={(e) => apply(Number(e.currentTarget.value))}
        style={{ width: 120, accentColor: "var(--of-ink-muted)" }}
      />
      <StatusButton
        label="Zoom in"
        enabled={enabled && scale < max}
        onClick={() => apply(scale + step)}
      >
        <Add16Regular />
      </StatusButton>
      <StatusButton
        label="Zoom presets"
        enabled={enabled}
        active={open}
        onClick={() => setOpen(!open)}
      >
        <span
          style={{ minWidth: 34, textAlign: "right" }}
        >{`${String(Math.round(scale * 100))}%`}</span>
      </StatusButton>
      {fit ? (
        <StatusButton label="Fit to window" enabled={enabled} onClick={() => apply("fit")}>
          <ZoomFit16Regular />
        </StatusButton>
      ) : null}
      {open ? (
        <PresetMenu
          presets={presets}
          fit={fit}
          onPick={(s) => {
            setOpen(false)
            apply(s)
          }}
          onClose={() => setOpen(false)}
        />
      ) : null}
    </span>
  )
}

/** Preset list above the percentage; Esc or a click outside closes it. */
function PresetMenu({
  presets,
  fit,
  onPick,
  onClose,
}: {
  presets: readonly number[]
  fit: boolean
  onPick: (s: number | "fit") => void
  onClose: () => void
}) {
  const ref = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const down = (e: PointerEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose()
    }
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose()
    }
    document.addEventListener("pointerdown", down)
    document.addEventListener("keydown", key)
    ref.current?.querySelector("button")?.focus()
    return () => {
      document.removeEventListener("pointerdown", down)
      document.removeEventListener("keydown", key)
    }
  }, [onClose])
  const items: (number | "fit")[] = fit ? [...presets, "fit"] : [...presets]
  return (
    <div
      ref={ref}
      role="menu"
      style={{
        position: "absolute",
        bottom: "calc(100% + 4px)",
        right: 0,
        minWidth: 120,
        padding: 4,
        background: "var(--of-surface)",
        borderRadius: "var(--of-radius)",
        boxShadow: "var(--of-card-shadow)",
        zIndex: 10,
      }}
    >
      {items.map((s) => (
        <button
          key={String(s)}
          type="button"
          role="menuitem"
          onClick={() => onPick(s)}
          style={{ ...itemStyle, width: "100%", justifyContent: "flex-start" }}
        >
          {s === "fit" ? "Fit to window" : `${String(Math.round(s * 100))}%`}
        </button>
      ))}
    </div>
  )
}

function StatusButton({
  label,
  enabled,
  active = false,
  onClick,
  children,
}: {
  label: string
  enabled: boolean
  active?: boolean
  onClick: () => void
  children: ReactNode
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      aria-pressed={active}
      disabled={!enabled}
      onClick={onClick}
      style={{
        ...itemStyle,
        background: active ? "var(--of-pressed)" : "transparent",
        opacity: enabled ? 1 : 0.4,
        cursor: enabled ? "pointer" : "default",
      }}
    >
      {children}
    </button>
  )
}

const itemStyle: CSSProperties = {
  height: 24,
  minWidth: 24,
  display: "inline-flex",
  alignItems: "center",
  justifyContent: "center",
  gap: 4,
  padding: "0 6px",
  border: "none",
  borderRadius: 4,
  background: "transparent",
  color: "inherit",
  font: "inherit",
}

function clamp(v: number, lo: number, hi: number) {
  return Math.min(hi, Math.max(lo, v))
}

function round(v: number) {
  return Math.round(v * 100) / 100
}
