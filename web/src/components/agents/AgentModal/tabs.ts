import { BadgeCheck, Cpu } from "lucide-react"

/** The two agent-configuration panes. */
export type TabId = "llm" | "vitals"

/**
 * The two panes, in canonical order.
 *
 * ITS OWN MODULE, and not a const beside the panes it labels, for one hard
 * reason: `manageBody.tsx` exports components, and a file that exports both
 * components and values breaks React Fast Refresh (react-refresh
 * only-export-components, an error here). The manage DIALOG's rail reads this
 * list, so it and the settings VIEW's two sections can never offer different
 * categories.
 *
 * The `blurb` these rows once carried is GONE (T757). It existed for the
 * settings view's two-line rail rows and had no reader once that rail went:
 * the view renders both panes on one scrolling page, and {@link VitalsTab}
 * leads with SessionVitals' own "Service vitals" heading, so a second label
 * above it would read as two headings for one board.
 */
export const TABS: { id: TabId; label: string; icon: typeof Cpu }[] = [
  { id: "llm", label: "Model", icon: Cpu },
  { id: "vitals", label: "Vitals", icon: BadgeCheck },
]
