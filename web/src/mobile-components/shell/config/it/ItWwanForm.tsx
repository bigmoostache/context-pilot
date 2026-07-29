import { useState } from "react"
import { useMutation, useQueryClient } from "@tanstack/react-query"
import { Loader2 } from "lucide-react"
import type { ItNetworkResponse, ItNetworkWwan } from "@/lib/api/generated/types.gen"
import { apiErrorMessage, setItNetworkWwan } from "@/lib/api"
import type { WwanDraft } from "@/lib/api/it/network"
import {
  IT_NETWORK_KEY,
  savedLabel,
  withWwan,
  wwanBody,
  wwanDraftFrom,
  wwanProblem,
} from "@/lib/api/it/network"
import { TextField, Toggle } from "./ItPane"

/**
 * 5G bearer form (FR-NET-15) — mobile twin of
 * `components/shell/config/it/ItWwanForm`. `password` and `pin` are write-only,
 * with the same omit-to-keep rule as the AP passphrase.
 *
 * Divergence from desktop is touch-only: a 16px `<select>` font (iOS Safari
 * auto-zooms the viewport on focus below 16px) and a taller submit button with
 * `active:` in place of `hover:`.
 *
 * Same draft-over-server-value shape as {@link ApForm}, and for the same reason:
 * this form is never remounted, so a poll tick cannot eat a half-typed PIN and
 * the "Saved" confirmation outlives the refetch that follows it (B8/B9). The PIN
 * is validated before it leaves the browser — a wrong one could burn one of the
 * SIM's three unlock retries (B11).
 */
export function WwanForm({ wwan }: { wwan: ItNetworkWwan }) {
  const qc = useQueryClient()
  const [draft, setDraft] = useState<Partial<WwanDraft>>({})
  const form: WwanDraft = { ...wwanDraftFrom(wwan), ...draft }

  const save = useMutation({
    mutationFn: () => setItNetworkWwan(wwanBody(form)),
    onSuccess: (result) => {
      qc.setQueryData<ItNetworkResponse>(IT_NETWORK_KEY, (previous) =>
        withWwan(previous, result.wwan),
      )
      setDraft({})
      void qc.invalidateQueries({ queryKey: IT_NETWORK_KEY })
    },
  })

  const edit =
    <K extends keyof WwanDraft>(field: K) =>
    (value: WwanDraft[K]) => {
      save.reset()
      setDraft((previous) => ({ ...previous, [field]: value }))
    }

  const problem = wwanProblem(form)

  return (
    <div className="rounded-xl border border-border bg-card px-3.5 py-3">
      <form
        className="flex flex-col gap-2.5"
        onSubmit={(event) => {
          event.preventDefault()
          if (problem === null && !save.isPending) save.mutate()
        }}
      >
        <TextField
          label="APN"
          hint="from your carrier"
          value={form.apn}
          onChange={edit("apn")}
          placeholder="orange.fr"
        />
        <TextField
          label="Username"
          hint="optional"
          value={form.username}
          onChange={edit("username")}
          placeholder=""
        />
        <TextField
          label="Password"
          hint={wwan.password_set ? "set — leave empty to keep it" : "optional"}
          value={form.password}
          onChange={edit("password")}
          placeholder={wwan.password_set ? "••••••••" : ""}
          type="password"
          autoComplete="new-password"
        />
        <TextField
          label="SIM PIN"
          hint={wwan.pin_set ? "set — leave empty to keep it" : "4–8 digits, only if the SIM asks"}
          value={form.pin}
          onChange={edit("pin")}
          placeholder={wwan.pin_set ? "••••" : ""}
          type="password"
          autoComplete="new-password"
          inputMode="numeric"
        />
        <Toggle label="Allow roaming networks" checked={form.roaming} onChange={edit("roaming")} />
        <label className="flex flex-col gap-1">
          <span className="text-[12px] font-medium text-foreground/90">Standby</span>
          <select
            value={form.standby}
            onChange={(event) => edit("standby")(event.target.value === "cold" ? "cold" : "hot")}
            className="w-full rounded-md border border-border bg-muted/50 px-2.5 py-2 text-[16px] text-foreground focus:ring-1 focus:ring-(--interactive) focus:outline-none"
          >
            <option value="hot">Hot — instant failover, the SIM stays attached</option>
            <option value="cold">Cold — slower failover, less data used</option>
          </select>
        </label>
        <p className="text-[11px] text-muted-foreground">
          Hot standby keeps the SIM attached and uses a little data on keep-alive. Choose cold on a
          metered plan.
        </p>
        <div className="flex items-center gap-2">
          <button
            type="submit"
            disabled={problem !== null || save.isPending}
            className="flex items-center gap-1.5 rounded-md bg-(--interactive) px-3.5 py-2.5 text-[13px] font-medium text-(--primary-foreground) transition-all active:brightness-105 disabled:opacity-50"
          >
            {save.isPending && <Loader2 className="size-3.5 animate-spin" />}
            Save 5G settings
          </button>
          {problem !== null && <span className="text-[11px] text-muted-foreground">{problem}</span>}
          {save.isSuccess && (
            <span className="text-[11px] text-(--ok)">{savedLabel(save.data.applied)}</span>
          )}
          {save.isError && (
            <span className="text-[11px] text-red-500">
              {apiErrorMessage(save.error, "Save failed")}
            </span>
          )}
        </div>
      </form>
    </div>
  )
}
