import { useState } from "react"
import { useMutation, useQueryClient } from "@tanstack/react-query"
import { Loader2 } from "lucide-react"
import type { ItNetworkAp, ItNetworkResponse } from "@/lib/api/generated/types.gen"
import { apiErrorMessage, setItNetworkAp } from "@/lib/api"
import type { ApDraft } from "@/lib/api/it/network"
import {
  IT_NETWORK_KEY,
  apBody,
  apDraftFrom,
  apProblem,
  savedLabel,
  withAp,
} from "@/lib/api/it/network"
import { TextField, Toggle } from "./ItPane"

/**
 * Access-point form. The passphrase is write-only — the server never sends it
 * back, so the field starts empty and staying empty keeps the stored key.
 *
 * **This form is never remounted** (B8). It used to be keyed off the 5 s-polled
 * network query, so any persisted change — another session, the boot applier, a
 * concurrent save — remounted it mid-typing and silently discarded everything
 * entered, half-typed passphrase included. Instead it holds a PARTIAL draft of
 * only the fields the operator has touched, merged over the server's latest
 * value at render time: a touched field is theirs and a poll cannot take it
 * back, an untouched one keeps tracking the server, and clearing the draft after
 * a save snaps the whole form onto the server's own response. That last part is
 * also what empties the passphrase field and what lets the "Saved" confirmation
 * survive its own refetch (B9).
 *
 * Every rule that decides whether the button is live lives in
 * `@/lib/api/it/network` and mirrors `state.rs::validate_ap` exactly (B11).
 * The server is still the authority; this only buys a better message.
 */
export function ApForm({ ap }: { ap: ItNetworkAp }) {
  const qc = useQueryClient()
  const [draft, setDraft] = useState<Partial<ApDraft>>({})
  const form: ApDraft = { ...apDraftFrom(ap), ...draft }

  const save = useMutation({
    mutationFn: () => setItNetworkAp(apBody(form)),
    onSuccess: (result) => {
      qc.setQueryData<ItNetworkResponse>(IT_NETWORK_KEY, (previous) => withAp(previous, result.ap))
      setDraft({})
      void qc.invalidateQueries({ queryKey: IT_NETWORK_KEY })
    },
  })

  /** Edit one field. Clearing the save banner on the first keystroke keeps a
   *  stale "Saved" from sitting under a form that no longer matches it. */
  const edit =
    <K extends keyof ApDraft>(field: K) =>
    (value: ApDraft[K]) => {
      save.reset()
      setDraft((previous) => ({ ...previous, [field]: value }))
    }

  const problem = apProblem(form, ap.passphrase_set)

  return (
    <div className="rounded-xl border border-border bg-card px-3.5 py-3">
      <form
        className="flex flex-col gap-2.5"
        onSubmit={(event) => {
          event.preventDefault()
          if (problem === null && !save.isPending) save.mutate()
        }}
      >
        <Toggle
          label="Broadcast the access point"
          checked={form.enabled}
          onChange={edit("enabled")}
        />
        <TextField
          label="Network name (SSID)"
          hint="1–32 bytes"
          value={form.ssid}
          onChange={edit("ssid")}
          placeholder="ContextPilot"
        />
        <TextField
          label="Passphrase"
          hint={ap.passphrase_set ? "set — leave empty to keep it" : "8–63 characters"}
          value={form.passphrase}
          onChange={edit("passphrase")}
          placeholder={ap.passphrase_set ? "••••••••" : "at least 8 characters"}
          type="password"
          autoComplete="new-password"
        />
        <div className="flex gap-2.5">
          <label className="flex flex-1 flex-col gap-1">
            <span className="text-[12px] font-medium text-foreground/90">Band</span>
            <select
              value={form.band}
              onChange={(event) => edit("band")(event.target.value === "bg" ? "bg" : "a")}
              className="w-full rounded-md border border-border bg-muted/50 px-2.5 py-1.5 text-[12px] text-foreground focus:ring-1 focus:ring-(--interactive) focus:outline-none"
            >
              <option value="a">5 GHz</option>
              <option value="bg">2.4 GHz</option>
            </select>
          </label>
          <div className="flex-1">
            <TextField
              label="Channel"
              hint="0 = auto"
              value={form.channel}
              onChange={edit("channel")}
              placeholder="0"
              inputMode="numeric"
            />
          </div>
          <div className="flex-1">
            <TextField
              label="Country"
              hint="required"
              value={form.country}
              onChange={edit("country")}
              placeholder="FR"
            />
          </div>
        </div>
        <Toggle label="Hide the network name" checked={form.hidden} onChange={edit("hidden")} />
        <Toggle
          label="Share this box's internet with Wi-Fi clients"
          checked={form.shareInternet}
          onChange={edit("shareInternet")}
        />
        <p className="text-[11px] text-muted-foreground">
          {form.shareInternet
            ? "Wi-Fi clients get an address, DNS and internet through this box — it becomes a router on your site."
            : "Wi-Fi clients get an address and can open the cockpit, but no traffic is forwarded to the internet."}
        </p>

        <div className="flex items-center gap-2">
          <button
            type="submit"
            disabled={problem !== null || save.isPending}
            className="flex items-center gap-1.5 rounded-md bg-(--interactive) px-3 py-1.5 text-[12px] font-medium text-(--primary-foreground) transition-all hover:brightness-105 disabled:opacity-50"
          >
            {save.isPending && <Loader2 className="size-3.5 animate-spin" />}
            Save access point
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
