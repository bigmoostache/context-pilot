import { Boxes, Coins, Package, Sliders } from "lucide-react"

/** Settings category identifiers (the four cockpit config panes). */
export type CatId = "general" | "usage" | "services" | "releases"

/**
 * The config pane catalogue: order, labels, blurbs, icons and the `adminOnly`
 * gate. Kept in its own module (not beside {@link CategoryBody}) so importing
 * this data never trips Fast Refresh's component-only-export rule.
 */
export const CATEGORIES: {
  id: CatId
  label: string
  blurb: string
  icon: typeof Sliders
  count?: number
  adminOnly?: boolean
}[] = [
  { id: "general", label: "General", blurb: "Models & autonomy", icon: Sliders },
  { id: "usage", label: "Usage & Cost", blurb: "Spend & token analytics", icon: Coins },
  { id: "services", label: "Services", blurb: "Available integrations", icon: Boxes },
  {
    id: "releases",
    label: "Releases",
    blurb: "Manage binary versions",
    icon: Package,
    adminOnly: true,
  },
]
