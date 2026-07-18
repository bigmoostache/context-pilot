# Search Portability: Boot Reconciliation + Incarnation-Agnostic Embedding Backup

> Two features on the search module (`cp-mod-search`): **(A)** a boot-time filesystem⇄index reconciliation sweep, and **(C)** a portable per-agent embedding backup carried inside the agent's own folder. They share the indexer's `index_one_file` / `delete_one_file` path and the same `(mtime,size)`fingerprint.
>
> The index uid stays derived from the **absolute path** (unchanged). That is a feature, not a bug — two incarnations of a project on one machine (`/a`, `/b`) hash to different uids and stay isolated. The backup is **incarnation-agnostic**: it carries no uid, only relative doc ids + vectors, so it restores into whatever uid the destination computes locally at boot.

---

## 1. Problem

The index is only ever mutated by **live file-watcher events**, and the uid is `cp_{hash(abspath)}_files` / `_logs`, computed at boot from `current_dir()`.

### 1.1 Offline drift

On restart the module loads persisted state, so `is_reload == true` → `skip_initial_scan == true` → **no full scan, and no reconciliation anywhere**. Anything changed while the agent was down is missed silently:

| While agent is down | Current outcome |
| --- | --- |
| File deleted | Orphan chunks + vectors linger |
| File added | Never indexed until touched live |
| File edited | Stale chunks kept until touched live |

### 1.2 Cross-machine re-embed

Voyage vectors live in the **global** Meilisearch dir (`~/.context-pilot/meilisearch/data/`), *outside* the folder. Copy the folder to another machine → destination computes its own uid → empty index → **full Voyage re-embed of everything**. Constraint: *the only thing ever copied is the agent's own folder.*

---

## 2. Key facts this leans on

Verified in `indexer.rs::index_one_file`:

```rust
let rel_path = abs_path.strip_prefix(&ctx.project_root).unwrap_or(abs_path);
let rel_str  = rel_path.to_string_lossy();
"file_path": rel_str,                    // stored field — RELATIVE
let safe_id  = format!("{rel_str}-{i}")  // document id — RELATIVE (then sanitized)
```

- Both `file_path` and the document `id` are **project-relative** → identical across machines with the same tree. This is what makes a doc-id-keyed backup portable with zero rewriting.
- `last_modified_ms` is **already** stored per chunk and declared `sortable`.
- The uid is the **only** absolute-path dependency — and the backup never stores it, so it doesn't matter.

---

## 3. Feature A — boot + periodic reconciliation

Runs **once at boot** *and* **hourly** thereafter (see §3.4). The hourly sweep
catches drift the live watcher misses — dropped/coalesced FS events, an editor
that writes without firing a `Modify`, a delete that raced startup.

### 3.1 Fingerprint

```
fingerprint = (last_modified_ms, size_bytes)
```

Both from one `fs::metadata()` stat — no read, no hash. `mtime + size` (not mtime alone) catches timestamp-preserving copies. `last_modified_ms` is free (already stored); **add** `size_bytes` (one int doc field, no re-embed).

### 3.2 New MeiliClient method

```rust
/// Paged projection of every doc in an index (no content, no vectors).
pub fn fetch_projection(&self, uid: &str, fields: &[&str])
    -> Result<Vec<serde_json::Value>, String>
```

`POST /indexes/{uid}/documents/fetch` with `{ fields, limit:1000, offset }`, paging on `offset` until `offset >= total` (read the response `total`, don't stop on an empty page).

### 3.3 Reconcile (in `load_module_data` + hourly, unconditional)

```
1. index_state = fetch_projection(files_uid, ["file_path","last_modified_ms","size_bytes"])
                 deduped by file_path (chunks of a file share the fingerprint).
2. disk_state  = walk the tree, fs::metadata() each survivor.
3. diff:
     in index, not on disk        → DeleteFile   // offline delete
     on disk, not in index         → IndexFile    // offline add
     fingerprint differs           → IndexFile    // offline edit (reindex whole file)
     equal                         → skip         // zero Voyage
4. Route through the existing IndexerCmd channel.
```

Retires `skip_initial_scan`: on an empty index the reconcile degenerates to "index everything", subsuming the cold-boot scan. One code path.

**Must-fix — filter parity.** The disk-walk MUST apply the *exact* same gates as `index_one_file` (excluded dirs, extension allowlist, 1 MiB cap, symlink skip), via one shared `is_indexable(path, meta) -> bool`. If the walk is looser, it re-queues files the indexer silently rejects → they stay "missing" forever → infinite re-queue churn.

### 3.4 Periodic tick (hourly)

One hourly timer does **reconcile, then export** — in that order, paired:

```
every 1h:
  1. reconcile()          // §3.3 — repair index vs current disk
  2. export_backup()      // §4.1 — overwrite the in-folder backup
```

Pairing (not two independent timers) guarantees the backup always reflects
**post-reconcile truth** — never a half-drifted snapshot. Both route through /
read the single-threaded indexer path, so no parallel-writer race. The timer is
best-effort: a missed or slow tick just means the next hour's backup is slightly
staler, which reimport-then-reconcile heals anyway (§4.3).

---

## 4. Feature C — incarnation-agnostic embedding backup

### 4.1 Export (hourly, atomic overwrite)

For each of the agent's two indexes:

```
POST /indexes/{uid}/documents/fetch  { retrieveVectors: true, limit, offset }
→ write { id, file_path, …, _vectors.<embedder>.embeddings } rows to
  ./.context-pilot/embeddings/{files|logs}.jsonl(.zst)
→ manifest.json { fingerprint, embedder_name, count_files, count_logs }
```

Keyed by **document id**, no uid, no content hash. \~22 MB for \~5500 chunks, compressible.

`fingerprint` = `hash(model + embedder_name + template_string)`, read at runtime from live embedder settings (`get_embedder_settings`) — not a hand-maintained version int. Any change to model / URL / name / template auto-invalidates the backup.

**Cadence.** Written on the hourly tick (§3.4), *after* reconcile, plus once on
clean shutdown. Each write **overwrites** the previous backup — a single rolling
snapshot, no history. Atomic: write `*.jsonl.zst.tmp` + `manifest.json.tmp`,
`fsync`, then rename into place (manifest **last**), so a crash mid-write never
leaves a torn backup — the old one survives intact until the new one is fully
committed.

### 4.2 Reimport (boot, when the index is empty)

**When the backup is used — and only then.** The backup is a *cold-start warmer*,
never a live-index mutation. It is consulted at **boot only**, and applied **only**
when *all* hold:

- the locally-computed index has **0 documents** (fresh machine after a folder
  copy, or a wiped/rebuilt index), **and**
- a backup file is present, **and**
- `manifest.fingerprint == current fingerprint` (same model + embedder +
  template).

If the index already has documents (an ordinary restart), the backup is **not
touched** — boot reconciliation (§3.3) handles any drift instead. Fingerprint
mismatch or a missing/corrupt/count-mismatched backup → **ignored**, full normal
indexing. So the backup can only ever *speed up an empty index*; it can never
corrupt a populated one.

```
If index has 0 docs AND backup exists AND manifest.fingerprint == current fingerprint:
   read jsonl → add_documents in ~500-doc batches WITH
     _vectors.<embedder_name> = { embeddings: <stored>, regenerate: false }
   → Meilisearch stores docs + vectors, ZERO Voyage calls.
Else: fall through to normal indexing.
```

The uid is computed locally from the current absolute path; the backup restores into it regardless of where the folder now lives (§2).

### 4.3 Boot order (load-bearing)

```
ensure_indexes → reimport-on-empty → reconcile → start live watcher
```

- `ensure_indexes` first: the embedder + `_vectors` key must exist before any document is added.
- Reimport warms the index (zero Voyage).
- Reconcile then diffs against *current* disk: files changed since the backup get re-embedded (Voyage only for the delta); deleted files get dropped. A stale backup self-heals.
- Watcher starts last, so its events queue behind a quiesced reconcile.

### 4.4 Safety

- Manifest fingerprint mismatch → ignore backup, full embed. Never mis-serve stale-model vectors.
- Manifest written **last** (both jsonl temp-file + rename first) and count-validated on reimport — a half-written set is rejected. The backup is an optimization, never a source of truth.
- `regenerate:false` is safe by construction: when a reimported file later changes, `index_one_file` does delete-by-filter + re-add *without* `_vectors`, so the stale vector is dropped and Meilisearch re-embeds fresh.
- Voyage key still required on the destination — for **search** (the `rest`embedder still embeds the query). Portability = no doc re-embed, not "Voyage-free".

---

## 5. Data-model & code changes

| Where | Change | Cost |
| --- | --- | --- |
| chunk document | add `size_bytes` (int) | trivial; no re-embed |
| `./.context-pilot/embeddings/` | new backup dir (jsonl.zst + manifest) | \~22 MB |
| MeiliClient | `fetch_projection` + `retrieveVectors` fetch | \~40–60 lines |
| indexer / boot | `is_indexable`, reconcile, export, reimport-on-empty | \~150 lines |

---

## 6. Phasing

**Phase 1 — reconciliation (Feature A), standalone value**

- [ ] `size_bytes` on chunk docs.

- [ ] `MeiliClient::fetch_projection` (paged by `total`).

- [ ] Shared `is_indexable(path, meta)`, called from reconcile + `index_one_file`.

- [ ] `reconcile()` in `load_module_data`, unconditional; retire `skip_initial_scan`.
- [ ] Hourly timer calling `reconcile()` (§3.4).

- [ ] Verify: offline add/delete/edit repaired; unchanged files = zero Voyage.

**Phase 2 — embedding backup (Feature C)**

- [ ] Export `documents/fetch?retrieveVectors=true` → `.context-pilot/embeddings/`jsonl(.zst) + manifest (fingerprint written last, count-validated).

- [ ] Reimport-on-empty with `_vectors {regenerate:false}`, \~500-doc batches.

- [ ] Boot order `ensure_indexes → reimport → reconcile → watcher`.
- [ ] Fold `export_backup()` into the hourly tick (reconcile → export) + clean-shutdown export; atomic overwrite.

- [ ] Verify: cross-machine folder copy rebuilds both indexes with **zero Voyage calls**; a post-backup edit re-embeds only that file.

---

## 7. Open questions

1. **Logs symmetry.** Apply export/reimport to the logs index too — same mechanism, simpler template. Recommend yes.