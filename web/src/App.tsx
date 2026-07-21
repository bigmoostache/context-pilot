import { lazy, Suspense } from "react"
import { AppSkeleton } from "@/switch/AppSkeleton"
import { useIsMobile } from "@/switch/useIsMobile"

// The two component trees, code-split. Only the active tree's dynamic chunk is
// fetched at runtime (Vite splits on the `import()`); both are compiled at build
// time. Hoisted to module scope so a re-render never re-creates `lazy()` (which
// would remount + refetch the chunk) — the switch is resolved once, at load.
const DesktopRoot = lazy(() => import("@/components/Root"))
const MobileRoot = lazy(() => import("@/mobile-components/Root"))

/**
 * Device switch — the ONE seam that selects the desktop or mobile component
 * tree. Everything above it (`QueryClientProvider` in `main.tsx`) is shared and
 * survives the choice; everything below is the chosen tree.
 *
 * {@link useIsMobile} resolves once at first paint and is frozen for the session
 * (no live-resize remount — design §6), so the ternary picks a stable tree. The
 * `Suspense` boundary paints {@link AppSkeleton} while the chosen tree's chunk
 * loads.
 */
export default function App() {
  const Root = useIsMobile() ? MobileRoot : DesktopRoot
  return (
    <Suspense fallback={<AppSkeleton />}>
      <Root />
    </Suspense>
  )
}
