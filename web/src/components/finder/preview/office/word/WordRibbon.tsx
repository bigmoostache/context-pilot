import { OfficeRibbon } from "../fluent/Frame"
import { StatusBar, Zoom } from "../fluent/StatusBar"
import type { OfficeTab, SearchItem } from "../fluent/TabRow"
import { HomeTab } from "./HomeTab"
import { FileBackstage, InsertTab, LayoutTab, ModeCombo, ReviewTab } from "./MoreTabs"
import { plain, useWordCmd } from "./wordCmd"

const TABS: readonly OfficeTab[] = [
  { id: "home", label: "Home", ribbon: <HomeTab /> },
  { id: "insert", label: "Insert", ribbon: <InsertTab /> },
  { id: "layout", label: "Layout", ribbon: <LayoutTab /> },
  { id: "review", label: "Review", ribbon: <ReviewTab /> },
]

/**
 * Word for the web's ribbon on the shared kit, passed to DocxEditor as
 * `toolbar` so every control resolves docx commands. Unsupported tabs and
 * Microsoft cloud controls are left out (user decision).
 */
export function WordRibbon() {
  const commands = useSearchItems()
  return (
    <OfficeRibbon
      layout="classic"
      tabs={TABS}
      defaultTab="home"
      backstage={<FileBackstage />}
      tabsRight={<ModeCombo />}
      commands={commands}
      status={<WordStatus />}
    />
  )
}

function WordStatus() {
  const zoom = useWordCmd("zoom")
  return (
    <StatusBar
      right={<Zoom cmd={zoom} toArgs={(s) => ({ scale: s === "fit" ? 1 : s })} fit={false} />}
    />
  )
}

/** Commands reachable from the title-bar search. */
function useSearchItems(): SearchItem[] {
  const items: [string, ReturnType<typeof plain>, string[]?][] = [
    ["Bold", plain(useWordCmd("bold"), null)],
    ["Italic", plain(useWordCmd("italic"), null)],
    ["Underline", plain(useWordCmd("underline"), null)],
    ["Strikethrough", plain(useWordCmd("strikethrough"), null)],
    ["Clear Formatting", plain(useWordCmd("clearFormatting"), null)],
    ["Bullets", plain(useWordCmd("bulletList"), null), ["list"]],
    ["Numbering", plain(useWordCmd("numberedList"), null), ["list"]],
    ["Find", plain(useWordCmd("find"), null), ["search"]],
    ["Replace", plain(useWordCmd("replace"), null)],
    ["Page Break", plain(useWordCmd("insertPageBreak"), null), ["insert"]],
    ["Picture", plain(useWordCmd("insertImage"), null), ["image", "insert"]],
    ["Link", plain(useWordCmd("insertLink"), null), ["hyperlink", "insert"]],
    ["Table of Contents", plain(useWordCmd("insertTOC"), null), ["toc"]],
    ["Page Setup", plain(useWordCmd("pageSetup"), null), ["margins", "orientation"]],
    ["Watermark", plain(useWordCmd("watermark"), null)],
    ["Comments", plain(useWordCmd("commentsSidebar"), null)],
    ["Save", plain(useWordCmd("save"), null)],
    ["Print", plain(useWordCmd("print"), null)],
  ]
  return items.map(([label, cmd, keywords]) => ({ label, cmd, keywords }))
}
