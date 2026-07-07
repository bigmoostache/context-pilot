import type { KeyboardEvent } from "react"

/**
 * Make a non-`<button>` element behave like a button for keyboard users.
 *
 * Some clickable surfaces cannot be a real `<button>` because they already
 * contain their own nested buttons (a Finder tab with a close ✕, a thread row
 * with archive/pause controls) — nesting a button inside a button is invalid
 * HTML. For those, the ARIA pattern is a `role="button"` element that is
 * focusable (`tabIndex={0}`) and activates on Enter/Space, matching native
 * button semantics.
 *
 * Spread the returned props onto the element and pass the activation callback:
 *
 * ```tsx
 * <div {...clickable(() => onSelect(id))}> … </div>
 * ```
 *
 * The returned `onClick` fires on pointer activation; `onKeyDown` mirrors it for
 * Enter and Space (Space is `preventDefault`ed so the page doesn't scroll). This
 * is the frontend twin of an accessible custom control — it exists so the
 * jsx-a11y interaction rules stay at `error` with zero suppressions.
 */
export function clickable(onActivate: () => void): {
  role: "button"
  tabIndex: 0
  onClick: () => void
  onKeyDown: (e: KeyboardEvent<HTMLElement>) => void
} {
  return {
    role: "button",
    tabIndex: 0,
    onClick: onActivate,
    onKeyDown: (e: KeyboardEvent<HTMLElement>) => {
      if (e.key !== "Enter" && e.key !== " ") return
      e.preventDefault()
      onActivate()
    },
  }
}
