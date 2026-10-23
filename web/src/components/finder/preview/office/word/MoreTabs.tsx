import { Fragment } from "react"
import {
  ArrowNext20Regular,
  ArrowPrevious20Regular,
  CheckmarkCircle20Regular,
  Comment20Regular,
  DismissCircle20Regular,
  Document20Regular,
  DocumentPageBreak20Regular,
  DocumentSettings20Regular,
  Edit20Regular,
  Image20Regular,
  Link20Regular,
  Print20Regular,
  Save20Regular,
  Table20Regular,
  TextBulletListLtr20Regular,
} from "@fluentui/react-icons"
import type { EditorMode } from "@betteroffice/docx-react"
import { Combo, RibbonGroup } from "../fluent/controls"
import { W } from "./WordBtn"
import { useWordCmd } from "./wordCmd"

/** Insert tab: page break, table, picture, link, table of contents. */
export function InsertTab() {
  return (
    <>
      <RibbonGroup label="Pages">
        <W id="insertPageBreak" size="lg" icon={<DocumentPageBreak20Regular />} text="Page Break" />
      </RibbonGroup>
      <RibbonGroup label="Tables">
        <W
          id="insertTable"
          args={{ rows: 3, columns: 3 }}
          size="lg"
          icon={<Table20Regular />}
          text="Table"
        />
      </RibbonGroup>
      <RibbonGroup label="Illustrations">
        <W id="insertImage" size="lg" icon={<Image20Regular />} text="Picture" />
      </RibbonGroup>
      <RibbonGroup label="Links">
        <W id="insertLink" size="lg" icon={<Link20Regular />} text="Link" />
      </RibbonGroup>
      <RibbonGroup label="Contents" last>
        <W
          id="insertTOC"
          size="lg"
          icon={<TextBulletListLtr20Regular />}
          text="Table of Contents"
        />
      </RibbonGroup>
    </>
  )
}

/** Layout tab: page setup, section breaks, watermark. */
export function LayoutTab() {
  return (
    <>
      <RibbonGroup label="Page Setup">
        <W id="pageSetup" size="lg" icon={<DocumentSettings20Regular />} text="Page Setup" />
      </RibbonGroup>
      <RibbonGroup
        label="Breaks"
        rows={[
          <W
            key="n"
            id="insertSectionBreakNextPage"
            icon={<DocumentPageBreak20Regular />}
            text="Section Break (Next Page)"
          />,
          <W
            key="c"
            id="insertSectionBreakContinuous"
            icon={<DocumentPageBreak20Regular />}
            text="Section Break (Continuous)"
          />,
        ]}
      />
      <RibbonGroup label="Background" last>
        <W id="watermark" size="lg" icon={<Document20Regular />} text="Watermark" />
      </RibbonGroup>
    </>
  )
}

/** Review tab: Track Changes (suggesting mode), change navigation, comments pane. */
export function ReviewTab() {
  return (
    <>
      <RibbonGroup label="Tracking">
        <W
          id="editingMode"
          args={{ mode: "suggesting" }}
          pressedValue="suggesting"
          label="Track Changes"
          size="lg"
          icon={<Edit20Regular />}
          text="Track Changes"
        />
      </RibbonGroup>
      <RibbonGroup
        label="Changes"
        rows={[
          <Fragment key="r105">
            <W id="reviewAccept" args={null} icon={<CheckmarkCircle20Regular />} text="Accept" />
            <W id="reviewReject" args={null} icon={<DismissCircle20Regular />} text="Reject" />
          </Fragment>,
          <Fragment key="r109">
            <W id="reviewPrevious" icon={<ArrowPrevious20Regular />} text="Previous" />
            <W id="reviewNext" icon={<ArrowNext20Regular />} text="Next" />
          </Fragment>,
        ]}
      />
      <RibbonGroup label="Comments" last>
        <W id="commentsSidebar" size="lg" icon={<Comment20Regular />} text="Comments" />
      </RibbonGroup>
    </>
  )
}

/** File panel: Save and Print. */
export function FileBackstage() {
  return (
    <div style={{ display: "flex", flexDirection: "column", alignItems: "flex-start", gap: 8 }}>
      <W id="save" icon={<Save20Regular />} text="Save" />
      <W id="print" icon={<Print20Regular />} text="Print" />
    </div>
  )
}

const MODES: readonly EditorMode[] = ["editing", "suggesting", "viewing"]

const MODE_LABEL: Record<EditorMode, string> = {
  editing: "Editing",
  suggesting: "Reviewing",
  viewing: "Viewing",
}

/** Word's mode switch at the right of the tab row: Editing / Reviewing / Viewing. */
export function ModeCombo() {
  const mode = useWordCmd("editingMode")
  const current = mode.state.value ?? "editing"
  return (
    <Combo
      label="Editing mode"
      width={112}
      options={MODES.map((m) => MODE_LABEL[m])}
      value={MODE_LABEL[current]}
      onCommit={(t) => {
        const hit = MODES.find((m) => MODE_LABEL[m].toLowerCase() === t.toLowerCase())
        if (hit) void mode.execute({ mode: hit })
      }}
    />
  )
}
