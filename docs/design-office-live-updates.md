# Office live updates without reload

Status: proposal (not built). Scope: the Finder's BetterOffice editors (.pptx, .docx, .xlsx).

## Problem

An agent rewrites an office file on disk → the polled version moves → `OfficeEditor` refetches the bytes and remounts (`key={version}`). The open document visibly reloads, and the user loses scroll, slide/sheet and selection.

BetterOffice engines are Yrs CRDTs. Tested in node (`@betteroffice/pptx` 0.2.0):

- Opening the **same bytes** twice yields the same base state. An update from one copy (`encodeDiff`) merges into the other (`applyUpdate`); concurrent edits on both sides are kept.
- Two **different saved files** do not merge (`invalid yrs update`). Reading the file from disk can therefore only ever produce a reload.

So a change can be applied in place only if it travels as a CRDT update made against the same base the editor opened.

## Constraints

1. The file on disk stays the source of truth. Agents may also rewrite files with other tools (python-pptx, openpyxl, python-docx); those produce no updates, so the reload path stays.
2. The update log is a cache. If it is missing or doesn't match the file version, use the bytes on disk. No state lives only in the orchestrator.
3. One generic path for all three formats. Checked: pptx, docx and xlsx editors all accept `collaboration.{clientId, initialUpdate, onReplica}`, and the replica exposes `encodeStateVector` / `applyUpdate` / `onUpdate`.
4. Restarting the orchestrator drops every agent, so it needs the user's OK at deploy time.

## Design

### Orchestrator: an opaque update log per file

The orchestrator does not understand CRDTs. For each office file it keeps (in memory, plus an optional spill file under the sync dir):

```
OfficeLog { base_version, base_bytes, updates: Vec<(seq, Vec<u8>)>, head_version }
```

- `base_bytes` = the file as it was when the log started. Every replica opens these bytes, so they all share the same base.
- `head_version` = the file version written by the last logged update.
- Valid only while the version on disk equals `head_version`. If it doesn't (an external write), the log is dropped and restarts from the current file.

Routes, next to the existing `fs/office` routes:

| Route | Purpose |
|---|---|
| `GET fs/office/log?path=&since=` | `{base_version, head_version, seq, updates[]}`, or 409 when `since` predates the base |
| `POST fs/office/update?path=&head=` | body = CRDT update + new file bytes (multipart); 409 if `head` ≠ current head. Appends the update and writes the file atomically (cp-office write). Returns the new `head_version`/`seq` |

### Writers

- **Agent**: a small node CLI shipped with the web build (`office-edit`). It fetches the log, opens `base_bytes`, applies the updates, runs the edit, then POSTs `encodeDiff` plus `save()`. It is the only way an agent gets a live update. Plain file rewrites still work but cause a reload.
- **Browser**: the editor sends the replica's local updates (`onUpdate`, origin local), batched about 1 s, through the same route. That replaces today's whole-file Save, so the log also covers the user's edits and an agent's replay sees them.

### Readers

The editor opens `base_bytes` + `initialUpdate` (all updates folded) and remembers `seq`. On every 3 s version poll:

- if the version moved and the log covers it, `GET log?since=seq` and `replica.applyUpdate` each update in place, with no remount;
- otherwise (external write, log missing, 409), reload as today, but keep the active slide/sheet, scroll and selection across the remount.

### Determinism risk

The shared base requires the same engine version in the browser and in `office-edit`. Pin them together; `office-edit` reads the engine from the web build's `node_modules`. If a base doesn't match, `applyUpdate` throws, and the client falls back to a reload.

## Phases

1. pptx end to end: log + routes, `office-edit`, editor wiring. Validate on the GlobaNova deck: an agent edit appears with no remount, and an unsaved user edit survives.
2. docx and xlsx: same wiring. xlsx uses the worker build's async replica.
3. Reload fallback that keeps the view state (slide/sheet, scroll, selection).

## Open questions

- Log size: compact by folding updates into a new base after N updates or on idle.
- Two browser tabs on the same file: they converge through the same log; presence is out of scope.
