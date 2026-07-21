# Design & Plan — Mobile Component Tree

Status: **proposed** (no code written). Branch: `mobile` (cut from `origin/master`).
Owner: frontend. Scope: `web/`.

---

## 1. Goal

Ship a mobile version of the web app as a **second component tree** that is an
**exact structural mirror** of `web/src/components`, with **one runtime switch**
that selects the desktop or mobile tree based on the current device context.

The design optimises for three properties, in order:

1. **Unbreakable** — it must be structurally impossible for the mobile tree to
   silently render a desktop screen, or leak back into the desktop tree.
2. **Minimal surface** — one switch point, no per-component wiring, no runtime
   resolver/Proxy.
3. **DRY** — screens that don't differ on mobile are not hand-duplicated.

### Non-goals

- No business logic in the mobile tree (the standing rule holds: all logic lives
  in the Rust backend / `web/src/lib`; component trees are presentation only).
- No separate mobile *build* or *bundle entry* — one app, one runtime switch.
- No responsive-CSS rewrite of the existing desktop components.

---

## 2. Findings (from exploring `web/src`)

- **108 files across ~19 subdirs** under `components/`: `finder` 33 · `shell` 23 ·
  `ui` 20 · `threads` 14 · `agents` 9 · `auth` 7 · `conversation` 2.
- **Absolute-import convention.** 98 files import via the `@/` alias
  (`@/ -> ./src`), and subdirectories cross-reference each other heavily
  (`ui/sheet` imports `@/components/ui/button`, `shell/TopBar` imports
  `@/components/auth/UsersDialog`). A verbatim copy of a component into the mobile
  tree would import straight back into the **desktop** tree.
- **No existing mobile infrastructure.** Only 22 Tailwind breakpoints across 14
  files; the app is desktop-first. Mobile is real work, not cosmetic.
- **`App.tsx` (210 lines) is imported only by `main.tsx`.** A clean single seam
  for the switch. It is a view-router (`fleet` / `threads` / `finder` / `costs`)
  wrapping the `TopBar` + `StatusBar` shell.
- Path alias `@/*` is defined in both `tsconfig` (`paths`) and `vite.config.ts`
  (`resolve.alias`). A new alias `@/mobile-components` needs registering in both.

---

## 3. Architecture

### 3.1 The switch — `App.tsx` becomes a chooser

Extract the current `App.tsx` body into `components/Root.tsx` (the desktop mirror
root). `App.tsx` shrinks to a single decision plus a Suspense boundary:

```tsx
// src/App.tsx — the ONE switch point
const isMobile = useIsMobile()
const Root = useMemo(
  () =>
    isMobile
      ? lazy(() => import("@/mobile-components/Root"))
      : lazy(() => import("@/components/Root")),
  [isMobile],
)
return (
  <Suspense fallback={<AppSkeleton />}>
    <Root />
  </Suspense>
)
```

Only the active tree's dynamic chunk downloads at runtime (Vite code-splits the
`import()`). Both trees are compiled at build time; only one is fetched by the
browser.

### 3.2 Self-containment via a mirrored root token

Each tree references **only itself**:

- desktop files import `@/components/...`
- mobile files import `@/mobile-components/...`

Imports that leave the component tree (`@/lib/...`, hooks, query client, API SDK,
`@/lib/providers/*`) stay **shared** — no logic is duplicated, only the
presentation tree forks. This is what keeps the mobile tree a pure render fork.

The switch self-propagates: flipping the one root `import()` cascades through the
whole tree because each tree's internal imports resolve within its own token.

### 3.3 The mirror — exact 1:1, DRY via stubs + ancestor-promotion

Every desktop path has a mobile twin at the identical relative path. Twins are one
of two kinds:

- **Stub** (default) — a generated re-export of the desktop file, for screens that
  do not differ on mobile:

  ```ts
  // @generated mobile-mirror stub — do not edit; regenerate via pnpm mirror:scaffold
  export * from "@/components/ui/button"
  export { default } from "@/components/ui/button"
  ```

  Note both lines: `export *` carries **named** exports (incl. types); the second
  line carries the **default** export, which `export *` does *not*. The scaffold
  emits the `default` line only when the source has a default export.

- **Real file** (hand-authored) — a genuine mobile implementation, importing its
  children via `@/mobile-components/...`.

**Ancestor-promotion (the correctness rule).** A stub is only safe if *none of its
descendants diverge* — otherwise the stub would bypass the divergent mobile child
by pulling in the desktop subtree. Therefore: when a leaf is made real (divergent),
**every ancestor up to `Root` must also be a real file** that routes to
`@/mobile-components/` children. The scaffold enforces this automatically.

**Generated-vs-authored boundary.** Every generated stub carries the
`// @generated mobile-mirror stub` marker on line 1. The scaffold **only creates,
rewrites, or deletes files bearing this marker**. A file without the marker is
hand-authored and never touched — this is what protects real mobile work from
being clobbered or deleted.

### 3.4 Bidirectional parity — where mobile-only primitives live

"Exact mirror" is **bidirectional**: a path present in one tree but not the other
fails the lint. Consequently a *mobile-only building block* cannot live in
`mobile-components/` (it would have no desktop twin). Rule:

> Shared primitives that only the mobile tree happens to use go in a
> **non-mirrored shared location** (`@/lib/ui` or `@/primitives`), not in
> `mobile-components/`. The mirror stays strictly 1:1.

---

## 4. The scaffold script (`web/scripts/scaffold-mobile-mirror.ts`)

Idempotent, re-runnable. Invoked as `pnpm mirror:scaffold`.

Responsibilities:

1. **Walk** `components/` recursively.
2. **Generate stubs** for any desktop path lacking a mobile twin — emitting the
   `@generated` marker, the `export *` line, and the `export { default }` line iff
   the source has a default export.
3. **Promote ancestors** — for any hand-authored (marker-less) mobile file, ensure
   every ancestor directory's index/container up to `Root` is a real routing file,
   not a stub.
4. **Orphan cleanup** — delete generated stubs (marker-bearing only) whose desktop
   source no longer exists. Never delete marker-less files.
5. **Rewrite** — inside generated files, apply `@/components/` -> `@/mobile-components/`
   (no-op for pure `export *` stubs that intentionally re-export desktop leaves).

Output is deterministic so `git diff --exit-code` after a scaffold run is a
drift check (same pattern as the existing generated-bindings contract check).

---

## 5. The lint (`.github/checks/check-mobile-mirror.sh`)

Three invariants, CI-side and callback-side (dual-mode like the other checks):

1. **Path-set parity (bidirectional).** The recursive relative-path set of
   `components/` and `mobile-components/` must be *equal*. Any file/dir in one but
   not the other is an error. This is the "EXACT" guarantee.
2. **Leak guard.** No `@/components/` import inside `mobile-components/`. The regex
   is anchored to a real import position (`^\s*(import|export)\b...from\s*['"]@/components/`)
   so a `@/components/` mention inside a string or comment does not false-positive
   (same class of gotcha as the lint-exception-registry substring trap).
3. **Marker integrity.** Every file matching the generated shape carries the
   `@generated mobile-mirror stub` marker; conversely no marker-bearing file has
   been hand-edited (optional phase-2: checksum the generated body).

Wiring:

- New script in `.github/checks/`, **added to the hash chain** (`protected-files.yaml`
  + `chain.sh --update`) so the lint itself can't be silently weakened.
- **Blocking callback** on `web/src/{components,mobile-components}/**` — twin of the
  existing structure/lint callbacks (CB4/CB5).
- A **CI job** in the Structure/Frontend workflow.

Phase-2 extension (optional): **export-name parity** per twin, so "exact" covers
signatures (each mobile twin must export the same symbol names as its desktop
source). Deferred to keep phase 1 shippable.

---

## 6. Device detection & reactivity

`useIsMobile()` — `window.matchMedia("(max-width: 768px)")`, mirroring the
existing `ThemeProvider` matchMedia pattern.

**Decision: switch once at load, do not hot-swap the tree on live resize.**
A live cross-over (e.g. desktop window dragged narrow) would re-mount the entire
tree, destroying component-local state (scroll positions, open dialogs, in-progress
form input). TanStack Query cache is shared and survives, but local UI state does
not. Recommended behaviour:

- Resolve `isMobile` **once at first paint**; render that tree for the session.
- Listen for the media-query change only to surface a non-intrusive
  **"Reload for the {mobile|desktop} layout" toast** — user-initiated reload
  performs the swap cleanly.

This keeps the switch unbreakable (no mid-session tree teardown) and avoids a
whole class of state-loss bugs. Reactive-remount can be revisited later if desired.

---

## 7. Build & styling considerations

- **Tailwind content globs** must include `./src/mobile-components/**` or mobile
  files' utility classes get purged (silent styling loss). Add the glob in the same
  step that creates the folder.
- **Suspense fallback** — `App.tsx` needs a `<Suspense fallback={<AppSkeleton />}>`
  around the lazy `Root` so first mobile paint isn't a blank frame.
- **Alias registration** — add `@/mobile-components` to `tsconfig` `paths` and
  `vite.config.ts` `resolve.alias`.
- **Bundle** — the lazy `import()` makes each tree its own dynamic chunk; only the
  active one is fetched. `tsc -b` already validates every twin resolves (structural
  parity + compile = no broken re-export path).

---

## 8. Phased rollout (PR on `mobile`)

- **P0 — prep.** Register `@/mobile-components` alias (tsconfig + vite); add
  Tailwind content glob; add `AppSkeleton`.
- **P1 — switch seam.** Extract `App.tsx` body -> `components/Root.tsx`; add
  `useIsMobile` + the `App.tsx` chooser + Suspense. Desktop behaves identically.
- **P2 — scaffold + full stub mirror.** Land `scaffold-mobile-mirror.ts`; generate
  all 108 stubs (mobile renders identical to desktop initially).
- **P3 — lint + wiring.** `check-mobile-mirror.sh` (3 invariants) + blocking
  callback + CI job + hash-chain the script.
- **P4 — first divergence (proof of concept).** Make `Root` + `shell/TopBar` real
  mobile files (drawer nav, bottom bar), exercising ancestor-promotion end-to-end.

Each phase leaves `master`-mergeable state; desktop is never regressed.

---

## 9. Risks & mitigations

| Risk | Mitigation |
|------|------------|
| Mobile file leaks into desktop tree | Leak-guard lint (anchored regex), blocking callback |
| Scaffold clobbers hand-authored mobile work | `@generated` marker; scaffold only touches marker-bearing files |
| Orphan stubs after desktop deletes | Scaffold orphan-cleanup (marker-only deletes) |
| `export *` drops default export | Stub emits explicit `export { default }` when source has one |
| Divergent leaf bypassed by stub ancestor | Ancestor-promotion rule enforced by scaffold |
| Live resize destroys UI state | Switch-once-at-load + reload toast (no hot remount) |
| Mobile classes purged by Tailwind | Add `mobile-components/**` to content globs in P0 |
| Blank first paint on mobile | Suspense fallback skeleton |
| Lint silently weakened | Hash-chain `check-mobile-mirror.sh` |
| Stub re-exports a divergent desktop **subtree** via a barrel | Divergence-closure over the *import graph*, not the folder tree (§11.1) |
| Blind text rewrite corrupts a non-specifier `@/components/` string | AST specifier-only rewrite, never `sed` (§11.2) |
| `lazy()` re-created each render → remount + chunk refetch | Hoist both `lazy()` to module scope (§11.3) |
| Case-only twin (`Button` vs `button`) passes on macOS, breaks/leaks on CI | Case-sensitive parity comparison (§11.5) |
| Mobile tree reads as dead code to knip before any consumer | Add mobile `Root` to knip `entry` (§11.6) |
| Divergent mobile `Root` drops a context provider desktop mounts | `Root` is a provider-contract boundary (§11.8) |

---

## 11. Second hardening pass (second-order failure modes)

The eight items in §3–§9 close the obvious gaps. These are the subtler ones a
senior review still catches — several are genuine correctness issues, not polish.

### 11.1 Divergence-closure is over the import graph, not the folder tree

The ancestor-promotion rule (§3.3) must be defined over the **module dependency
graph**, not just the directory tree. `web/src/components` has **3 barrel
`index.ts` files** that re-export siblings. A stub that `export *`s a desktop
*barrel* (or any container) transitively pulls in that barrel's whole desktop
subtree — bypassing every divergent mobile child underneath it.

Rule: **a stub may only re-export a desktop module whose divergence-closure is
empty** — i.e. neither the module nor anything it re-exports/imports within the
component tree is divergent. When any node in that closure is divergent, the stub
must be promoted to a real routing file. The scaffold computes this closure from
the parsed import graph, so barrels are handled correctly rather than silently
leaking.

### 11.2 The rewrite is AST specifier-only, never `sed`

`@/components/` → `@/mobile-components/` must rewrite **only import/export/dynamic-
`import()` specifiers**, resolved from the parsed AST. A blind text substitution
would also corrupt a `@/components/` string that is *not* a module specifier — an
analytics event name, a telemetry tag, a comment, a `className`. The scaffold
parses (same toolchain as the codegen already in the repo) and rewrites specifier
nodes only. This also means dynamic `lazy(() => import("@/components/…"))` inside a
component is rewritten correctly (it is a specifier), while a look-alike string is
left alone. The leak-guard lint (§5.2) is anchored for the same reason.

### 11.3 Hoist the two `lazy()` out of render

Because the switch is resolved once at load (§6), the two lazy roots can be
**module-level constants**, not created inside the component:

```tsx
const DesktopRoot = lazy(() => import("@/components/Root"))
const MobileRoot = lazy(() => import("@/mobile-components/Root"))
// in App:  const Root = useIsMobile() ? MobileRoot : DesktopRoot
```

This removes the `useMemo` subtlety entirely and guarantees no accidental remount
/ chunk-refetch from re-creating `lazy()` on a re-render.

### 11.4 Mirror file-set filter is explicit

The parity set is **component source only**: `*.tsx` / `*.ts` that are components
or their local helpers. Excluded from *both* sides: `*.test.*`, `*.stories.*`,
`*.d.ts`. (There are currently 0 such files under `components/`, but stating the
filter keeps a future test/story co-location from demanding a mobile twin.)

### 11.5 Case-sensitive parity

The parity comparison is **case-sensitive** and separator-normalised. macOS dev
filesystems are case-insensitive; CI Linux is case-sensitive — a `Button.tsx` twin
against a desktop `button.tsx` would pass locally and either fail CI or, worse,
resolve to the wrong file. The check compares raw byte paths.

### 11.6 knip + the generated-bindings drift pattern (house style)

Two integrations with the existing frontend lint stack:

- **knip entry point.** `knip.json` currently only has `e2e/**` as `entry` and
  ignores `components/ui/**`. The mobile tree is reached *only* via a dynamic
  `import()`, so without help knip reports the whole tree as dead code. Add
  `src/mobile-components/Root.tsx` to `entry` (and mirror the `ui/**` ignore for
  `mobile-components/ui/**`).
- **Reuse the generated-bindings drift model.** The scaffold's CI check is the
  same shape as the existing `check-typescript-contract.sh`: *regenerate, then
  `git diff --exit-code`*. Generated stubs carry the `@generated` header (already
  §3.3); the scaffold emits already-formatted output so prettier/eslint are stable
  over them. This makes the mirror check consistent with the house pattern rather
  than a bespoke mechanism.

### 11.7 `useIsMobile` is SSR-safe (cheap future-proofing)

The app is a client-only SPA today (`createRoot`, no hydration), so `matchMedia`
at first render is fine. Guard `useIsMobile` with a `typeof window === "undefined"`
fallback anyway, so a future prerender/SSR step can't turn the switch into a
`window is not defined` crash. Costs one line now, removes a booby-trap later.

### 11.8 `Root` is a provider-contract boundary

`main.tsx` mounts `QueryClientProvider` **above** the switch, so the shared query
cache survives it. But any provider mounted *inside* `components/Root` (a layout
context, a theme boundary) is part of `Root`'s contract: a divergent mobile `Root`
**must mount the same providers** its desktop twin does, or mobile children that
consume those contexts break. This is exactly what the optional phase-2
export-signature parity (§5) would enforce mechanically; until then it is a
documented invariant for anyone authoring a real mobile `Root`.

### 11.9 Verified during this pass

- `index.html` **already** carries `<meta name="viewport" content="width=device-width, initial-scale=1.0">` — mobile layout won't be broken at the document level. No action needed.

---

## 12. Open questions (for confirmation)

1. **Stub model vs full-copy** — recommend the stub + ancestor-promotion model
   (DRY). Full-copy (108 real files kept in sync) is the simpler-but-duplicative
   fallback.
2. **Breakpoint** — `768px` cutover. Confirm value and the switch-once (not
   reactive-remount) behaviour from §6.
3. **Export-name parity (phase 2)** — do you want "exact" to eventually mean
   matching export *signatures*, or is structural path parity sufficient?
4. **Detection signal** — `max-width: 768px` keys the switch on *viewport width*
   (a narrow desktop window gets the mobile tree — usually desired). The
   alternative is a *device* signal (`pointer: coarse` / UA), which tracks
   "touch device" rather than "small window." Confirm width-based is what you
   want, or specify the device-based axis.
