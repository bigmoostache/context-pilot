// ── web-vitals producer ──────────────────────────────────────────────
//
// Subscribes to the Core Web Vitals via the library's ATTRIBUTION build (the
// same tiny, Google-maintained lib Chrome's CrUX uses) and normalizes each
// sample into a {@link VitalEvent} for the store. The headline metric for our
// freeze investigation is **INP** (Interaction to Next Paint — the successor to
// FID since March 2024): it measures how long the UI takes to visually respond
// to a click/tap/keypress, and its attribution splits that latency into the
// three phases (input delay → processing → presentation) plus the DOM element
// the user interacted with. That phase split is exactly "where the wall-time
// went" for a laggy interaction.

import {
  onCLS,
  onFCP,
  onINP,
  onLCP,
  onTTFB,
  type CLSMetricWithAttribution,
  type FCPMetricWithAttribution,
  type INPMetricWithAttribution,
  type LCPMetricWithAttribution,
  type TTFBMetricWithAttribution,
} from "web-vitals/attribution"
import { record, type VitalEvent } from "./store"

/** Round to one decimal — vitals are ms (or unitless CLS); noise past 0.1 is
 *  irrelevant for a where-does-time-go readout. */
function round(n: number): number {
  return Math.round(n * 10) / 10
}

function push(name: string, value: number, rating: VitalEvent["rating"], detail?: string): void {
  record({ kind: "vital", name, value: round(value), rating, detail, ts: Date.now() })
}

/** Human one-liner for the INP breakdown: the slowest phase + the target. */
function inpDetail(a: INPMetricWithAttribution["attribution"]): string {
  const phases: [string, number][] = [
    ["input", a.inputDelay],
    ["processing", a.processingDuration],
    ["presentation", a.presentationDelay],
  ]
  const [slowest] = phases.toSorted(([, x], [, y]) => y - x)
  const target = a.interactionTarget || "unknown"
  const worst = slowest ? `${slowest[0]} ${Math.round(slowest[1])}ms` : "—"
  return `${worst} · on ${target}`
}

/**
 * Register all Web Vitals listeners. Called once by {@link initTelemetry}. INP
 * (and CLS) report multiple times as the interaction picture sharpens; the
 * store simply keeps the latest per metric.
 */
export function initWebVitals(): void {
  onINP((m: INPMetricWithAttribution) => {
    push(m.name, m.value, m.rating, inpDetail(m.attribution))
  })
  onLCP((m: LCPMetricWithAttribution) => {
    push(m.name, m.value, m.rating, m.attribution.target ?? undefined)
  })
  onCLS((m: CLSMetricWithAttribution) => {
    push(m.name, m.value, m.rating, m.attribution.largestShiftTarget ?? undefined)
  })
  onFCP((m: FCPMetricWithAttribution) => {
    push(m.name, m.value, m.rating)
  })
  onTTFB((m: TTFBMetricWithAttribution) => {
    push(m.name, m.value, m.rating)
  })
}
