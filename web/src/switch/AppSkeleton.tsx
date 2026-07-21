import { cn } from "@/lib/utils"

/**
 * Suspense fallback shown while the active component tree's dynamic chunk
 * (desktop or mobile) is still loading — see `App.tsx`.
 *
 * Deliberately self-contained: it imports NOTHING from `@/components` (the
 * mirrored tree it is a fallback FOR), so it can paint before either tree's
 * chunk resolves. Only `cn` + Tailwind primitives. It mimics the shell bands
 * (top bar · body · status bar) so the first frame isn't a blank screen.
 */
function Bar({ className }: { className?: string }) {
  return <div className={cn("animate-pulse rounded-md bg-muted", className)} />
}

export function AppSkeleton() {
  return (
    <div className="flex h-screen w-screen flex-col overflow-hidden bg-background text-foreground">
      {/* Top bar band */}
      <div className="flex h-12 shrink-0 items-center gap-3 border-b border-border px-4">
        <Bar className="size-6" />
        <Bar className="h-4 w-32" />
        <div className="flex-1" />
        <Bar className="size-6" />
      </div>

      {/* Body */}
      <div className="flex flex-1 items-center justify-center">
        <Bar className="size-8 rounded-full" />
      </div>

      {/* Status bar band */}
      <div className="flex h-6 shrink-0 items-center gap-2 border-t border-border px-3">
        <Bar className="h-3 w-24" />
      </div>
    </div>
  )
}
