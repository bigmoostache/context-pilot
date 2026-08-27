# Design — Fixed Tiered Memory Slots

> Status: **SHIPPED** (T650, cp-mod-memory reworked, built + reloaded). Functional
> requirements only — **no NFR** (performance, persistence-format details,
> multi-worker semantics out of scope for this pass).
>
> **Locked decisions (T650 form + follow-ups):** panel render = summary +
> occupied only; tidy nudge = total occupancy ≥ 90 %; **one tool `memory_edit`**
> (batch, addressed by slot id `M-<tier>-<n>`; no create/update/delete/move —
> editing a slot to empty frees it); `M-safe` shown in full; `importance` orders
> display within a tier only; existing memories **migrated, not wiped** (FR7).

## 1. Intent

Today memories are an **unbounded list** (`M1`, `M2`, …) with a one-liner
`tl_dr`, a rich `contents` body, `importance`, and freeform `labels`. Nothing
stops the list from growing without limit, and nothing pressures the agent to
keep each entry dense.

This rework replaces the open list with a **fixed, bounded budget of tiered
slots**. The budget is **set in stone**; empty slots stay visible so the agent
always feels the ceiling; and the tool copy + tool results actively push the
agent to write dense, synthetic memories and to tidy up as the budget fills.

The `cp-mod-memory` crate is **adapted**, not replaced.

## 2. The slot tiers

Five namespaces, each a fixed count of individually-bounded slots:

| Tier       | Slots | Bound          | Purpose                                                  |
|------------|-------|----------------|---------------------------------------------------------|
| `M-safe`   | 50    | **≤ 200 chars**| env vars, keys, passwords, values — a **safe key/value repository** |
| `M-tiny`   | 100   | ≤ 60 tokens    | one-sentence facts                                       |
| `M-short`  | 40    | ≤ 120 tokens   | **the preferred tier** — most items need ≤ 100 tokens   |
| `M-mid`    | 20    | ≤ 200 tokens   | lengthier information, when unavoidable                  |
| `M-long`   | 10    | ≤ 400 tokens   | incompressible material the LLM cannot shrink — last resort |

**Total = 220 slots.** `M-safe` is bounded in **characters** (it holds literal
values — keys, tokens — where a char cap is the honest limit); the other four
are bounded in **tokens** via the existing `estimate_tokens`.

Slot ids are tier-scoped and stable: `M-safe-1 … M-safe-50`, `M-tiny-1 …
M-tiny-100`, `M-short-1 … M-short-40`, `M-mid-1 … M-mid-20`, `M-long-1 …
M-long-10`.

## 3. Functional requirements (the mandate)

- **FR1 — Fixed tiered budget.** 220 slots across the five tiers above; the
  budget is immutable at runtime.
- **FR2 — Per-slot bound, hard-enforced.** A write whose `contents` exceeds its
  tier's bound is **rejected** (server-side), with a message that names the
  bound.
- **FR3 — New field set.** Each slot holds `{ title, contents, importance }`.
  - **ADD `title`** — max **25 chars**, the always-visible label (replaces the
    role `tl_dr` played).
  - **KEEP `contents`** — the tier-bounded value.
  - **KEEP `importance`** — Low / Medium / High / Critical.
  - **DROP `tl_dr`** and **DROP `labels`**.
- **FR4 — Empty slots are visible.** Unused slots render in the panel as
  `**empty**` so the agent always sees remaining capacity (§6).
- **FR5 — Dense-memory tool copy.** The `memory_*` tool descriptions **strongly**
  push compact, synthetic, dense entries, state that the budget is fixed, and
  steer toward `M-short` (§7).
- **FR6 — Tidy-up nudge when filling.** When occupancy crosses **~90 %**, the
  tool **result** appends a nudge to *faire de l'ordre* — drop useless items,
  factor duplicates, compress several into one `M-short` (§8).
- **FR7 — One-shot lossy migration** (owner reversed the wipe, T650). On first
  load of the new binary, existing memories are **folded into the fixed slot
  budget** rather than dropped: sort by importance descending, then fill tiers in
  `FILL_ORDER` **long → mid → short → tiny → safe** (most important land in the
  roomiest slots); new `contents` = old `tl_dr` + `contents` **truncated** to the
  destination tier's bound (accepted information loss); `title` = the first words
  of the old `tl_dr` (≤ 25 chars); `labels` dropped. The file is then rewritten
  in the new slot-keyed format so the legacy detection never fires again. (The 87
  pre-existing memories landed as long 10 / mid 20 / short 40 / tiny 17, safe 0.)

## 4. Data model

```
MemorySlot {
  id:         String,          // "M-short-23" — tier + index, fixed
  tier:       Tier,            // Safe | Tiny | Short | Mid | Long
  title:      String,          // ≤ 25 chars (FR3)
  contents:   String,          // ≤ tier bound (FR2)
  importance: MemoryImportance,// Low | Medium | High | Critical (kept)
  // occupied == false → renders as **empty**; title/contents empty
  occupied:   bool,
}
```

- `MemoryState` becomes a **fixed set of 220 slots** (occupied or empty) rather
  than a growable `Vec`. The old `next_memory_id` counter is gone — ids are
  positional, not allocated.
- `yaml_key` (today `SHA-256(tl_dr)`) is **replaced by the slot id itself** as
  the stable YAML key — the slot id is already unique and stable, so the
  backing store keys on it directly. `memories.yaml` stores only occupied slots.
- `is_global` stays **true** (shared `memories.yaml`), unchanged.

**Tier bounds — advertise-vs-enforce (reused).** Today `tl_dr` advertises 80
tokens but enforces 120, because the model overshoots a stated cap. Each tier
keeps this split: advertise a number slightly under the true hard cap so a
marginal overrun still lands, and never surface the real cap.

| Tier      | advertised | enforced (hard) |
|-----------|-----------|-----------------|
| `M-safe`  | 180 chars | 200 chars       |
| `M-tiny`  | 50 tokens | 60 tokens       |
| `M-short` | 100 tokens| 120 tokens      |
| `M-mid`   | 170 tokens| 200 tokens      |
| `M-long`  | 360 tokens| 400 tokens      |

## 5. Tool — a single `memory_edit`

**One tool** (the two-tool create/update split was collapsed at the owner's
request): a batch of slot edits, each addressed by an explicit slot id. There is
**no create, update, delete, or move** — only edit. Editing a slot to empty
`contents` (and empty `title`) frees it (renders `**empty**` again).

```
memory_edit({
  edits: [
    { id: "M-short-23",         // explicit slot id, must exist in the fixed set
      title?: string,           // ≤ 25 chars
      contents?: string,        // ≤ tier bound (chars for safe, tokens else)
      importance?: "low"|"medium"|"high"|"critical" }
  ]
})
```

- Addresses a slot **by id**. A partial edit (e.g. `importance` only) preserves
  the other fields. Bounds are re-checked on any `contents` change; the whole
  entry is rejected (per id) if title/contents overflow, with the other edits in
  the batch still applied.
- **Freeing a slot** = editing it so both `title` and `contents` are empty → it
  returns to `**empty**`. No dedicated `delete`/`clear` flag.
- Validation names the tier's **advertised** bound; the write is enforced against
  the (slightly higher) hard cap — the advertise-vs-enforce split below.
- **No `move` between tiers** — a re-tier is free-old-slot + write-new-slot.

## 6. Panel rendering (FR4)

The panel shows the **fixed budget**, grouped by tier, so the ceiling is always
in view. Two candidate renderings — **decision needed (§9)**:

**(a) Summary + occupied only (recommended).** Per tier: a `used/total`
header, the occupied slots in full, and a single line for the free ones:
```
M-safe   (3/50):
  M-safe-1   [critical] ANTHROPIC_API_KEY → sk-…             
  M-safe-2   [high]     orchestrator port → 7878
  M-safe-3   [medium]   meili port → 49595
  … 47 empty slots free
M-short  (2/40):
  M-short-1  [high]  deploy checklist → build→copy→restart→verify
  M-short-2  [medium] gilbert ssh → tailscale SSH, no key/pw
  … 38 empty slots free
```
Cost: ~one line per occupied slot + one "N free" line per tier — cheap.

**(b) Every slot literal.** All 220 rows, empty ones as `**empty**`. Honors
"empty slots show up" most literally, but costs ~1.7 k tokens **permanently**
even with an empty store. Flagged as the expensive option.

Both keep empties **visible**; they differ only in whether each empty is its own
row. `importance` drives ordering within a tier (critical first).

## 7. Tool copy — dense-memory incentives (FR5)

The `memory_create` / `memory_update` descriptions (in `yamls/tools/memory.yaml`)
are rewritten to:
- state plainly that the **budget is fixed at 220 slots** and cannot grow;
- push **compact, synthetic, dense** phrasing — every token must earn its slot;
- steer to **`M-short` first** ("most memories need ≤ 100 tokens"); reserve
  `M-mid`/`M-long` for genuinely incompressible material;
- describe `M-safe` as the **safe key/value repository** for secrets/values;
- require a **≤ 25-char title** that is a real label, not a sentence.

## 8. Tidy-up nudge when filling (FR6)

- **Trigger:** after any `memory_create`/`memory_update`, compute occupancy.
  When **total occupancy ≥ 90 %** (≥ 198 / 220), or the **written tier is full**,
  append a nudge to the tool **result** (not a blocking error).
- **Message:** short, directive — *"Memory is nearly full (198/220). Tidy up:
  delete stale slots, factor duplicates, and compress several small entries into
  one `M-short`."* Tier-full adds: *"`M-mid` is full — free a slot or use a
  smaller tier."*
- **Non-blocking** unless the specific write had nowhere to go (tier full on
  create → that create is rejected, but the *nudge* is guidance, not a wall
  elsewhere).

## 9. Open questions (to iterate)

1. **Panel rendering** — summary + occupied (a, recommended, ~cheap) vs every
   slot literal (b, honors "show empty" most, ~1.7 k tokens permanent). Which?
2. **`>90 %` scope** — total occupancy, per-tier, or both (recommended: total
   for the tidy nudge + tier-full for the hard reject).
3. **Re-tier path** — delete + create only (recommended), or a dedicated
   `memory_move(id, tier)`?
4. **`importance` role now** — display ordering only, or also a
   compaction-priority hint in the nudge ("compress your Low items first")?
5. **`M-safe` secrecy** — should `M-safe` contents be masked in the panel
   (show `sk-…`) since it holds secrets, or shown in full? Masking changes the
   render.
6. **Wipe timing (FR7)** — wipe on first boot of the new binary, or drop the old
   `memories.yaml` outright at deploy?

## 10. Surface touched (informational, for later planning)

- `cp-mod-memory/types.rs`: `Tier` enum, `MemorySlot`, fixed-slot `MemoryState`;
  drop `tl_dr`/`labels`/`next_memory_id`, add `title`/`tier`/`occupied`.
- `cp-mod-memory/tools.rs`: tier-aware create (lowest-free-slot), id-addressed
  update/delete, per-tier bound validation (chars for safe, tokens otherwise),
  tidy-up nudge in the result.
- `cp-mod-memory/panel.rs`: fixed-budget render (per-tier used/total + empties),
  `overview_context_section` → per-tier occupancy.
- `cp-mod-memory/storage.rs`: key `memories.yaml` on slot id; store occupied
  slots only.
- `cp-mod-memory/lib.rs`: tool defs + rewritten `yamls/tools/memory.yaml` copy;
  per-tier advertised/enforced constants.
- Frontend/orchestrator (M141, later): memory projection gains
  `tier`/`title`/`slot`, drops `tl_dr`/`labels`; web memory surface renders the
  tiered budget.
