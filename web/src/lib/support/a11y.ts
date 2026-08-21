import { useEffect, useRef, useState } from "react"
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

/**
 * Watch the ⌘/Ctrl modifier and bind a letter shortcut to each supplied action.
 *
 * Returns whether the modifier is currently HELD, which is what a caller uses
 * to reveal its shortcut badges. Detection and binding are ONE hook on purpose:
 * a badge is a promise that the key will do something, so a badge that could
 * appear without its binding (or the reverse) would be a lie.
 *
 * SCOPE IS THE CALLER'S MOUNT. There is no visibility check in here — the
 * listener lives for exactly as long as the component that calls this hook, so
 * "only fire while the buttons are shown" is true by construction rather than
 * by a second copy of the visibility condition that could drift from the first.
 *
 * THREE THINGS THAT ARE NOT OBVIOUS:
 *
 * 1. The held state is read from `metaKey`/`ctrlKey` on EVERY key event rather
 *    than by matching `e.key === "Meta"`. Holding ⌘ and then pressing another
 *    key emits a keydown for that key, not a second one for ⌘ — matching on the
 *    key name alone would miss it.
 *
 * 2. THE STICKY-BADGE BUG, and the reason for the blur/visibility listeners:
 *    ⌘-Tab, ⌘-Space and ⌘-Shift-4 all take the window away WHILE THE MODIFIER
 *    IS DOWN, so the keyup is delivered to something else and never arrives
 *    here. Without clearing on blur, the badges stay lit forever after the
 *    first app switch.
 *
 * 3. SOME LETTERS ARE ALREADY TAKEN, and `c` is the dangerous one — it is Copy.
 *    A binding on `c` fires ONLY when a copy would have done nothing anyway
 *    (see {@link copyWouldBeNoop}), so a real copy is never stolen. Note that
 *    the test is on the SELECTION, not on focus: a caret sitting in a text box
 *    with nothing highlighted copies nothing, so the shortcut still works
 *    there. Letters with no such conflict are bound unconditionally.
 *
 * @param actions Lowercase letter → what ⌘/Ctrl + that letter should do.
 */
export function useModifierShortcuts(actions: Record<string, () => void>): boolean {
  const [modHeld, setModHeld] = useState(false)

  // Latest-ref so the listeners bind ONCE. The map is rebuilt by the caller on
  // every render, so a dependency on it would tear down and re-add a window
  // listener on each keystroke.
  const actionsRef = useRef(actions)
  useEffect(() => {
    actionsRef.current = actions
  })

  useEffect(() => {
    const onKeyDown = (e: globalThis.KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey
      setModHeld(mod)
      if (!mod) return

      const key = e.key.toLowerCase()
      const run = actionsRef.current[key]
      if (!run) return
      // See note 3 — the one letter that must yield to the platform.
      if (key === "c" && !copyWouldBeNoop()) return

      e.preventDefault()
      run()
    }
    const onKeyUp = (e: globalThis.KeyboardEvent) => setModHeld(e.metaKey || e.ctrlKey)
    // See note 2 — the modifier is released off-window and the keyup is lost.
    const clear = () => setModHeld(false)

    window.addEventListener("keydown", onKeyDown)
    window.addEventListener("keyup", onKeyUp)
    window.addEventListener("blur", clear)
    document.addEventListener("visibilitychange", clear)
    return () => {
      window.removeEventListener("keydown", onKeyDown)
      window.removeEventListener("keyup", onKeyUp)
      window.removeEventListener("blur", clear)
      document.removeEventListener("visibilitychange", clear)
    }
  }, [])

  return modHeld
}

/**
 * The two items ⌘/Ctrl+Up and ⌘/Ctrl+Down would move TO, given the current
 * selection index — the data behind the arrow hint badges.
 *
 * THE 2-ITEM CASE IS THE SUBTLE ONE. With exactly two items, up and down BOTH
 * land on the other one, so showing both a ↑ and a ↓ on the same row would be
 * noise. Only the NON-WRAPPING direction is surfaced — the "natural" one: from
 * the first item that is Down, from the second it is Up. Both keys still work;
 * only one badge is drawn. With one item there is nowhere to go, so neither.
 */
function loopBadges(
  ids: readonly string[],
  idx: number,
): { prevId: string | null; nextId: string | null } {
  if (idx === -1) return { prevId: null, nextId: null }
  const n = ids.length
  if (n >= 3) {
    return { prevId: ids[(idx - 1 + n) % n] ?? null, nextId: ids[(idx + 1) % n] ?? null }
  }
  if (n === 2) {
    return idx === 0
      ? { prevId: null, nextId: ids[1] ?? null }
      : { prevId: ids[0] ?? null, nextId: null }
  }
  return { prevId: null, nextId: null }
}

/**
 * Loop-navigate a selection list with ⌘/Ctrl+Up / ⌘/Ctrl+Down, and report which
 * neighbours to badge while the modifier is held.
 *
 * Drives the Threads list and the Settings category rail: Up selects the
 * previous item, Down the next, and both WRAP at the ends. `orderedIds` must be
 * in the on-screen order so the movement matches what the eye expects; the
 * selection moves through {@link onSelect}.
 *
 * Bound through {@link useModifierShortcuts}, so the listener lives exactly as
 * long as the calling rail is mounted (only one of Threads / Settings is ever
 * on screen, so their bindings never overlap). The arrows loop the sidebar even
 * while a text field is focused (T634 — the native caret-to-edge is suppressed
 * by the shortcut's own `preventDefault`), and a selection that is not in the
 * list (index −1) jumps to the first item rather than doing nothing.
 *
 * @returns `modHeld` to gate the badges, plus the `prevId` / `nextId` to draw
 *          the ↑ / ↓ badge on (see {@link loopBadges} for the edge rules).
 */
export function useLoopNav(
  orderedIds: readonly string[],
  selectedId: string,
  onSelect: (id: string) => void,
): { modHeld: boolean; prevId: string | null; nextId: string | null } {
  const n = orderedIds.length
  const idx = orderedIds.indexOf(selectedId)

  const go = (dir: 1 | -1) => {
    // No text-field bail (T634): the shortcut must loop the sidebar even while
    // the caret sits in the composer. `useModifierShortcuts` already
    // `preventDefault`s the bound arrows, so the native ⌘/Ctrl+Up/Down
    // caret-to-edge is suppressed regardless — a guard here only left the keys
    // dead (native move blocked AND no nav), which is the bug being fixed.
    if (n === 0) return
    const target = orderedIds[idx === -1 ? 0 : (idx + dir + n) % n]
    if (target !== undefined) onSelect(target)
  }

  const modHeld = useModifierShortcuts({ arrowup: () => go(-1), arrowdown: () => go(1) })
  const { prevId, nextId } = loopBadges(orderedIds, idx)
  return { modHeld, prevId, nextId }
}

/**
 * Whether ⌘C at this moment would copy NOTHING — the only case in which the
 * combination is safe to repurpose.
 *
 * THE QUESTION IS "IS ANYTHING SELECTED", NOT "IS FOCUS IN A FIELD". This used
 * to bail on any focused input/textarea/contenteditable, which killed the
 * shortcut for the most common situation there is — a caret parked in the
 * thread composer. A caret with nothing selected copies nothing, so there was
 * no copy to protect and the guard was refusing exactly the case it existed to
 * allow.
 *
 * A FOCUSED TEXT FIELD OWNS ITS OWN SELECTION, and the document does not report
 * it: `getSelection()` returns an empty string while a textarea holds
 * highlighted text. So the field has to be asked directly, which is why the two
 * branches cannot be collapsed into one.
 */
function copyWouldBeNoop(): boolean {
  const el = document.activeElement

  if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) {
    // `selectionStart` is null on input types that do not support selection
    // (number, email, colour). Those have nothing to copy either, so the null
    // case folds into "no-op" rather than needing a guard of its own.
    return el.selectionStart === null || el.selectionStart === el.selectionEnd
  }

  // contentEditable included: its selection IS the document's.
  return (getSelection()?.toString() ?? "") === ""
}
