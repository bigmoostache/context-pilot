# Office Fluent kit (`office/fluent/*`): API proposal

One toolbar template for the Word, Excel and PowerPoint lookalikes. The kit
draws every piece of Office for the web chrome; an app supplies only its accent,
logo, tabs, groups and commands. Status: **revised after A1076 + A1077 reviews** (owner A1075).

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
/** The command shape all three engines return. `A` = the command's args. */
interface Cmd<V = unknown, A = void> {
  label: string
  state: { enabled: boolean; active?: boolean; value?: V }
  execute: (args: A) => unknown
}

type OfficeApp = "word" | "excel" | "powerpoint"
```

## 3. Tokens (`tokens.ts`)

```ts
const FLUENT: {
  font: string            // "Segoe UI", system-ui fallback
  chrome: "#f5f5f5"; surface: "#ffffff"; ink: "#242424"; inkTab: "#424242"
  inkMuted: "#616161"; hover: "#ebebeb"; pressed: "#e0e0e0"; line: "#e0e0e0"
  titleH: 48; tabsH: 36; radius: 8
}
const ACCENT: Record<OfficeApp, string>  // word #185abd, excel #107c41, powerpoint #c43e1c
const HEIGHTS: Record<OfficeApp, { statusH: number; ribbonH: number }>
function officeVars(app: OfficeApp): CSSProperties  // --of-* CSS vars on the shell root
```

App-specific heights (Excel formula bar, Word canvas offset) stay in the app.

## 4. Frame (`Frame.tsx`)

Engine command hooks only work inside the engine's command context, so the
frame is split in two (review A1076, blocker):

```tsx
<OfficeShell app="word" bodyStyle={WORD_BODY_VARS}>   // --of-* vars + canvas/pane bg hook
  <OfficeTitleBar
    title={fileName}
    saveState={state}               // "idle" | "saving" | "saved" | "error" → title chip
    onRename={(name) => …}          // omit = title not editable
  />
  <DocxEditor
    toolbar={
      <OfficeRibbon                 // INSIDE the engine: TabRow + RibbonCard + backstage
        layout="classic"            // "classic" (two-row, Word/Excel) | "single" (PPT web, 40 px card, 32 px buttons)
        tabs={[{ id: "home", label: "Home", ribbon: <HomeRibbon /> }, …]}
        defaultTab="home"
        backstage={<FileBackstage />}   // omit = no File tab
        tabsRight={<…/>}                // e.g. Comments, Editing mode
        belowRibbon={<…/>}              // Excel formula bar; rendered between ribbon and engine body
        commands={searchable}           // SearchItem[] → title-bar search via portal; omit = no search
        status={<StatusBar>…</StatusBar>}  // portalled into OfficeShell's status slot
      />
    }
  />
</OfficeShell>
```

`OfficeRibbon` portals the search box and the status bar into slots that
`OfficeShell` renders outside the engine, so both keep access to commands
(zoom, notes, word count) without an `onReady` API.

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
| `Combo` | `cmd: Cmd<string\|number>` or `onCommit(text)`, `options`, `width`, `parse?`, `renderOption?` | typeable (font name/size, Excel name box), Enter applies, Esc reverts, ↑/↓ list; `renderOption` = per-item preview (number formats) |
| `Gallery` | `cmd: Cmd<string>`, `items: {value,label,preview}[]`, `rows?` | inline strip + expand popover (Styles) |
| `ColorPicker` | `cmd`, `toArgs(color)`, `theme: string[][]`, `standard: string[]`, `noneLabel?` | Word/Excel/PPT theme 10×6 grid, standard row, recent |
| `Tooltip` | wraps any control: `title`, `shortcut?`, `description?` | 500 ms delay, Fluent card style |

`Btn` tooltip defaults to `cmd.label` + `shortcut`. `Gallery` renders inline
or inside any menu popover (`SplitBtn menu={<Gallery rows={3} …/>}`), with
`ReactNode` previews (shapes, layouts, styles).

Engine-drawn areas (Word canvas, PPT thumbnail pane / notes, Excel grid) are not
kit components: the app styles them through `OfficeShell bodyStyle` CSS vars.

## 6. Menus (`menus.tsx`)

`Menu`, `MenuItem({ cmd | onSelect, icon?, checked?, shortcut? })`,
`MenuSub({ label, icon?, children })` (Borders ▸ Line Color ▸ / Line Style ▸),
`MenuSeparator`, `ContextMenu({ items, children })` for the canvas right-click.
Keyboard: ↑/↓, Enter, Esc, typeahead, → opens a submenu; focus returns to the opener.

Global keys (KeyTips, Option+Q search) never fire while focus is inside the
engine's text/cell editor; the engine owns arrows, Enter and Tab there.

## 7. Status bar (`StatusBar.tsx`)

`StatusBar` (left/right slots, height from `heights.statusH`: Word 24, Excel
and PowerPoint 23), `StatusItem({ text, icon?, cmd? | onSelect?, active? })`
(Notes, Normal/Sorter toggles, word count dialog), `Zoom({ cmd: Cmd<number>,
toArgs(scale), min, max, presets })` = − / slider / + / percentage (click →
presets menu).

## 8. Migration

1. Word builds on the kit first (reference implementation).
2. Excel (`office/excel/*`) and PowerPoint (`office/powerpoint/*`) replace
   their own `*Theme`/`*Buttons`/`*Chrome` with the kit, keeping only groups and
   app-specific rows (Excel formula bar, headers).
3. Each app runs the shared dead-control audit: a Playwright pass enumerates
   every `[role=button],button,[role=combobox],[role=tab]` in the frame, clicks
   it, and fails if neither engine state nor DOM changed.
