import { Boxes, Coins, KeyRound, Package, ShieldCheck, Sliders } from "lucide-react"

// Split out of `ConfigPanes.tsx` so that component file only exports components
// (React Fast Refresh). Consumed by both `ConfigPanes` and `ConfigPanel`.

// ── categories ────────────────────────────────────────────────────
export type CatId = "general" | "usage" | "services" | "secrets" | "it" | "releases"

export const CATEGORIES: {
  id: CatId
  label: string
  blurb: string
  icon: typeof Sliders
  count?: number
  adminOnly?: boolean
  /** Superadmin-only (`can_manage_secrets`), or god-mode when access control off. */
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
