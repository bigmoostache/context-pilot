# Word for the web clone: battle plan

Goal: a user who knows Word for the web cannot tell our Finder .docx viewer
apart from it, by looking or by using it. Engine stays BetterOffice
`@betteroffice/docx-react`; everything we draw lives in `office/word/*` on top
of a shared Office kit (`office/fluent/*`) used by Word, Excel and PowerPoint.

Reference: Word for the web editor, light theme, 1440×900, captured headless
(`/tmp/wh/ref/web-editor.png`, DOM spec `/tmp/wh/ref/spec.json`). Locale of the
reference is FR; we ship EN labels with the same geometry.

User decisions (2026-10-23): Microsoft cloud controls (Share, Dictate, Editor,
Add-ins, Catch up, waffle, Settings, account) are **removed**; tabs the engine
cannot support (References, Draw, Design, Help) are **hidden**.

## 0. Rules

1. **No dead element.** Every visible control does what Word does, or it is
   removed. A control is never "for the look only".
2. **Behaviour before pixels.** A click, hover, keyboard shortcut or menu that
   behaves differently is a worse tell than a 2 px offset.
3. **One template.** Title bar, tab row, ribbon card, buttons, split buttons,
   dropdowns, galleries, colour pickers, tooltips, status bar and zoom live in
   `office/fluent/*`, parameterised by app (accent colour, logo, tabs). Word,
   Excel and PowerPoint only supply their groups and commands.
4. **Measured, not eyeballed.** Each zone has a pixel-diff budget against the
   reference and a Playwright behaviour test.

## 1. Shared kit (`office/fluent/*`), built once

| Part | Word behaviour to reproduce | Acceptance |
|---|---|---|
| Tokens | Fluent 2 neutrals (#f5f5f5 chrome, #fff card, #242424 ink, #424242 tabs, #616161 muted, #e0e0e0 hover-pressed), per-app accent (Word #185abd, Excel #107c41, PPT #c43e1c), Segoe UI → system fallback, 14 px tab / 12 px small text | colour picks match reference ±2 |
| Icons | Fluent UI System Icons (MIT), 20 px regular / 16 px small, replacing lucide (biggest visual tell today) | same glyph as reference for every control |
| `TitleBar` | waffle, app logo, document name (click → rename field), "Saved" state chip, centred search box (Option+Q focuses; typing filters commands, Enter runs one), Settings, account | rename persists to file name; search runs commands |
| `TabRow` | tabs switch the ribbon; active tab 600 weight + accent underline 3 px; hover background; double-click collapses ribbon; File opens backstage | each tab shows its own ribbon |
| `RibbonCard` | white card, radius 8, shadow; classic two-row layout; overflow collapses groups into "…" when narrow | resize 1440→900 matches Word's collapse order |
| `Button` | 28×24 (small) / 40×40+label (large), hover #f5f5f5, pressed #e0e0e0, checked state, disabled 40 % | states match on hover/press screenshots |
| `SplitButton` | main action + chevron menu (Highlight, Font Color, Underline, Bullets, Numbering, Paste) | last-used colour shown on the icon bar |
| `Dropdown` / `Combo` | font name (typeable, previews fonts in their face), font size (typeable, 8…72 list) | typing "14" + Enter applies |
| `Gallery` | Styles gallery: live previews, hover-preview, expand arrow | applying a style matches Word's style |
| `ColorPicker` | theme colours grid 10×6, standard colours, "No Color", recent | same layout and swatches |
| `Tooltip` | delayed ~500 ms, title + shortcut + one-line description | text matches Word |
| `KeyTips` | Alt (Ctrl+Option on Mac) shows letter badges on tabs then controls | sequence Alt,H,1 bolds |
| `StatusBar` | page x of y, word count (click → dialog), language, text predictions, focus mode, zoom −/slider/+, percentage (click → zoom presets), "Give feedback" | values live from the document |
| `ContextMenu` | right-click on canvas: Cut/Copy/Paste, Font, Paragraph, Link, New comment | replaces browser menu |

## 2. Word, zone by zone

### 2.1 Title bar
| Element | Behaviour | Engine/impl |
|---|---|---|
| Waffle | opens app launcher | remove (no Microsoft 365 apps) |
| Word logo | none in Word web except home link | static logo, no hover |
| Document name | click → inline rename, Enter commits | Finder rename API |
| Saved chip | "Saving…" / "Saved" / "Save failed" | from `SaveState` |
| Search | command search | kit `TitleBar` + command registry |
| Settings / account | menus | remove (no account) |

### 2.2 Tab row
| Tab | Content |
|---|---|
| File | backstage: Save a copy (download), Print (`print`), Info (size, modified), Close |
| Home | §2.3 |
| Insert | Page break, Section breaks, Table (grid picker 10×8 → `insertTable`), Picture (`insertImage`), Link (`insertLink`), TOC (`insertTOC`) |
| Layout | Page setup (`pageSetup`), Watermark (`watermark`), Breaks |
| Review | Track changes / editing mode (`editingMode`), Previous/Next change, Accept/Reject (`reviewAccept/Reject`), Comments pane (`commentsSidebar`) |
| View | Zoom presets (`zoom`), Ruler toggle (`showRuler`) |
| References, Help, Draw, Design | removed until the engine supports them |
| Right side | Comments → `commentsSidebar`; Editing ▾ → Editing / Reviewing / Viewing (`editingMode`); Share, Catch up → removed |

### 2.3 Home ribbon, control by control
| Group | Control | Word behaviour | Engine |
|---|---|---|---|
| Undo | Undo / Redo | Cmd+Z / Cmd+Shift+Z, disabled when empty | `undo` `redo` |
| Clipboard | Paste ▾ | paste; menu: Keep formatting / Text only | Clipboard API → engine paste |
| | Cut, Copy | selection to clipboard | `document.execCommand` / Clipboard API |
| | Format Painter | copies run format, applies on next selection; double-click = sticky | read selection marks → apply |
| Font | Font name | combo, recent fonts, previews | `fontFamily` (options list) |
| | Font size | combo 8…72, typeable | `fontSize` |
| | Grow / Shrink | Word's size ladder (8,9,10,11,12,14,16,18,20,22,24,26,28,36,48,72) not ±1 | `fontSize` |
| | Clear formatting | | `clearFormatting` |
| | B I U S, x₂ x² | toggles, Cmd+B/I/U, active state | existing |
| | Underline ▾ | styles menu | remove chevron unless engine supports styles |
| | Change Case ▾ | Sentence/lower/UPPER/Capitalise/tOGGLE | text rewrite of selection |
| | Highlight ▾ | colour menu, last colour on bar | `highlightColor` |
| | Font Color ▾ | ColorPicker | `textColor` |
| Paragraph | Bullets ▾ / Numbering ▾ | toggle + library menu | `bulletList` `numberedList` |
| | Multilevel list | | remove unless supported |
| | Decrease / Increase indent | | `outdent` `indent` |
| | LTR / RTL | | `setLtr` `setRtl` |
| | Show ¶ | formatting marks | remove unless engine renders marks |
| | Align L/C/R/J | Cmd+L/E/R/J, radio state | `alignment` |
| | Line spacing ▾ | 1.0 1.15 1.5 2.0 2.5 3.0 | `lineSpacing` (240×n twips) |
| | Shading, Borders | | remove (no engine command) |
| Styles | Gallery | Normal, No Spacing, Heading 1–3, Title, Subtitle… from the document's styles | `paragraphStyle` (options list) |
| Editing | Find / Replace / Select | Find → Cmd+F pane; Replace dialog; Select All | `find` `replace`; Select → Select All |
| Voice, Editor, Add-ins | Dictate, Editor, Add-ins | Microsoft cloud services | removed |

### 2.4 Canvas
| Behaviour | Word | Plan |
|---|---|---|
| Background | #f5f5f5, page white, faint shadow, no border | tune `--doc-*` vars |
| Page position | centred, 20 px top gap below ribbon | measure + CSS |
| Scroll | smooth, page gap 20 px | check engine pagination gap |
| Caret / selection | blue #0078d4-ish selection, 1 px black caret | engine CSS vars |
| Hyperlink hover | Ctrl+click tooltip | engine check |
| Fonts | Calibri→Carlito, Cambria→Caladea (metric-compatible) | `measurementFontProvider` + `@betteroffice/fonts` |
| Rulers | hidden by default (View → Ruler) | `showRuler` toggle |

### 2.5 Keyboard
Every Word web shortcut that has an engine command: Cmd+B/I/U, Cmd+Z/Y,
Cmd+Shift+Z, Cmd+L/E/R/J, Cmd+]/[ (grow/shrink), Cmd+Shift+L (bullets),
Cmd+F, Cmd+H (replace), Cmd+S (save), Cmd+P (print), Tab/Shift+Tab in lists,
Option+Q (search). Test each in Playwright.

## 3. Phases

1. **Kit** (`office/fluent/*`): tokens, Fluent icons, Button/Split/Dropdown/
   ColorPicker/Tooltip, TitleBar, TabRow with tab switching, StatusBar with
   zoom. Excel and PowerPoint agents migrate to it.
2. **Zero dead controls on Home**: wire or remove each row of §2.3.
3. **Other tabs**: File, Insert, Layout, Review, View (§2.2).
4. **Canvas and fonts** (§2.4), context menu, keyboard (§2.5).
5. **Pixel pass**: per-zone diff budget: title ≤3 %, tabs ≤3 %, ribbon ≤5 %,
   canvas ≤2 %, status ≤3 %.

## 4. Verification
- `/tmp/wh` harness: `shot.mjs` + `diff.py` per zone, against the reference.
- Behaviour suite (Playwright): one test per control in §2: click it, assert
  the document XML or engine state changed as Word would.
- Dead-control audit: the suite enumerates every `button` in the chrome and
  fails if one has no handler.
