import { Boxes, Coins, KeyRound, Package, ShieldCheck, Sliders } from "lucide-react"

/** Settings category identifiers (the cockpit config panes). */
export type CatId = "general" | "usage" | "services" | "secrets" | "it" | "releases"

/**
 * The config pane catalogue: order, labels, blurbs, icons and the `adminOnly` /
 * `superadminOnly` gates. Kept in its own module (not beside {@link CategoryBody})
 * so importing this data never trips Fast Refresh's component-only-export rule.
 */
export const CATEGORIES: {
  id: CatId
  label: string
  blurb: string
  icon: typeof Sliders
  count?: number
  adminOnly?: boolean
  /** Renders only for a superadmin (`can_manage_secrets`) — or in god-mode when
   *  access control is off (design §13.5/§13.10). */
  superadminOnly?: boolean
}[] = [
  { id: "general", label: "General", blurb: "Models & autonomy", icon: Sliders },
  { id: "usage", label: "Usage & Cost", blurb: "Spend & token analytics", icon: Coins },
  { id: "services", label: "Services", blurb: "Available integrations", icon: Boxes },
  {
    id: "secrets",
    label: "Secrets",
    blurb: "Provider API keys & Claude login",
    icon: KeyRound,
    superadminOnly: true,
  },
  {
    id: "it",
    label: "IT",
    blurb: "Network identity & TLS trust",
    icon: ShieldCheck,
    adminOnly: true,
  },
  {
    id: "releases",
    label: "Releases",
    blurb: "Manage binary versions",
    icon: Package,
    adminOnly: true,
  },
]
