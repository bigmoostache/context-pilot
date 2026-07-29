import { useState } from "react"
import { useMutation, useQueryClient } from "@tanstack/react-query"
import { Loader2 } from "lucide-react"
import type { ItNetworkWwan } from "@/lib/api/generated/types.gen"
import { setItNetworkWwan } from "@/lib/api"
import { TextField, Toggle } from "./ItPane"

/** 5G bearer form (FR-NET-15). `password` and `pin` are write-only, with the
 *  same omit-to-keep rule as the AP passphrase. */
export function WwanForm({ initial }: { initial: ItNetworkWwan }) {
  const qc = useQueryClient()
  const [apn, setApn] = useState(initial.apn)
  const [username, setUsername] = useState(initial.username ?? "")
  const [password, setPassword] = useState("")
  const [pin, setPin] = useState("")
  const [roaming, setRoaming] = useState(initial.roaming)
  const [standby, setStandby] = useState<ItNetworkWwan["standby"]>(initial.standby)

  const save = useMutation({
    mutationFn: () =>
      setItNetworkWwan({
        apn: apn.trim(),
        username: username.trim() === "" ? null : username.trim(),
        ...(password !== "" && { password }),
        ...(pin !== "" && { pin }),
        roaming,
        standby,
      }),
    onSuccess: () => {
      setPassword("")
      setPin("")
      void qc.invalidateQueries({ queryKey: ["it-network"] })
    },
  })

  return (
    <div className="rounded-xl border border-border bg-card px-3.5 py-3">
      <form
        className="flex flex-col gap-2.5"
        onSubmit={(event) => {
          event.preventDefault()
          if (!save.isPending) save.mutate()
        }}
      >
        <TextField
          label="APN"
          hint="from your carrier"
          value={apn}
          onChange={setApn}
          placeholder="orange.fr"
        />
        <TextField
          label="Username"
          hint="optional"
          value={username}
          onChange={setUsername}
          placeholder=""
        />
        <TextField
          label="Password"
          hint={initial.password_set ? "set — leave empty to keep it" : "optional"}
          value={password}
          onChange={setPassword}
          placeholder={initial.password_set ? "••••••••" : ""}
        />
        <TextField
          label="SIM PIN"
          hint={initial.pin_set ? "set — leave empty to keep it" : "only if the SIM asks for one"}
          value={pin}
          onChange={setPin}
          placeholder={initial.pin_set ? "••••" : ""}
        />
        <Toggle label="Allow roaming networks" checked={roaming} onChange={setRoaming} />
        <label className="flex flex-col gap-1">
          <span className="text-[12px] font-medium text-foreground/90">Standby</span>
          <select
            value={standby}
            onChange={(event) => setStandby(event.target.value === "cold" ? "cold" : "hot")}
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
            disabled={save.isPending}
            className="flex items-center gap-1.5 rounded-md bg-(--interactive) px-3.5 py-2.5 text-[13px] font-medium text-(--primary-foreground) transition-all active:brightness-105 disabled:opacity-50"
          >
            {save.isPending && <Loader2 className="size-3.5 animate-spin" />}
            Save 5G settings
          </button>
          {save.isSuccess && <span className="text-[11px] text-(--ok)">Saved</span>}
          {save.isError && (
            <span className="text-[11px] text-red-500">
              {save.error instanceof Error ? save.error.message : "Save failed"}
            </span>
          )}
        </div>
      </form>
    </div>
  )
}
