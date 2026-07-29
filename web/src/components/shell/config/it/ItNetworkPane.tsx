import type { ReactNode } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { Loader2, RefreshCw } from "lucide-react"
import type {
  ItNetworkConfig,
  ItNetworkProbe,
  ItNetworkResponse,
  ItNetworkStatus,
  ItNetworkSupervisor,
} from "@/lib/api/generated/types.gen"
import { apiErrorMessage, fetchItNetwork, setItNetworkMode } from "@/lib/api"
import { IT_NETWORK_KEY, POLL_MS, modeRows, savedLabel, withMode } from "@/lib/api/it/network"
import type { StatusRow } from "@/lib/api/it/networkStatus"
import {
  activeUplinkLabel,
  apLine,
  probeRows,
  supervisorView,
  wanLine,
  wwanLine,
} from "@/lib/api/it/networkStatus"
import { cn } from "@/lib/utils"
import { SectionLabel } from "./ItPane"
import { ApForm } from "./ItApForm"
import { WwanForm } from "./ItWwanForm"

/**
 * Internet uplink + Wi-Fi access point.
 *
 * A sibling of {@link ItPane}, mounted next to it by `ConfigPanes`, so it
 * inherits the same `can_manage_it` gate: the caller only renders the IT
 * category for admin+, and the backend answers 403 to anyone else regardless.
 * The client-side gating is cosmetic; the server is the authority.
 *
 * Everything with no styling in it — the mode table, the validation mirror, the
 * status/probe/supervisor formatting — lives in `@/lib/api/it/network`, so
 * this file and its mobile twin differ only in Tailwind classes (C8).
 *
 * Both forms write through the API layer, whose secret handling is what the UI
 * has to respect: the passphrase is **write-only**, so the server never sends it
 * back and the field starts empty on every load. Leaving it empty keeps the
 * stored key; typing replaces it.
 */
export function ItNetworkPane() {
  const { data, isLoading, isError, error, refetch } = useQuery({
    queryKey: IT_NETWORK_KEY,
    queryFn: fetchItNetwork,
    refetchInterval: POLL_MS,
  })

  // R6: the read can fail, and it used to render "Loading…" forever while the
  // 5 s poll retried in silence — a 403, a 500 and an unreachable box all
  // looked like a slow load. With data in hand a failed poll is a banner
  // instead (below), because one bad tick must not tear down a working pane.
  if (isError && data === undefined) {
    return (
      <PaneFrame>
        <div className="flex flex-col items-start gap-2 py-1">
          <span className="text-[12px] text-(--danger)">
            {apiErrorMessage(error, "Could not read this box's network settings.")}
          </span>
          <button
            type="button"
            onClick={() => void refetch()}
            className="flex items-center gap-1 rounded-md px-2 py-1 text-[11px] text-muted-foreground transition-colors hover:bg-muted/60 hover:text-foreground"
          >
            <RefreshCw className="size-3" />
            Retry
          </button>
        </div>
      </PaneFrame>
    )
  }

  if (isLoading || data === undefined) {
    return (
      <PaneFrame>
        <div className="flex items-center gap-2 py-1 text-[12px] text-muted-foreground">
          <Loader2 className="size-3.5 animate-spin" /> Loading…
        </div>
      </PaneFrame>
    )
  }

  return <Loaded data={data} stale={isError ? apiErrorMessage(error, "unreachable") : null} />
}

/** The pane's section chrome, shared by the loading and failure states. */
function PaneFrame({ children }: { children: ReactNode }) {
  return (
    <section className="flex flex-col gap-2">
      <SectionLabel label="Internet uplink" hint="Ethernet, 5G, or failover" />
      <div className="rounded-xl border border-border bg-card px-3.5 py-3">{children}</div>
    </section>
  )
}

/** The pane proper, once the first read has landed. */
function Loaded({ data, stale }: { data: ItNetworkResponse; stale: string | null }) {
  return (
    <>
      {stale !== null && (
        <div className="rounded-md border border-(--danger)/40 bg-(--danger)/10 px-2.5 py-1.5 text-[11px] text-(--danger)">
          Showing the last reading — the box stopped answering ({stale}).
        </div>
      )}

      <section className="flex flex-col gap-2">
        <SectionLabel label="Internet uplink" hint="Ethernet, 5G, or failover" />
        <UplinkSection mode={data.config.mode} status={data.status} />
      </section>

      <section className="flex flex-col gap-2">
        <SectionLabel
          label="Wi-Fi access point"
          hint="Let clients reach the cockpit over the air"
        />
        <ApForm ap={data.config.ap} />
      </section>

      {/* Vendor-only. The server returns `config.wwan: null` to anyone without
          `can_manage_secrets`, so the absence of the block IS the gate — the
          client never has to be told which role it would take. */}
      {data.config.wwan && (
        <section className="flex flex-col gap-2">
          <SectionLabel
            label="5G bearer"
            hint="APN, SIM and standby policy — managed by the vendor"
          />
          <WwanForm wwan={data.config.wwan} />
        </section>
      )}

      <section className="flex flex-col gap-2">
        <SectionLabel label="Failover probe" hint="Seeded at provisioning — read-only" />
        <ProbeCard probe={data.config.probe} />
      </section>
    </>
  )
}

/** Three-way mode selector plus the live status card. */
function UplinkSection({
  mode,
  status,
}: {
  mode: ItNetworkConfig["mode"]
  status: ItNetworkStatus
}) {
  const qc = useQueryClient()
  const save = useMutation({
    mutationFn: (next: ItNetworkConfig["mode"]) => setItNetworkMode({ mode: next }),
    onSuccess: (result) => {
      qc.setQueryData<ItNetworkResponse>(IT_NETWORK_KEY, (previous) =>
        withMode(previous, result.mode),
      )
      void qc.invalidateQueries({ queryKey: IT_NETWORK_KEY })
    },
  })

  return (
    <div className="flex flex-col gap-2.5 rounded-xl border border-border bg-card px-3.5 py-3">
      {!status.modem_present && (
        <p className="text-[11px] text-muted-foreground">
          This box has no 5G module, so ethernet is its only uplink.
        </p>
      )}
      <div className="flex flex-col gap-1.5">
        {modeRows(mode, status.modem_present).map((row) => (
          <button
            key={row.id}
            type="button"
            disabled={save.isPending || row.unavailable !== null}
            onClick={() => save.mutate(row.id)}
            className={cn(
              "flex items-center gap-2.5 rounded-md border px-2.5 py-2 text-left transition-colors disabled:opacity-50",
              row.selected
                ? "border-(--interactive) bg-(--interactive)/10"
                : "border-border hover:bg-muted/60",
            )}
          >
            <span
              className={cn(
                "size-2 shrink-0 rounded-full",
                row.selected ? "bg-(--interactive)" : "bg-muted-foreground/30",
              )}
            />
            <span className="flex flex-col">
              <span className="text-[12px] font-medium text-foreground/90">{row.label}</span>
              <span className="text-[11px] text-muted-foreground/70">
                {row.unavailable ?? row.blurb}
              </span>
            </span>
          </button>
        ))}
      </div>

      {save.isSuccess && (
        <span className="text-[11px] text-(--ok)">{savedLabel(save.data.applied)}</span>
      )}
      {save.isError && (
        <span className="text-[11px] text-red-500">
          {apiErrorMessage(save.error, "Could not change the uplink mode")}
        </span>
      )}

      <StatusCard status={status} />
      {status.supervisor !== null && <SupervisorCard supervisor={status.supervisor} />}

      <p className="text-[11px] text-muted-foreground">
        Changing the uplink never touches this box's LAN address, so the cockpit stays reachable
        whatever you pick.
      </p>
    </div>
  )
}

/** Live read-back: what the box is actually routing through right now. */
function StatusCard({ status }: { status: ItNetworkStatus }) {
  return (
    <div className="flex flex-col gap-1 rounded-md border border-border bg-muted/40 p-2.5">
      <Row label="Active uplink" value={activeUplinkLabel(status.active_uplink)} />
      <Row label="Ethernet" value={wanLine(status.wan)} />
      {status.wwan && <Row label="5G" value={wwanLine(status.wwan)} />}
      {status.ap && <Row label="Access point" value={apLine(status.ap)} />}
    </div>
  )
}

/**
 * What the failover supervisor itself believes (C1) — the block it wrote every
 * 10 s while nothing read it.
 *
 * The state that has to be unmistakable is `promoted` without `achieved`: the
 * box decided it needed 5G and could not get it, so it has no uplink at all
 * rather than the 5G one the decision implies. That, and the two startup
 * refusals, are the only things here `/proc/net/route` cannot say.
 */
function SupervisorCard({ supervisor }: { supervisor: ItNetworkSupervisor }) {
  const view = supervisorView(supervisor)
  return (
    <div
      className={cn(
        "flex flex-col gap-1 rounded-md border p-2.5",
        view.tone === "alert"
          ? "border-(--danger)/50 bg-(--danger)/10"
          : view.tone === "warn"
            ? "border-(--warn)/50 bg-(--warn)/10"
            : "border-border bg-muted/40",
      )}
    >
      <span
        className={cn(
          "text-[11.5px] font-semibold",
          view.tone === "alert" ? "text-(--danger)" : "text-foreground/90",
        )}
      >
        Failover supervisor — {view.headline}
      </span>
      {view.detail !== null && view.detail !== "" && (
        <span className="text-[11px] text-muted-foreground">{view.detail}</span>
      )}
      {view.rows.map((row) => (
        <Row key={row.label} label={row.label} value={row.value} />
      ))}
    </div>
  )
}

/** The probe tuning the supervisor runs with. Read-only by contract: `probe` is
 *  `readOnly` in the schema, seeded by Ansible, and no endpoint accepts it. */
function ProbeCard({ probe }: { probe: ItNetworkProbe }) {
  return (
    <div className="flex flex-col gap-1 rounded-xl border border-border bg-card px-3.5 py-3">
      {probeRows(probe).map((row: StatusRow) => (
        <Row key={row.label} label={row.label} value={row.value} />
      ))}
      <p className="mt-1 text-[11px] text-muted-foreground">
        Set when the box was provisioned. Changing it means re-running the installer.
      </p>
    </div>
  )
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-baseline justify-between gap-3">
      <span className="text-[11px] text-muted-foreground/70">{label}</span>
      <span className="text-right font-mono text-[11px] text-foreground/90">{value}</span>
    </div>
  )
}
