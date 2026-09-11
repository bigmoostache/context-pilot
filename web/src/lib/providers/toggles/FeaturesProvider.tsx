import { useMemo } from "react"
import { useQuery } from "@tanstack/react-query"
import { fetchFeatures } from "@/lib/api"
import { FEATURE_DEFAULTS, FeaturesContext } from "./features"

/**
 * Fetches `GET /api/features` once per session and provides it to the tree.
 *
 * Flags come from the deployment's environment and only change on a backend
 * restart, so the query never goes stale. A failed fetch (backend down) falls
 * back to the table defaults and still counts as loaded — the same posture
 * `AuthProvider` takes when the status probe fails, so the app never locks
 * itself out on a transient outage.
 */
export function FeaturesProvider({ children }: { children: React.ReactNode }) {
  const query = useQuery({
    queryKey: ["features"],
    queryFn: fetchFeatures,
    staleTime: Infinity,
    retry: 1,
  })
  const value = useMemo(
    () => ({
      features: query.data ?? FEATURE_DEFAULTS,
      loaded: query.isSuccess || query.isError,
    }),
    [query.data, query.isSuccess, query.isError],
  )
  return <FeaturesContext value={value}>{children}</FeaturesContext>
}
