import { useState } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { Loader2 } from "lucide-react"
import type {
  ItNetworkAp,
  ItNetworkConfig,
  ItNetworkStatus,
  ItNetworkWwan,
} from "@/lib/api/generated/types.gen"
import { fetchItNetwork, setItNetworkAp, setItNetworkMode } from "@/lib/api"
import { cn } from "@/lib/utils"
import { SectionLabel, TextField, Toggle } from "./ItPane"
import { WwanForm } from "./ItWwanForm"

/** Uplink modes, in the order an admin escalates through them. `needsModem`
 *  marks the two that a box without the 5G module cannot honour — picking `5g`
 *  there would suppress the ethernet default route with nothing to replace it,
 *  so they are not rendered at all on that variant. */
const MODES: { id: ItNetworkConfig["mode"]; label: string; blurb: string; needsModem: boolean }[] =
  [
    { id: "wan", label: "Ethernet", blurb: "Cable only — the modem stays off", needsModem: false },
    {
      id: "wan_5g",
      label: "Ethernet + 5G",
      blurb: "5G takes over when the cable stops working",
      needsModem: true,
    },
    {
      id: "5g",
      label: "5G only",
      blurb: "The cable's default route is suppressed",
      needsModem: true,
    },
  ]

/** How often the status card re-reads the box while the pane is open. Fast
 *  enough that a failover is visible without a manual refresh (O4.1). */
const POLL_MS = 5000

/**
 * Internet uplink + Wi-Fi access point (docs/design-network-uplink.md §11) —
 * mobile twin of `components/shell/config/ItNetworkPane`.
 *
 * Mounted inside {@link ItPane}, so it inherits the same `can_manage_it` gate:
 * the caller only renders the IT pane for admin+, and the backend answers 403
 * to anyone else regardless (NFR-NET-03 — client gating is cosmetic).
 *
 * Both forms write through the API layer, whose secret handling is what the UI
 * has to respect: the passphrase is **write-only**, so the server never sends it
 * back and the field starts empty on every load. Leaving it empty keeps the
 * stored key; typing replaces it (FR-NET-13).
 *
 * Divergence from desktop is touch-only: the `<select>` and the buttons carry a
 * **16px font** (iOS Safari auto-zooms the viewport on focus below 16px) and a
 * taller hit area, the band/channel/country row stacks instead of sitting in
 * three columns (it does not fit a 390px viewport), and `hover:` becomes
 * `active:`. Every mutation — mode change, AP save, the write-only passphrase
 * handling — is byte-identical to the desktop twin; it lives in the shared
 * `@/lib/api` layer, not forked.
 */
export function ItNetworkPane() {
  const { data, isLoading } = useQuery({
    queryKey: ["it-network"],
    queryFn: fetchItNetwork,
    refetchInterval: POLL_MS,
  })

  if (isLoading || data === undefined) {
    return (
      <section className="flex flex-col gap-2">
        <SectionLabel label="Internet uplink" hint="Ethernet, 5G, or failover" />
        <div className="rounded-xl border border-border bg-card px-3.5 py-3">
          <div className="flex items-center gap-2 py-1 text-[12px] text-muted-foreground">
            <Loader2 className="size-3.5 animate-spin" /> Loading…
          </div>
        </div>
      </section>
    )
  }

  return (
    <>
      <section className="flex flex-col gap-2">
        <SectionLabel label="Internet uplink" hint="Ethernet, 5G, or failover" />
        <UplinkSection mode={data.config.mode} status={data.status} />
      </section>

      <section className="flex flex-col gap-2">
        <SectionLabel
          label="Wi-Fi access point"
          hint="Let clients reach the cockpit over the air"
        />
        {/* Remount on any persisted change so the fields re-seed from the server
            value rather than being copied into state by an effect. */}
        <ApForm key={apKey(data.config.ap)} initial={data.config.ap} />
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
          <WwanForm key={wwanKey(data.config.wwan)} initial={data.config.wwan} />
        </section>
      )}
    </>
  )
}

/** Identity of the persisted AP settings, for the remount key. */
function apKey(ap: ItNetworkAp): string {
  return [ap.enabled, ap.ssid, ap.band, ap.channel, ap.country, ap.hidden, ap.share_internet].join(
    "|",
  )
}

/** Identity of the persisted bearer settings, for the remount key. */
function wwanKey(wwan: ItNetworkWwan): string {
  return [
    wwan.apn,
    wwan.username,
    wwan.password_set,
    wwan.pin_set,
    wwan.roaming,
    wwan.standby,
  ].join("|")
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
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["it-network"] }),
  })

  return (
    <div className="flex flex-col gap-2.5 rounded-xl border border-border bg-card px-3.5 py-3">
      {!status.modem_present && (
        <p className="text-[11px] text-muted-foreground">
          This box has no 5G module, so ethernet is its only uplink.
        </p>
      )}
      <div className="flex flex-col gap-1.5">
        {MODES.filter((option) => !option.needsModem || status.modem_present).map((option) => (
          <button
            key={option.id}
            type="button"
            disabled={save.isPending}
            onClick={() => save.mutate(option.id)}
            className={cn(
              "flex items-center gap-2.5 rounded-md border px-3 py-2.5 text-left transition-colors disabled:opacity-50",
              option.id === mode
                ? "border-(--interactive) bg-(--interactive)/10"
                : "border-border active:bg-muted/60",
            )}
          >
            <span
              className={cn(
                "size-2 shrink-0 rounded-full",
                option.id === mode ? "bg-(--interactive)" : "bg-muted-foreground/30",
              )}
            />
            <span className="flex flex-col">
              <span className="text-[12px] font-medium text-foreground/90">{option.label}</span>
              <span className="text-[11px] text-muted-foreground/70">{option.blurb}</span>
            </span>
          </button>
        ))}
      </div>

      {save.isError && (
        <span className="text-[11px] text-red-500">
          {save.error instanceof Error ? save.error.message : "Could not change the uplink mode"}
        </span>
      )}

      <StatusCard status={status} />

      <p className="text-[11px] text-muted-foreground">
        Changing the uplink never touches this box's LAN address, so the cockpit stays reachable
        whatever you pick.
      </p>
    </div>
  )
}

/** Live read-back: what the box is actually routing through right now. */
function StatusCard({ status }: { status: ItNetworkStatus }) {
  const active =
    status.active_uplink === "wan"
      ? "Ethernet"
      : status.active_uplink === "wwan"
        ? "5G"
        : "No internet"
  const wan = status.wan
  const wwan = status.wwan
  return (
    <div className="flex flex-col gap-1 rounded-md border border-border bg-muted/40 p-2.5">
      <Row label="Active uplink" value={active} />
      {wan && (
        <Row
          label="Ethernet"
          value={`${wan.carrier ? "cable in" : "no cable"}${wan.ip ? ` · ${wan.ip}` : ""}${
            wan.has_default_route ? " · routing" : ""
          }`}
        />
      )}
      {wwan && (
        <Row
          label="5G"
          value={[
            wwan.state ?? "unknown",
            wwan.operator,
            wwan.tech,
            wwan.signal_dbm === null ? null : `${wwan.signal_dbm} dBm`,
            wwan.ip,
          ]
            .filter(Boolean)
            .join(" · ")}
        />
      )}
      {status.ap && (
        <Row
          label="Access point"
          value={
            status.ap.running
              ? `on · ${status.ap.ssid ?? "?"}${status.ap.channel === null ? "" : ` · ch ${status.ap.channel}`} · ${status.ap.clients} client${status.ap.clients === 1 ? "" : "s"}`
              : "off"
          }
        />
      )}
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

/** Access-point form. The passphrase field is write-only — see the pane doc. */
function ApForm({ initial }: { initial: ItNetworkAp }) {
  const qc = useQueryClient()
  const [enabled, setEnabled] = useState(initial.enabled)
  const [ssid, setSsid] = useState(initial.ssid)
  const [passphrase, setPassphrase] = useState("")
  const [band, setBand] = useState<ItNetworkAp["band"]>(initial.band)
  const [channel, setChannel] = useState(String(initial.channel))
  const [country, setCountry] = useState(initial.country)
  const [hidden, setHidden] = useState(initial.hidden)
  const [share, setShare] = useState(initial.share_internet)

  const save = useMutation({
    mutationFn: () =>
      setItNetworkAp({
        enabled,
        ssid: ssid.trim(),
        // Omitted entirely when untouched: absent means "keep the stored key",
        // which is the only way a UI that never receives the key can save the
        // rest of the form without wiping it.
        ...(passphrase !== "" && { passphrase }),
        band,
        channel: Number(channel) || 0,
        country: country.trim().toUpperCase(),
        hidden,
        share_internet: share,
      }),
    onSuccess: () => {
      setPassphrase("")
      void qc.invalidateQueries({ queryKey: ["it-network"] })
    },
  })

  // A country code is a functional prerequisite, not a preference: without one
  // every 5 GHz channel is `no IR` and the radio cannot beacon at all. The
  // server refuses with a 400; the client mirrors it so the button explains
  // itself before the round-trip (FR-NET-14).
  const missingCountry = country.trim().length !== 2
  const missingKey = !initial.passphrase_set && passphrase.length < 8
  const blocked = enabled && (missingCountry || missingKey)

  return (
    <div className="rounded-xl border border-border bg-card px-3.5 py-3">
      <form
        className="flex flex-col gap-2.5"
        onSubmit={(event) => {
          event.preventDefault()
          if (!blocked && !save.isPending) save.mutate()
        }}
      >
        <Toggle label="Broadcast the access point" checked={enabled} onChange={setEnabled} />
        <TextField
          label="Network name (SSID)"
          value={ssid}
          onChange={setSsid}
          placeholder="ContextPilot"
        />
        <TextField
          label="Passphrase"
          hint={initial.passphrase_set ? "set — leave empty to keep it" : "8–63 characters"}
          value={passphrase}
          onChange={setPassphrase}
          placeholder={initial.passphrase_set ? "••••••••" : "at least 8 characters"}
        />
        <div className="flex flex-col gap-2.5">
          <label className="flex flex-col gap-1">
            <span className="text-[12px] font-medium text-foreground/90">Band</span>
            <select
              value={band}
              onChange={(event) => setBand(event.target.value === "bg" ? "bg" : "a")}
              className="w-full rounded-md border border-border bg-muted/50 px-2.5 py-2 text-[16px] text-foreground focus:ring-1 focus:ring-(--interactive) focus:outline-none"
            >
              <option value="a">5 GHz</option>
              <option value="bg">2.4 GHz</option>
            </select>
          </label>
          <div>
            <TextField
              label="Channel"
              hint="0 = auto"
              value={channel}
              onChange={setChannel}
              placeholder="0"
            />
          </div>
          <div>
            <TextField
              label="Country"
              hint="required"
              value={country}
              onChange={setCountry}
              placeholder="FR"
            />
          </div>
        </div>
        <Toggle label="Hide the network name" checked={hidden} onChange={setHidden} />
        <Toggle
          label="Share this box's internet with Wi-Fi clients"
          checked={share}
          onChange={setShare}
        />
        <p className="text-[11px] text-muted-foreground">
          {share
            ? "Wi-Fi clients get an address, DNS and internet through this box — it becomes a router on your site."
            : "Wi-Fi clients get an address and can open the cockpit, but no traffic is forwarded to the internet."}
        </p>

        <div className="flex items-center gap-2">
          <button
            type="submit"
            disabled={blocked || save.isPending}
            className="flex items-center gap-1.5 rounded-md bg-(--interactive) px-3.5 py-2.5 text-[13px] font-medium text-(--primary-foreground) transition-all active:brightness-105 disabled:opacity-50"
          >
            {save.isPending && <Loader2 className="size-3.5 animate-spin" />}
            Save access point
          </button>
          {blocked && (
            <span className="text-[11px] text-muted-foreground">
              {missingCountry
                ? "A two-letter country code is required before the radio can start."
                : "Set a passphrase of at least 8 characters."}
            </span>
          )}
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
