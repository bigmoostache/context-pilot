import type { ReactNode } from "react"
import {
  AArrowDown,
  AArrowUp,
  AlignCenter,
  AlignJustify,
  AlignLeft,
  AlignRight,
  ArrowUpDown,
  Baseline,
  Bold,
  CaseSensitive,
  ClipboardPaste,
  Copy,
  Eraser,
  Grid2x2,
  Highlighter,
  IndentDecrease,
  IndentIncrease,
  Italic,
  LayoutGrid,
  List,
  ListChecks,
  ListOrdered,
  ListTree,
  Mic,
  MousePointer2,
  PaintBucket,
  Paintbrush,
  PenLine,
  Pilcrow,
  Redo2,
  Replace,
  Scissors,
  Search,
  Strikethrough,
  Subscript,
  Superscript,
  Underline,
  Undo2,
} from "lucide-react"
import { WORD } from "./wordTheme"
import { Align, BigButton, Cmd, FontBoxes, FontStep, GroupSep, Static } from "./wordButtons"

const I = 16

/**
 * Word for the web's classic two-row Home ribbon, passed to DocxEditor as
 * `toolbar` so it renders inside the editor's command context. Groups and
 * x-positions follow the live editor: Undo, Clipboard, Font, Paragraph,
 * Styles, Editing, Dictate, Editor, Add-ins.
 */
export function WordRibbon() {
  return (
    <div
      style={{
        height: WORD.ribbonH + WORD.ribbonGap,
        padding: `${WORD.ribbonGap}px 8px 0`,
        background: WORD.chrome,
        fontFamily: WORD.font,
        display: "flex",
        boxSizing: "border-box",
      }}
    >
      <div
        style={{
          flex: 1,
          display: "flex",
          alignItems: "stretch",
          padding: "0 8px",
          background: WORD.surface,
          borderRadius: 8,
          boxShadow: "0 0 2px rgba(0,0,0,0.12), 0 1px 2px rgba(0,0,0,0.14)",
          overflow: "hidden",
        }}
      >
        <Rows
          top={<Cmd id="undo" icon={<Undo2 size={I} />} />}
          bottom={<Cmd id="redo" icon={<Redo2 size={I} />} />}
        />
        <GroupSep />
        <Group>
          <BigButton label="Paste" icon={<ClipboardPaste size={24} />} />
          <Column>
            <Static label="Cut" icon={<Scissors size={I} />} />
            <Static label="Copy" icon={<Copy size={I} />} />
            <Static label="Format Painter" icon={<Paintbrush size={I} />} />
          </Column>
        </Group>
        <GroupSep />
        <Rows
          top={
            <>
              <FontBoxes />
              <FontStep delta={1} icon={<AArrowUp size={I} />} />
              <FontStep delta={-1} icon={<AArrowDown size={I} />} />
              <Cmd id="clearFormatting" icon={<Eraser size={I} />} />
            </>
          }
          bottom={
            <>
              <Cmd id="bold" icon={<Bold size={I} />} />
              <Cmd id="italic" icon={<Italic size={I} />} />
              <Cmd id="underline" icon={<Underline size={I} />} />
              <Cmd id="strikethrough" icon={<Strikethrough size={I} />} />
              <Cmd id="subscript" icon={<Subscript size={I} />} />
              <Cmd id="superscript" icon={<Superscript size={I} />} />
              <Static label="Change Case" icon={<CaseSensitive size={I} />} />
              <Static label="Highlight" icon={<Highlighter size={I} color="#c9a400" />} />
              <Static label="Font Color" icon={<Baseline size={I} color="#c50f1f" />} />
            </>
          }
        />
        <GroupSep />
        <Rows
          top={
            <>
              <Cmd id="bulletList" icon={<List size={I} />} />
              <Cmd id="numberedList" icon={<ListOrdered size={I} />} />
              <Static label="Multilevel List" icon={<ListTree size={I} />} />
              <Static label="Checklist" icon={<ListChecks size={I} />} />
              <Cmd id="outdent" icon={<IndentDecrease size={I} />} />
              <Cmd id="indent" icon={<IndentIncrease size={I} />} />
              <Static label="Show/Hide ¶" icon={<Pilcrow size={I} />} />
            </>
          }
          bottom={
            <>
              <Align value="left" icon={<AlignLeft size={I} />} />
              <Align value="center" icon={<AlignCenter size={I} />} />
              <Align value="right" icon={<AlignRight size={I} />} />
              <Align value="both" icon={<AlignJustify size={I} />} />
              <Static label="Line Spacing" icon={<ArrowUpDown size={I} />} />
              <Static label="Borders" icon={<Grid2x2 size={I} />} />
              <Static label="Shading" icon={<PaintBucket size={I} />} />
            </>
          }
        />
        <GroupSep />
        <StylePreview />
        <GroupSep />
        <Column>
          <Cmd id="find" icon={<Search size={I} />} text="Find" />
          <Cmd id="replace" icon={<Replace size={I} />} text="Replace" />
          <Static label="Select" icon={<MousePointer2 size={I} />} text="Select" />
        </Column>
        <GroupSep />
        <Group>
          <BigButton label="Dictate" icon={<Mic size={24} />} />
        </Group>
        <GroupSep />
        <Group>
          <BigButton label="Editor" icon={<PenLine size={24} color={WORD.brand} />} />
        </Group>
        <GroupSep />
        <Group>
          <BigButton label="Add-ins" icon={<LayoutGrid size={24} />} />
        </Group>
      </div>
    </div>
  )
}

/** Word's style gallery, collapsed to the current paragraph style card. */
function StylePreview() {
  return (
    <Group>
      <div
        style={{
          width: 120,
          height: 52,
          border: `1px solid ${WORD.line}`,
          borderRadius: 4,
          display: "flex",
          flexDirection: "column",
          justifyContent: "center",
          padding: "0 8px",
          boxSizing: "border-box",
        }}
      >
        <span style={{ fontSize: 15, color: WORD.ink }}>Normal</span>
        <span style={{ fontSize: 11, color: WORD.inkMuted }}>Styles</span>
      </div>
    </Group>
  )
}

function Group({ children }: { children: ReactNode }) {
  return <div style={{ display: "flex", alignItems: "center", gap: 4 }}>{children}</div>
}

function Column({ children }: { children: ReactNode }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", justifyContent: "center", gap: 2 }}>
      {children}
    </div>
  )
}

/** Two 24px rows (y 98 and 127 in Word), the classic ribbon layout. */
function Rows({ top, bottom }: { top: ReactNode; bottom: ReactNode }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", justifyContent: "center", gap: 5 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 0 }}>{top}</div>
      <div style={{ display: "flex", alignItems: "center", gap: 0 }}>{bottom}</div>
    </div>
  )
}
