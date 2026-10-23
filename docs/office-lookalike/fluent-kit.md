# Office Fluent kit (`office/fluent/*`): API proposal

One toolbar template for the Word, Excel and PowerPoint lookalikes. The kit
draws every piece of Office for the web chrome; an app supplies only its accent,
logo, tabs, groups and commands. Status: **proposal for review** (owner A1075).

## 1. Principles

- **Engine-agnostic.** The three BetterOffice engines expose the same command
  shape: `use{Docx,Xlsx,Pptx}Command(id, args)` → `{ label, state: { enabled,
  active?, value? }, execute(args) }`. The kit consumes that shape as `Cmd`; it
  never imports an engine.
- **No dead control, by type.** Every interactive part requires `cmd` or
  `onSelect`. There is no prop combination that renders a clickable element
  without behaviour. A disabled command renders disabled (40 % opacity), never
  hidden-but-clickable.
- **Fluent icons only.** `@fluentui/react-icons` (MIT, NOTICE in
  `office/fluent/NOTICE.md`), 20 px regular for large/two-row buttons, 16 px
  for small. lucide is no longer used in Office chrome.
- **Files ≤ 500 lines, folder ≤ 8 entries**: `fluent/` holds `tokens.ts`,
  `icons.ts`, `Frame.tsx`, `TabRow.tsx`, `controls.tsx`, `menus.tsx`,
  `StatusBar.tsx`, `NOTICE.md`. Mobile twins via `pnpm mirror:scaffold`.

## 2. Types

```ts
/** The command shape all three engines return. */
interface Cmd<V = unknown> {
  label: string
  state: { enabled: boolean; active?: boolean; value?: V }
  execute: (args?: never) => unknown
}

type OfficeApp = "word" | "excel" | "powerpoint"
```

## 3. Tokens (`tokens.ts`)

```ts
const FLUENT: {
  font: string            // "Segoe UI", system-ui fallback
  chrome: "#f5f5f5"; surface: "#ffffff"; ink: "#242424"; inkTab: "#424242"
  inkMuted: "#616161"; hover: "#ebebeb"; pressed: "#e0e0e0"; line: "#e0e0e0"
  titleH: 48; tabsH: 36; statusH: 24; radius: 8
}
const ACCENT: Record<OfficeApp, string>  // word #185abd, excel #107c41, powerpoint #c43e1c
function officeVars(app: OfficeApp): CSSProperties  // --of-* CSS vars on the frame root
```

App-specific heights (Excel formula bar, Word canvas offset) stay in the app.

## 4. Frame (`Frame.tsx`)

```tsx
<OfficeFrame
  app="word"
  title={fileName}
  saveState={state}                 // "idle" | "saving" | "saved" | "error" → title chip
  onRename={(name) => …}            // omit = title not editable
  commands={searchable}             // SearchItem[] for the title-bar search; omit = no search box
  tabs={[{ id: "home", label: "Home", ribbon: <HomeRibbon /> }, …]}
  defaultTab="home"
  backstage={<FileBackstage />}     // omit = no File tab
  tabsRight={<…/>}                  // e.g. Comments, Editing mode
  status={<StatusBar>…</StatusBar>}
>
  {engine}
</OfficeFrame>
```

- Title bar: logo (per app, inline SVG), document name, save chip, centred
  search (Option+Q focuses, typing filters `commands`, Enter runs, Esc closes).
  No waffle, settings or account (user decision: removed).
- `TabRow`: switches ribbons, active = 600 + 3 px accent underline, hover bg,
  double-click toggles ribbon collapse, ←/→ keyboard nav. File opens
  `backstage` as a full-height panel with Esc/back arrow.
- `RibbonCard`: white, radius 8, faint shadow, horizontal overflow collapses
  rightmost groups into a "…" menu (ResizeObserver).

`SearchItem = { label: string; keywords?: string[]; cmd: Cmd }`.

## 5. Controls (`controls.tsx`)

| Component | Props | Notes |
|---|---|---|
| `RibbonGroup` | `label`, children | two-row layout slot, separator after |
| `Btn` | `cmd`, `icon`, `size?: "sm"\|"lg"`, `text?`, `args?`, `shortcut?` | toggle when `state.active` defined (aria-pressed) |
| `SplitBtn` | `cmd`, `icon`, `menu: ReactNode`, `args?` | main half runs last-used args; chevron opens menu |
| `DropBtn` | `label`, `icon`, `menu` | opens menu only (e.g. Line spacing) |
| `Combo` | `cmd: Cmd<string\|number>`, `options`, `width`, `parse?`, `renderOption?` | typeable (font name/size), Enter applies, Esc reverts, ↑/↓ list |
| `Gallery` | `cmd: Cmd<string>`, `items: {value,label,preview}[]`, `rows?` | inline strip + expand popover (Styles) |
| `ColorPicker` | `cmd`, `toArgs(color)`, `theme: string[][]`, `standard: string[]`, `noneLabel?` | Word/Excel/PPT theme 10×6 grid, standard row, recent |
| `Tooltip` | wraps any control: `title`, `shortcut?`, `description?` | 500 ms delay, Fluent card style |

`Btn` tooltip defaults to `cmd.label` + `shortcut`.

## 6. Menus (`menus.tsx`)

`Menu`, `MenuItem({ cmd | onSelect, icon?, checked?, shortcut? })`,
`MenuSeparator`, `ContextMenu({ items, children })` for the canvas right-click.
Keyboard: ↑/↓, Enter, Esc, typeahead; focus returns to the opener.

## 7. Status bar (`StatusBar.tsx`)

`StatusBar` (left/right slots), `StatusItem({ text, onSelect? })`,
`Zoom({ cmd: Cmd<number>, toArgs(scale), min, max, presets })` = − / slider / +
/ percentage (click → presets menu).

## 8. Migration

1. Word builds on the kit first (reference implementation).
2. Excel (`office/excel/*`) and PowerPoint (`office/powerpoint/*`) replace
   their own `*Theme`/`*Buttons`/`*Chrome` with the kit, keeping only groups and
   app-specific rows (Excel formula bar, headers).
3. Each app runs the shared dead-control audit: a Playwright pass enumerates
   every `[role=button],button,[role=combobox],[role=tab]` in the frame, clicks
   it, and fails if neither engine state nor DOM changed.
