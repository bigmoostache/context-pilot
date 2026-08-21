import { BadgeCheck, Bot, Cpu } from "lucide-react"

/** The three agent-configuration panes. */
export type TabId = "identity" | "llm" | "vitals"

/**
 * The three panes, in canonical order.
 *
 * ITS OWN MODULE, and not a const beside the panes it labels, for one hard
 * reason: `manageBody.tsx` exports components, and a file that exports both
 * components and values breaks React Fast Refresh (react-refresh
 * only-export-components, an error here). Two surfaces consume this list — the
 * manage DIALOG's rail and the settings VIEW's rail — and they must offer the
 * same three categories in the same order, so a second hand-kept copy is
 * exactly how they would drift apart.
 *
 * `blurb` is used only by the view, whose rail rows are two-line (mirroring a
 * thread row's title + preview). The dialog's narrower rail shows the label
 * alone.
 */
export const TABS: { id: TabId; label: string; icon: typeof Bot; blurb: string }[] = [
  { id: "identity", label: "Identity", icon: Bot, blurb: "How the agent sees itself" },
  { id: "llm", label: "Model", icon: Cpu, blurb: "Name, realm, provider and model" },
  { id: "vitals", label: "Vitals", icon: BadgeCheck, blurb: "Service health and lifecycle" },
]
