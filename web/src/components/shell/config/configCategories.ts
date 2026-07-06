import { Boxes, Coins, Package, Sliders } from "lucide-react"

// Split out of `ConfigPanes.tsx` so that component file only exports components
// (React Fast Refresh). Consumed by both `ConfigPanes` and `ConfigPanel`.

// ── categories ────────────────────────────────────────────────────
export type CatId = "general" | "usage" | "services" | "releases"

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
  { id: "releases", label: "Releases", blurb: "Manage binary versions", icon: Package, adminOnly: true },
]
