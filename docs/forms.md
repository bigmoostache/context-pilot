# Inline `form` blocks — design spec

> Status: **design complete** — ready to implement. Replaces the `questions` parameter on the `Send` tool (removed, see §8).

> **Implementation scope — HARD CONSTRAINT.** This feature must **not** touch any existing API. Forms are pure message-parsing logic riding the **unchanged**message API as plain markdown. The implementation touches **only**:
>
> 1. **The** `Send` **tool** — its param **descriptions** (text), plus removing the now-dead `questions` param (§8), plus an **optional** guard that validates an agent-authored ```` ```form ```` block's YAML at send time and **refuses the send** (returns an error) if it is malformed. No other Rust.
> 2. **The frontend** — rendering, draft, submit, answered-state derivation, and deleting the old `questions` render path.
>
> No new endpoint, no wire/schema change, no backend form state. See §7.

## 1. Motivation

Today an agent asks the user structured questions through the `questions`parameter of `Send` — a channel **separate** from the message text, rendered at one fixed spot. That is rigid:

- the form cannot be placed at an exact point in the narrative;
- only one question set per message;
- it is a bespoke parameter instead of reusing the mechanism we already have for presenting content inline.

We already render **inline fenced blocks** for files: the agent writes a ```` ```file-upload ```` block with YAML inside, and the frontend replaces it, at the exact spot in the prose, with a clickable file card. Several blocks = several cards, interleaved with text.

`form` blocks apply the same idea to questions:

- placed **exactly** where the agent wants in the message body;
- **multiple** forms per message;
- one unified inline mechanism, no side-channel parameter.

Crucially, a `form` block is **just markdown** inside an ordinary message. The message API does not change; only the frontend learns to render the block and the agent learns (via the tool description) to author it.

## 2. Authoring — the ```` ```form ```` block

The agent writes a fenced block with `form` as the language tag and a YAML body. The frontend parses every such block and replaces it with an interactive form widget.

```
```form
form-id: deploy-v0-3-0
title: Déploiement release v0.3.0
submit: Confirmer
fields:
  - id: target
    label: Sur quelle machine déployer ?
    type: single
    allow-other: true
    options:
      - label: gilbert
        detail: Mac Mini arm64, prod tailnet
      - label: asterix
        detail: serveur x86_64, staging
      - label: local
        detail: cette machine, test
  - id: agents
    label: Quels agents redémarrer ?
    type: multi
    options:
      - label: opio-1
        detail: agent principal
      - label: general-config
        detail: agent de configuration
  - id: note
    label: Note de déploiement
    type: text
  - id: count
    label: Nombre de tentatives max
    type: number
  - id: when
    label: Date de déploiement
    type: date
  - id: dry-run
    label: Mode dry-run
    type: toggle
  - id: artifacts
    label: Joindre les logs de build
    type: files
  - id: go
    label: Confirmer le déploiement en production
    type: confirm
    confirm-word: DEPLOY
```
```

There is **no** `status` **field**. Whether a form is answered is **derived**, not stored — see §5. The agent authors the form and never has to track its state.

### Global fields (top-level)

| Field | Required | Meaning |
| --- | --- | --- |
| `form-id` | yes | Unique identifier, **agent-generated**, **thread-unique** (unique within the thread). The pivot: it re-matches the answer back to this form and drives the derived answered-state (§5). Recommend a short slug or UUID. |
| `title` | no | Header shown above the form. |
| `submit` | no | Submit-button label (default e.g. "Envoyer"). |
| `fields` | yes | Ordered list of field entries. |

### Field entry

| Key | Meaning |
| --- | --- |
| `id` | Field identifier, unique within the form. Keys the answer. |
| `label` | Question / prompt text shown to the user. |
| `type` | One of the v1 types below. |
| `options` | Only for `single` / `multi`. A list of real YAML entries (see §3). |
| `allow-other` | Only for `single`, optional. Adds a free-text "Other…" choice (see §3). |
| `confirm-word` | Only for `confirm`, optional. If set, user must type this word to arm the danger button. |

**All fields are mandatory.** There is no `required` flag — every field must be answered before the form can be submitted. Because the backend has no form awareness (§7), this is enforced **client-side**: the frontend disables submit until every field is filled.

## 3. Field types (v1)

| Type | Widget | Answer shape |
| --- | --- | --- |
| `single` | radio group (+ optional "other") | one option `label`, or the typed free-text string |
| `multi` | checkbox group | list of option `label`s |
| `text` | text input / textarea | string |
| `number` | numeric input | number |
| `date` | date picker | ISO date string (`YYYY-MM-DD`) |
| `toggle` | on/off switch | boolean |
| `confirm` | danger button (+ cancel), optional type-to-arm | boolean |
| `files` | file upload (immediate) | list of relative paths |

### Options — real YAML entries, not a one-line list

For `single` and `multi`, `options` is **not** an inline `[a, b, c]` list. Each option is a proper YAML entry with:

- `label` (string) — the choice text / the value that comes back;
- `detail` (string) — a secondary description shown under the label.

```yaml
options:
  - label: gilbert
    detail: Mac Mini arm64, prod tailnet
  - label: asterix
    detail: serveur x86_64, staging
```

### `single` + `allow-other` — free-text escape

With `allow-other: true`, a `single` field renders an extra **"Other…"** radio that reveals a text input. If the user picks it, the answer is the **typed string** instead of an option `label`. Lets the user answer off-menu without a separate `text` field. (v1: `single` only; `multi` + other is a v2 idea.)

### `date` — calendar

`date` renders a date picker. Its answer is an ISO date string, `YYYY-MM-DD`.

### `toggle` vs `confirm` — two different booleans

Both answer a boolean, but they are **not** the same widget:

- `toggle` — a neutral on/off switch. No accent, no guard. For plain yes/no options (`dry-run`, `notify me`, …).
- `confirm` — a **danger-accented** button (+ cancel), optionally guarded by `confirm-word` (type `DEPLOY` / `DELETE` to arm). The first-class path for "confirmations avant action destructive".

### `files` — answer by uploading

`files` lets the user answer by uploading one or more files. Its answer is a **list of relative paths**.

**Upload is immediate, not deferred to submit.** The moment the user picks files, they are uploaded through the **existing upload path** (the same one the composer already uses, into `.uploads/`) — no new endpoint. The resulting **relative paths** are gathered and cached in the `localStorage` draft. On submit, those paths — not the bytes — go into the `form-answer` YAML.

This is the one field whose side effect (bytes on the server) happens **before**submit; every other field only commits its value at submit time. If the user never submits, the uploaded files simply remain as orphaned uploads (same as any composer upload that is never sent).

## 4. Rendering rules

- The frontend replaces each ```` ```form ```` block, at its exact position in the message, with the interactive widget (like `file-upload` cards).
- Multiple `form` blocks per message are allowed; each is independent, keyed by its own `form-id`.
- A form renders **locked** (widgets disabled, showing the submitted values) once a matching `form-answer` message exists later in the thread — the answered-state is **derived** by the frontend, never a stored field (§5).

## 5. Answer lifecycle

The core rule: **a form is "answered" iff a** `form-answer` **message with the same** `form-id` **appears later in the thread.** No status field, no message mutation — presence of the answer *is* the state.

1. **Draft (browser-local).** As the user fills the form, answers are stored **temporarily in** `localStorage`, keyed by `form-id`. This survives a page refresh before submit, so a half-filled form is not lost. For `files`, the draft holds the **relative paths** of the already-uploaded files (see §3 — the bytes are uploaded on pick, immediately, not at submit).

2. **Submit.** When the user submits, the **frontend composes a** `form-answer`**message and sends it through the existing send-message path** — the exact same call an ordinary user message uses. There is no special endpoint and no backend form logic; the `form-answer` message is just a normal user message whose body carries a fenced block. The original form message is **never touched**.

   ```
   ```form-answer
   form-id: deploy-v0-3-0
   answers:
     - id: target
       answer: gilbert
     - id: agents
       answer: [opio-1, general-config]
     - id: note
       answer: "rollback plan ready"
     - id: count
       answer: 3
     - id: when
       answer: 2026-07-20
     - id: dry-run
       answer: false
     - id: artifacts
       answer: [.uploads/build-log.txt, .uploads/manifest.json]
     - id: go
       answer: true
   ```
   ```

   `answer` is a scalar for `single` / `text` / `number` / `date` / `toggle` / `confirm`, and a list for `multi` and `files`.

3. **Frontend derivation + parsing.**

   - The frontend scans the thread: a form is locked when a `form-answer` with its `form-id` exists after it. That single lookup drives the state.
   - It parses the `form-answer` values; **these overrule** `localStorage` (the committed answer is now the source of truth for the locked form's display).
   - The automatic user message is **not shown as raw YAML**. It renders only as a short line: **"User answered form** `<form-id>`**"**.
   - **Only** the original ```` ```form ``` message renders the form widget. The ````form-answer\` message does **not** render a form.

4. **Agent receives the answers.** The agent reads the `form-answer` block in the user message body and parses it to learn what was chosen — reliable, keyed by field `id`. For `files`, the agent gets the relative paths and can open them.

## 6. Why the answer lives in its own message (and nothing is mutated)

Deriving answered-state from a separate `form-answer` message — rather than flipping a `status` field on the original — buys three things:

- **No mutation of old messages.** The original form message is immutable; the thread is strictly append-only, as it already is for everything else.
- **No agent bookkeeping.** The agent never authors or maintains a `status`field (no systematic `status: pending`); it just writes the form once.
- **Single source of truth.** The presence of the `form-answer` *is* the state. There is no stored flag that could disagree with reality — impossible to have a form marked `answered` with no answer, or vice-versa.

It also means the answer is a real user message: the thread gets a genuine turn for it (the agent's turn resumes), the agent has a clean structured payload to parse, and — decisively — **no API change is needed**, because sending a user message with a fenced block is something the message API already does.

## 7. Backend involvement (deliberately near-zero — see the scope constraint)

The backend does **not** participate in the form mechanism:

- it does **not** create the `form-answer` message — the **frontend** composes and sends it via the existing send-message path (§5);
- it does **not** derive answered/locked state — the **frontend** scans the thread by `form-id` (§4);
- it stores **no** form status and gains **no** new endpoint, type, or schema.

The `form` and `form-answer` blocks are opaque markdown to the backend; they ride the unchanged message API exactly like a `file-upload` block does.

**The single permitted Rust touch (optional):** a send-time guard that parses an agent-authored ```` ```form ```` block and **refuses the send with an error** if the YAML is malformed (unknown `type`, missing `form-id`, `options` on a non-select field, …). This is a validation-only gate — it adds no state and changes no API shape. It exists so a broken form never lands in a thread.

**Consequence — answer validation is client-side.** With no backend form awareness, "every field mandatory" is enforced only by the frontend gating the submit button. This is a deliberate trade for the zero-API-change constraint, and a conscious deviation from the usual "backend is authoritative" rule (M141): forms are a presentation/parsing layer over free-form messages, introducing no new domain state, so the logic living in the frontend is acceptable here.

## 8. Removal of the `questions` parameter

The old `questions` parameter of `Send` is **removed entirely** — the user has approved editing the tool definition (Rust) for this, not just its description:

- **tool (Rust):** drop the `questions` param from the `Send` tool schema and all its handling in `cp-mod-threads`;
- **frontend:** remove `QuestionForm.tsx` and the `questions` render path;
- `form` blocks are a superset — no functionality is lost.

This is the one structural Rust change beyond descriptions, explicitly sanctioned. It removes surface; it adds none.

## 9. `Send` tool description update

The `Send` tool description must be updated to teach the LLM how to author `form` blocks, with at least one worked example (the §2 block) covering each field type and the `form-id` convention. It must state plainly that the agent authors the form **once**, never tracks its status, and picks a **thread-unique**`form-id`. This mirrors how the tool already documents the `file-upload` block.

## 10. Resolved decisions

- `form-id` **scope** — thread-unique. Agent-generated; recommend a short slug or UUID.
- **Field types** — `single` (+ `allow-other`), `multi`, `text`, `number`, `date`, `toggle`, `confirm`, `files` are **all v1**.
- **Cross-field validation** (min/max selections on `multi`) — not done. v1 is "everything mandatory, ≥1 for multi/files", client-enforced.
- **Re-answer** — impossible. A form is answered **once**; "answered" is terminal (once a `form-answer` exists, the form is locked forever).
- **Derivation site** — the **frontend**. It scans the thread by `form-id`; the backend is not involved (§7).
- `questions` **removal** — removed now, tool def + frontend (§8).

**v2 ideas:**

- `allow-other` on `multi` (v1 is `single` only).
- `single` with a richer "other" (structured, not just free text).
- an explicit **supersede** rule if re-answering is ever wanted.

## 11. Final checklist — verify before calling it done

### Backend (Rust)

- [ ] `questions` param removed from the `Send` tool schema + all handling in `cp-mod-threads`. `grep -ri questions` in that crate returns nothing form-related.

- [ ] `Send` tool description updated: teaches ```` ```form ```` authoring with the §2 worked example, all 8 field types, the **thread-unique** `form-id`convention, and "author once, never track status".

- [ ] (optional) send-time guard validates an agent-authored ```` ```form ```` block and rejects a malformed one with an error.

- [ ] **No new endpoint, wire type, or schema.** OpenAPI spec unchanged; api contract callbacks green. The message API is byte-for-byte the same.

- [ ] `cargo build` + workspace tests green; all Rust callbacks (fmt, clippy, structure, api-contract) green.

### Frontend

- [ ] `QuestionForm.tsx` deleted; the `questions` render path removed; `grep -ri questions web/src` returns nothing render-related.

- [ ] ```` ```form ```` blocks parsed and rendered as an interactive widget at their **exact inline position** (mirrors the `file-upload` card mechanism).

- [ ] All 8 field types render + validate client-side: `single` (+`allow-other`free-text), `multi`, `text`, `number`, `date`, `toggle`, `confirm` (+`confirm-word` type-to-arm), `files`.

- [ ] **All fields mandatory** — submit disabled until every field is filled (`multi`/`files` need ≥1).

- [ ] `localStorage` draft keyed by `form-id`, survives a pre-submit refresh.

- [ ] `files`: immediate upload via the existing `.uploads/` path on pick; relative paths cached in the draft and emitted in the answer.

- [ ] Submit composes a ```` ```form-answer ```` message and sends it via the **existing** send-message path (no new call).

- [ ] Locked/answered state **derived** by scanning the thread for a matching `form-answer` by `form-id`.

- [ ] An answered form renders **locked** (widgets disabled, showing the submitted values), and stays locked after reload.

- [ ] The `form-answer` message renders only as **"User answered form** `<form-id>`**"**, never raw YAML.

- [ ] **Only** the original ```` ```form ```` message renders the widget — the `form-answer` message does not.

- [ ] Multiple `form` blocks per message render independently.

- [ ] `tsc` 0, `eslint` 0, `prettier` 0; the `web-lint` callback is green.

### Integration / end-to-end

- [ ] Agent authors a form once (no status field anywhere).

- [ ] Full loop: agent sends a form → user answers → `form-answer` message appears → form locks → agent parses the answers by field `id`.

- [ ] `files` answer round-trips: agent can open the uploaded relative paths.