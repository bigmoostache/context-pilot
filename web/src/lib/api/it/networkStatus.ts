// ── Internet-uplink status formatting ───────────────────────────────
//
// The read half of the uplink pane: turning `GET /api/it/network`'s `status`
// and its read-only `probe` block into the strings the status cards render.
//
// Split from `./network.ts` (the write half — modes, form drafts, the
// validation mirror) purely by size; both exist for the same reason, which is
// that the pane exists TWICE and the two twins differ only in Tailwind classes
// (review finding C8). Nothing here may emit JSX, and — like its sibling — it
// must not import the `@/lib/api` barrel, which re-exports `./index.ts` from
// this directory.

import type {
  ItNetworkApStatus,
  ItNetworkProbe,
  ItNetworkStatus,
  ItNetworkSupervisor,
  ItNetworkWanStatus,
  ItNetworkWwanStatus,
} from "../generated/types.gen"

/**
 * How long ago, in the shape the cockpit uses everywhere else ("just now",
 * "5m ago", "3h ago", "2d ago").
 *
 * Deliberately NOT `formatAge` from `@/lib/api`: that helper is defined in the
 * barrel module itself, and this file lives in a directory the barrel
 * re-exports, so importing it would close a cycle. Takes unix **seconds** —
 * what the supervisor writes — rather than `formatAge`'s epoch milliseconds.
 */
function agoLabel(unixSeconds: number): string {
  const minutes = Math.floor((Date.now() / 1000 - unixSeconds) / 60)
  if (minutes < 1) return "just now"
  if (minutes < 60) return `${minutes}m ago`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours}h ago`
  return `${Math.floor(hours / 24)}d ago`
}

// ── Status formatting ────────────────────────────────────────────────

/** Join the non-empty facts of a status line the way the status card reads. */
function facts(parts: (string | null | false)[]): string {
  return parts.filter((part): part is string => typeof part === "string" && part !== "").join(" · ")
}

const ACTIVE_UPLINK_LABEL: Record<ItNetworkStatus["active_uplink"], string> = {
  wan: "Ethernet",
  wwan: "5G",
  // Never null and never a lie: an unrecognised device (a VPN, a container
  // bridge) used to be reported as `wwan`, which read as a failover that had
  // not happened.
  other: "Another device (VPN or bridge)",
  none: "No internet",
}

/** The device carrying the lowest-metric default route, in words. */
export function activeUplinkLabel(active: ItNetworkStatus["active_uplink"]): string {
  return ACTIVE_UPLINK_LABEL[active]
}

/** The ethernet status line, gateway included (C5). */
export function wanLine(wan: ItNetworkWanStatus): string {
  return facts([
    wan.carrier ? "cable in" : "no cable",
    wan.ip,
    wan.gateway === null ? null : `via ${wan.gateway}`,
    wan.has_default_route && "routing",
  ])
}

/** The bearer status line, registration included (C5). */
export function wwanLine(wwan: ItNetworkWwanStatus): string {
  return facts([
    wwan.state ?? "unknown",
    wwan.registered ? "registered" : "not registered",
    wwan.operator,
    wwan.tech,
    wwan.signal_dbm === null ? null : `${wwan.signal_dbm} dBm`,
    wwan.ip,
  ])
}

/** The access-point status line, regulatory country included (C5). */
export function apLine(ap: ItNetworkApStatus): string {
  if (!ap.running) return "off"
  return facts([
    "on",
    ap.ssid,
    ap.channel === null ? null : `ch ${ap.channel}`,
    ap.country,
    `${ap.clients} client${ap.clients === 1 ? "" : "s"}`,
  ])
}

/** A label/value row of a status or summary card. */
export interface StatusRow {
  label: string
  value: string
}

/**
 * The failover probe tuning, as a read-only summary (C5).
 *
 * `probe` is `readOnly` in the schema — seeded by Ansible at provisioning time,
 * accepted by no endpoint — so this is deliberately a summary and not a form.
 * The cooldown note is the one non-obvious interaction: a cooldown shorter than
 * a probe round can never be inside the round that follows it, so it stops
 * meaning anything, and `interval_s` is permitted up to 3600.
 */
export function probeRows(probe: ItNetworkProbe): StatusRow[] {
  const cooldownNote = probe.cooldown_s <= probe.interval_s ? " (inert — shorter than a round)" : ""
  return [
    { label: "Targets", value: probe.targets.join(", ") },
    {
      label: "Round",
      value: `every ${probe.interval_s}s · ${probe.probe_timeout_s}s per-target timeout`,
    },
    {
      label: "Thresholds",
      value: `${probe.fail_threshold} failed → 5G · ${probe.ok_threshold} ok → ethernet`,
    },
    {
      label: "Limits",
      value: `${probe.cooldown_s}s cooldown${cooldownNote} · ${probe.nm_wait_s}s nmcli cap`,
    },
  ]
}

// ── The failover supervisor (C1) ─────────────────────────────────────

/** How loudly the supervisor card should read. The component picks the colours;
 *  the decision of WHICH state is alarming is logic and lives here. */
export type SupervisorTone = "alert" | "warn" | "ok"

/** The supervisor card's content — headline, one explanatory line, and the raw
 *  fields underneath it. */
export interface SupervisorView {
  tone: SupervisorTone
  headline: string
  detail: string | null
  rows: StatusRow[]
}

/** The prefix `cp-uplink-watch` puts on both of its startup refusals. */
const REFUSAL_PREFIX = "not supervising:"

/**
 * Read the supervisor's own state — the block that was written every 10 s and
 * parsed by nobody until C1.
 *
 * The states worth being able to tell apart, in the order they are tested:
 *
 *  1. **A startup refusal.** `ping` is not installed, or there are no probe
 *     targets. The daemon is running and reporting, but it will never fail
 *     over: every probe would "fail" identically to a dead upstream. Alarming,
 *     and invisible from anywhere else.
 *  2. **Decided but not achieved** — `promoted` without `achieved`. The box
 *     wanted 5G and could not get it (no coverage, no SIM, modem still
 *     enumerating), so it has NO uplink rather than the 5G one the decision
 *     implies. This is the outage signature.
 *  3. **Failed over.** 5G is carrying, which means the ethernet path is down.
 *  4. **Observing only.** Mode `wan` or `5g`: the routing is static and the
 *     applier owns it, so the daemon reports and arbitrates nothing.
 *  5. Watching, nothing to report.
 */
export function supervisorView(supervisor: ItNetworkSupervisor): SupervisorView {
  const rows = supervisorRows(supervisor)
  const reason = supervisor.last_reason
  if (reason?.startsWith(REFUSAL_PREFIX) === true) {
    return {
      tone: "alert",
      headline: "Failover is NOT running",
      detail: `${reason.slice(REFUSAL_PREFIX.length).trim()} — the box will never fail over to 5G until this is fixed.`,
      rows,
    }
  }
  if (supervisor.promoted && !supervisor.achieved) {
    return {
      tone: "alert",
      headline: "5G was requested and did not come up",
      detail: facts([
        "The supervisor gave up on the cable and the bearer did not take over, so this box has no working uplink",
        reason,
      ]),
      rows,
    }
  }
  if (supervisor.promoted) {
    return {
      tone: "warn",
      headline: "Failed over to 5G",
      detail: facts(["The ethernet path stopped answering", reason]),
      rows,
    }
  }
  if (!supervisor.supervising) {
    return {
      tone: "ok",
      headline: "Observing only",
      detail:
        "The uplink mode is static, so the applier owns the routing and the supervisor only reports.",
      rows,
    }
  }
  return { tone: "ok", headline: "Watching the ethernet path", detail: reason, rows }
}

/**
 * The supervisor's raw fields, in the order they answer "and why?".
 *
 * The card deliberately shows three things that are routinely confused for one:
 *
 *  - **`promoted`** — what the supervisor *decided*. "5G should carry" or
 *    "ethernet should carry". A decision, nothing more.
 *  - **`achieved`** — whether that decision was actually *actuated*, re-read
 *    from `nmcli` rather than inferred from the exit status of the command that
 *    tried. A promotion that could not land (modem still enumerating, no
 *    coverage, the `--wait` cap) is `promoted` without `achieved`, and it is
 *    retried on later turns instead of being written off as done.
 *  - **`active_uplink`** — what the *kernel* is routing through right now, which
 *    is the only one of the three an outside observer can check. It can
 *    disagree with the other two both ways: a decision that has not landed
 *    yet, or something outside the supervisor (a `nmcli` run, a reboot)
 *    resetting the metric under it.
 *
 * Collapsing them was the original bug: a supervisor that set `promoted` before
 * actuating and never checked believed it had failed over while the box had no
 * uplink at all. Hence "Decision" (decided · in force / NOT in force) and
 * "Kernel route" as two separate rows.
 */
function supervisorRows(supervisor: ItNetworkSupervisor): StatusRow[] {
  return [
    {
      label: "Decision",
      value: facts([
        supervisor.promoted ? "5G should carry" : "ethernet should carry",
        supervisor.achieved ? "in force" : "NOT in force",
      ]),
    },
    { label: "Kernel route", value: activeUplinkLabel(supervisor.active_uplink) },
    {
      label: "Streaks",
      value: `${supervisor.fail_streak} failed · ${supervisor.ok_streak} ok`,
    },
    {
      label: "Last change",
      value:
        supervisor.last_transition === null
          ? "none since it started"
          : agoLabel(supervisor.last_transition),
    },
    { label: "Reason", value: supervisor.last_reason ?? "—" },
  ]
}
