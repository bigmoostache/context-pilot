import { useState } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { Download, Loader2 } from "lucide-react"
import {
  downloadItCaCert,
  fetchItCaFingerprint,
  fetchItIdentity,
  fetchItProvisioned,
  setItIdentity,
} from "@/lib/api"
import { cn } from "@/lib/utils"

/**
 * IT settings pane (design §13.5) — mobile twin of `components/shell/config/
 * ItPane`. Gated on `can_manage_it` (admin+; the caller only renders it for that
 * role and the backend enforces 403 otherwise, NFR-05). The maintenance-plane
 * IT functions re-homed onto `:443`:
 *
 *  - **Network identity** — the box's DNS name + LAN IP. Saving re-issues the
 *    private-CA leaf and reloads Caddy (`POST /api/it/identity`).
 *  - **TLS trust** — download the private-CA root for client distribution
 *    (`GET /api/it/ca.crt`) and its SHA-256 fingerprint for out-of-band
 *    verification (`GET /api/it/ca/fingerprint`).
 *
 * Divergence from desktop is touch-only: the text inputs carry a **16px font**
 * (iOS Safari auto-zooms the viewport on focus below 16px), and the action
 * buttons grow / swap `hover:` for `active:`. All mutation logic — identity
 * re-issue, CA download, fingerprint poll — is byte-identical to the desktop
 * twin (it lives in the shared `@/lib/api` layer, not forked).
 */
export function ItPane() {
  return (
    <div className="flex flex-col gap-6">
      <ProvisionStatus />

      <section className="flex flex-col gap-2">
        <SectionLabel label="Network identity" hint="Box DNS name & LAN IP" />
        <IdentitySection />
      </section>

      <section className="flex flex-col gap-2">
        <SectionLabel label="TLS trust" hint="Distribute the private-CA root to clients" />
        <TrustSection />
      </section>
    </div>
  )
}

/** A compact banner reflecting whether the box has been provisioned
 *  (`GET /api/it/provisioned`). Once `:443` is live the box is normally
 *  provisioned; the indicator confirms it and surfaces a not-yet state. */
function ProvisionStatus() {
  const { data } = useQuery({ queryKey: ["it-provisioned"], queryFn: fetchItProvisioned })
  if (data === undefined) return null
  const done = data.provisioned
  return (
    <div className="flex items-center gap-2.5 rounded-xl border border-border bg-card px-3.5 py-2.5">
      <span
        className={cn(
          "size-2 shrink-0 rounded-full",
          done ? "bg-(--ok)" : "bg-muted-foreground/40",
        )}
      />
      <span className="text-[12px] font-medium text-foreground/90">
        {done ? "Box provisioned" : "Box not yet provisioned"}
      </span>
    </div>
  )
}

function SectionLabel({ label, hint }: { label: string; hint: string }) {
  return (
    <div className="flex items-baseline gap-2">
      <span className="text-[10.5px] font-semibold tracking-[0.07em] text-muted-foreground/80 uppercase">
        {label}
      </span>
      <span className="text-[11px] text-muted-foreground/60">{hint}</span>
    </div>
  )
}

/** Name/IP form. Prefilled from `GET /api/it/identity`; saving posts to
 *  `POST /api/it/identity`, which re-issues the leaf and reloads Caddy. */
function IdentitySection() {
  const { data, isLoading } = useQuery({ queryKey: ["it-identity"], queryFn: fetchItIdentity })

  if (isLoading || data === undefined) {
    return (
      <div className="rounded-xl border border-border bg-card px-3.5 py-3">
        <div className="flex items-center gap-2 py-1 text-[12px] text-muted-foreground">
          <Loader2 className="size-3.5 animate-spin" /> Loading…
        </div>
      </div>
    )
  }

  const initial = data.identity
  // Remount when the persisted identity changes so the fields re-seed from the
  // server value — avoids copying props into state via an effect.
  return (
    <IdentityForm
      key={`${initial?.name ?? ""}|${initial?.ip ?? ""}`}
      initialName={initial?.name ?? ""}
      initialIp={initial?.ip ?? ""}
    />
  )
}

function IdentityForm({ initialName, initialIp }: { initialName: string; initialIp: string }) {
  const qc = useQueryClient()
  const [name, setName] = useState(initialName)
  const [ip, setIp] = useState(initialIp)

  const save = useMutation({
    mutationFn: () => setItIdentity(name.trim(), ip.trim()),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["it-identity"] }),
  })

  // Clear any success/error banner as soon as the operator edits a field.
  const edit = (setter: (v: string) => void) => (v: string) => {
    save.reset()
    setter(v)
  }

  return (
    <div className="rounded-xl border border-border bg-card px-3.5 py-3">
      <form
        className="flex flex-col gap-2.5"
        onSubmit={(e) => {
          e.preventDefault()
          if (ip.trim() !== "" && !save.isPending) save.mutate()
        }}
      >
        <TextField
          label="DNS name"
          hint="optional"
          value={name}
          onChange={edit(setName)}
          placeholder="pilot.acme.corp"
        />
        <TextField
          label="LAN IP address"
          value={ip}
          onChange={edit(setIp)}
          placeholder="192.168.1.116"
        />
        <p className="text-[11px] text-muted-foreground">
          Saving re-issues the TLS certificate for this name/IP. Use a static lease so the address
          doesn't change.
        </p>
        <div className="flex items-center gap-2">
          <button
            type="submit"
            disabled={ip.trim() === "" || save.isPending}
            className="flex items-center gap-1.5 rounded-md bg-(--interactive) px-3.5 py-2 text-[13px] font-medium text-(--primary-foreground) transition-[filter] active:brightness-105 disabled:opacity-50"
          >
            {save.isPending && <Loader2 className="size-3.5 animate-spin" />}
            Save &amp; re-issue certificate
          </button>
          {save.isSuccess && (
            <span className="text-[11px] text-(--ok)">Saved — certificate re-issued</span>
          )}
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

/** CA-root download + fingerprint. Fingerprint from `GET /api/it/ca/fingerprint`;
 *  download via the authenticated binary blob (`GET /api/it/ca.crt`). */
function TrustSection() {
  // Caddy mints the private-CA root lazily on the first `:443` handshake, so the
  // fingerprint 404s for a beat after the box is provisioned. Poll every 2s until
  // it lands, then stop (data present ⇒ no further refetch).
  const { data: fp } = useQuery({
    queryKey: ["it-ca-fingerprint"],
    queryFn: fetchItCaFingerprint,
    retry: false,
    refetchInterval: (query) => (query.state.data ? false : 2000),
  })
  const download = useMutation({ mutationFn: downloadItCaCert })

  return (
    <div className="flex flex-col gap-2.5 rounded-xl border border-border bg-card px-3.5 py-3">
      <p className="text-[12px] text-muted-foreground">
        Download the certificate-authority root and install it as a trusted root on every client
        (push it via Group Policy or your MDM). Verify the fingerprint below out-of-band before
        trusting it.
      </p>

      <div className="rounded-md border border-border bg-muted/40 p-2.5">
        <div className="mb-1 text-[10.5px] font-semibold tracking-[0.06em] text-muted-foreground/80 uppercase">
          SHA-256 fingerprint
        </div>
        <div className="font-mono text-[11px] break-all text-foreground/90">
          {fp?.fingerprint ?? "waiting for the CA root…"}
        </div>
      </div>

      <div className="flex items-center gap-2">
        <button
          onClick={() => download.mutate()}
          disabled={download.isPending}
          className="flex items-center gap-1.5 rounded-md border border-border px-3 py-2 text-[13px] font-medium text-foreground/80 transition-colors active:bg-muted/60 disabled:opacity-50"
        >
          {download.isPending ? (
            <Loader2 className="size-3.5 animate-spin" />
          ) : (
            <Download className="size-3.5" />
          )}
          Download CA root (root.crt)
        </button>
        {download.isError && (
          <span className="text-[11px] text-red-500">
            {download.error instanceof Error ? download.error.message : "Download failed"}
          </span>
        )}
      </div>
    </div>
  )
}

/** A labelled single-line text input, matching the pane's card styling. The
 *  input font is 16px so iOS Safari doesn't auto-zoom the viewport on focus. */
function TextField({
  label,
  hint,
  value,
  onChange,
  placeholder,
}: {
  label: string
  hint?: string
  value: string
  onChange: (v: string) => void
  placeholder?: string
}) {
  return (
    <label className="flex flex-col gap-1">
      <span className="flex items-baseline gap-2">
        <span className="text-[12px] font-medium text-foreground/90">{label}</span>
        {hint && <span className="text-[11px] text-muted-foreground/60">{hint}</span>}
      </span>
      <input
        type="text"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        className={cn(
          "w-full rounded-md border border-border bg-muted/50 px-2.5 py-2 font-mono text-[16px] text-foreground",
          "placeholder:text-muted-foreground/50 focus:ring-1 focus:ring-(--interactive) focus:outline-none",
        )}
      />
    </label>
  )
}
