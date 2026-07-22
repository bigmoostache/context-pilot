import { useState } from "react"
import { Check, X } from "lucide-react"
import { DialogClose } from "@/mobile-components/ui/dialog"
import { cn } from "@/lib/utils"
import { useAuth } from "@/lib/providers/auth"
import { CategoryBody } from "./ConfigPanes"
import { type CatId, CATEGORIES } from "./categories"

/**
 * Context Pilot settings surface — mobile twin of `components/shell/config/
 * ConfigPanel`.
 *
 * The desktop twin is a macOS System-Settings layout: a fixed 230px category
 * **rail on the left** beside the detail pane. A phone has no room for a
 * side-by-side rail, so the mobile fork moves category selection to a
 * **horizontal scrollable tab strip across the top**, with the active pane
 * filling the full width beneath it. Same category set, same visibility gates
 * (admin/superadmin), same footer — only the navigation axis rotates from
 * vertical rail to horizontal strip.
 *
 * The category panes + presentational building blocks live in `./ConfigPanes`
 * (the mobile twin, relative import); this file owns only the strip + chrome.
 */
export function ConfigPanel({ variant = "dialog" }: { variant?: "dialog" | "inline" }) {
  const [cat, setCat] = useState<CatId>("general")
  const { user: authUser, authEnabled } = useAuth()
  // `adminOnly` categories (IT, Update) are gated on `can_manage_it` (admin+):
  // an `admin` OR a `superadmin` (which subsumes it), or god-mode when access
  // control is off (design §13.5/§13.10).
  const isAdmin =
    authEnabled === false || authUser?.role === "admin" || authUser?.role === "superadmin"
  // Secrets is superadmin-only (`can_manage_secrets`). In god-mode (access
  // control off ⇒ authEnabled false, no user) the single-user appliance viewer
  // is treated as superadmin so it can still manage keys (design §13.5/§13.10).
  const isSuperadmin = authEnabled === false || authUser?.role === "superadmin"
  const visibleCategories = CATEGORIES.filter(
    (c) => (!c.adminOnly || isAdmin) && (!c.superadminOnly || isSuperadmin),
  )
  const inline = variant === "inline"

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {/* category strip — the mobile replacement for the desktop side rail:
          a horizontally scrollable row of pill tabs. */}
      <div
        className={cn(
          "flex shrink-0 items-center gap-1.5 border-b border-border/70 px-3",
          inline ? "bg-surface" : "bg-muted/30",
        )}
      >
        <nav className="no-scrollbar flex min-w-0 flex-1 items-center gap-1.5 overflow-x-auto py-2.5">
          {visibleCategories.map((c) => {
            const on = c.id === cat
            return (
              <button
                key={c.id}
                onClick={() => setCat(c.id)}
                className={cn(
                  "flex shrink-0 items-center gap-1.5 rounded-full px-3 py-2 text-[13px] transition-colors",
                  on
                    ? "card-shadow bg-card font-medium text-foreground"
                    : "text-foreground/70 active:bg-muted/60",
                )}
              >
                <c.icon
                  className={cn(
                    "size-[15px]",
                    on ? "text-(--interactive)" : "text-muted-foreground/70",
                  )}
                />
                <span className="whitespace-nowrap">{c.label}</span>
                {c.count != null && (
                  <span className="rounded-full bg-muted/70 px-1.5 py-px text-[9.5px] font-semibold text-muted-foreground tabular-nums">
                    {c.count}
                  </span>
                )}
              </button>
            )
          })}
        </nav>
        {/* Close control sits at the strip's end in the dialog variant. */}
        {!inline && (
          <DialogClose
            className="flex size-9 shrink-0 items-center justify-center rounded-md text-muted-foreground/55 transition-colors active:bg-muted/70 active:text-foreground"
            aria-label="Close"
          >
            <X className="size-5" />
          </DialogClose>
        )}
      </div>

      {/* detail pane — full width beneath the strip */}
      <main className="flex min-h-0 flex-1 flex-col">
        {cat === "usage" ? (
          <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
            <CategoryBody cat={cat} />
          </div>
        ) : (
          <div className="min-h-0 flex-1 overflow-y-auto p-4">
            <CategoryBody cat={cat} />
          </div>
        )}

        <footer className="flex h-[62px] shrink-0 items-center border-t border-border/70 bg-muted/25 px-4">
          <span className="text-[11.5px] text-muted-foreground/70">Changes apply on save.</span>
          {inline ? (
            <button className="ml-auto flex items-center gap-2 rounded-lg bg-(--interactive) px-4 py-2.5 text-[13px] font-medium text-(--primary-foreground) transition-[filter] active:brightness-105">
              <Check className="size-4" strokeWidth={2.5} />
              Save
            </button>
          ) : (
            <DialogClose className="ml-auto flex items-center gap-2 rounded-lg bg-(--interactive) px-4 py-2.5 text-[13px] font-medium text-(--primary-foreground) transition-[filter] active:brightness-105">
              <Check className="size-4" strokeWidth={2.5} />
              Done
            </DialogClose>
          )}
        </footer>
      </main>
    </div>
  )
}
