import { useState } from "react"
import { Check, X } from "lucide-react"
import { DialogClose } from "@/components/ui/dialog"
import { cn } from "@/lib/utils"
import { useAuth } from "@/lib/providers/auth"
import { CategoryBody } from "./ConfigPanes"
import { type CatId, CATEGORIES } from "./categories"

/**
 * Context Pilot settings surface — a macOS System-Settings-style layout with a
 * category rail on the left and a detail pane on the right. Design-only: every
 * key/value is illustrative, nothing is persisted.
 *
 * This is the **shared body**, decoupled from any container. Two consumers:
 *  - {@link ConfigModal} wraps it in a portaled Dialog (the TopBar gear, used
 *    inside an agent) — pass `variant="dialog"` so the header/footer render
 *    `DialogClose` controls.
 *  - The fleet dashboard renders it **inline** as the "Settings" page — pass
 *    `variant="inline"` so it fills the page and drops the dialog-only chrome.
 *
 * The category panes + presentational building blocks live in `./ConfigPanes`
 * (split out for the file-size limit); this file owns only the rail + chrome.
 */
export function ConfigPanel({ variant = "dialog" }: { variant?: "dialog" | "inline" }) {
  const [cat, setCat] = useState<CatId>("general")
  const { user: authUser, authEnabled } = useAuth()
  // `adminOnly` categories (IT, Releases) are gated on `can_manage_it` (admin+):
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
    <div className="flex min-h-0 flex-1">
      {/* category rail */}
      <aside
        className={cn(
          "flex w-[230px] shrink-0 flex-col border-r border-border/70",
          inline ? "bg-surface" : "bg-muted/30",
        )}
      >
        <nav className="flex min-h-0 flex-1 flex-col gap-0.5 overflow-y-auto px-2.5 py-3.5">
          {visibleCategories.map((c) => {
            const on = c.id === cat
            return (
              <button
                key={c.id}
                onClick={() => setCat(c.id)}
                className={cn(
                  "group flex items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-[12.5px] transition-colors",
                  on
                    ? "card-shadow bg-card font-medium text-foreground"
                    : "text-foreground/75 hover:bg-muted/60",
                )}
              >
                <span
                  className={cn(
                    "flex size-6 shrink-0 items-center justify-center rounded-md transition-colors",
                    on ? "bg-(--interactive)/15 text-(--interactive)" : "text-muted-foreground/70",
                  )}
                >
                  <c.icon className="size-[15px]" />
                </span>
                <span className="min-w-0 flex-1 truncate">{c.label}</span>
                {c.count != null && (
                  <span className="shrink-0 rounded-full bg-muted/70 px-1.5 py-px text-[9.5px] font-semibold text-muted-foreground tabular-nums">
                    {c.count}
                  </span>
                )}
              </button>
            )
          })}
        </nav>
      </aside>

      {/* detail pane */}
      <main className="flex min-w-0 flex-1 flex-col">
        {/* No per-category title here — the focused item in the left rail
            already names the page, so repeating it as a heading is noise.
            Dialog variant keeps just a slim bar holding the close control. */}
        {!inline && (
          <header className="flex h-[46px] shrink-0 items-center justify-end border-b border-border/70 px-3">
            <DialogClose
              className="flex size-7 items-center justify-center rounded-md text-muted-foreground/55 transition-colors hover:bg-muted/70 hover:text-foreground"
              aria-label="Close"
            >
              <X className="size-4" />
            </DialogClose>
          </header>
        )}

        {cat === "usage" ? (
          <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
            <CategoryBody cat={cat} />
          </div>
        ) : (
          <div className="min-h-0 flex-1 overflow-y-auto px-6 py-5">
            <CategoryBody cat={cat} />
          </div>
        )}

        <footer className="flex h-[58px] shrink-0 items-center border-t border-border/70 bg-muted/25 px-6">
          <span className="text-[11.5px] text-muted-foreground/70">Changes apply on save.</span>
          {inline ? (
            <button className="ml-auto flex items-center gap-2 rounded-lg bg-(--interactive) px-4 py-2 text-[13px] font-medium text-(--primary-foreground) transition-all hover:brightness-105 active:scale-[0.98]">
              <Check className="size-4" strokeWidth={2.5} />
              Save
            </button>
          ) : (
            <DialogClose className="ml-auto flex items-center gap-2 rounded-lg bg-(--interactive) px-4 py-2 text-[13px] font-medium text-(--primary-foreground) transition-all hover:brightness-105 active:scale-[0.98]">
              <Check className="size-4" strokeWidth={2.5} />
              Done
            </DialogClose>
          )}
        </footer>
      </main>
    </div>
  )
}
