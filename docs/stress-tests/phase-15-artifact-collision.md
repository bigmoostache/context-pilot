# P15 — Artifact file-write collisions & disk

**Todo:** X572 · **Primary hazard:** `now_ms()` filename clobber under concurrency

## Objective
`screenshot`/`extract` workers write artifact files named by `now_ms()`. Two ops in
the same millisecond collide and one silently overwrites the other. Stress
concurrent writes, disk-full, unwritable dirs, path traversal, and huge PNGs.

## Targeted hazard
`tools.rs` builds `artifact_path` from a millisecond timestamp under
`.context-pilot/browser/`. With ops now on **concurrent workers**, two screenshots
(or screenshot+extract) finishing in the same ms produce the **same filename** →
last writer wins, the other artifact is lost, and the LLM gets a path to the wrong
file. No pid/counter/uuid disambiguator.

## Subtasks

### [M] Medium
- **X858** Two screenshots in the same ms; `artifact_path` filename collision.
- **X859** screenshot + extract concurrent writes; both files intact.
- **X860** Verify `now_ms()` granularity vs collision likelihood.
- **X861** Large extract → file path returned; readable.
- **X862** Artifact dir auto-created under `.context-pilot/browser/`.

### [H] Hard
- **X863** 10 screenshots in one turn; count distinct files written.
- **X864** Unwritable artifact dir (`chmod 000`); clean error.
- **X865** Disk-full during a screenshot write; error not crash.
- **X866** Collision: prove the last writer overwrites silently.
- **X867** Concurrent extract+screenshot same ms; cross-clobber.

### [V] Very hard
- **X868** Propose fix: add pid/counter/uuid to the artifact filename.
- **X869** Path traversal via cwd or selector into the filename?
- **X870** Huge full_page PNG (50MB) write + timeout interplay.
- **X871** Artifact accumulation; no cleanup → disk growth.
- **X872** Symlink in the artifact dir; does the write follow it? (security).

### [X] Extreme
- **X873** 1000 artifacts; FS inode / dir-size limits.
- **X874** Race: same-ms collisions under burst, quantify the loss rate.
- **X875** Disk-full mid-burst; partial files + recovery.
- **X876** Artifact write + reload mid-write; truncated file.
- **X877** Prove the filename-uniqueness fix eliminates all collisions.

## Findings
| ID | Severity | Repro | Status | Fix / Issue |
|----|----------|-------|--------|-------------|
| H15-1 | **S2 (narrowed)** | `tools.rs::artifact_path` was `format!("capture_{}.{ext}", now_ms())` — millisecond timestamp, no pid/counter/uuid. Two writes in the same ms → silent `fs::write` clobber; the LLM gets a path whose bytes belong to the other op. **Likelihood reduced** vs the original "concurrent" claim: ALL ops (incl. the in-op `fs::write`) are serialized by `op_lock`, so two artifact writes can't truly overlap — but two fast serialized ops can still both land in the same millisecond. | **CONFIRMED then FIXED+VERIFIED** — 2026-06-11. | **FIXED** (`tools.rs::artifact_path`): filename is now `capture_{now_ms}_{seq}.{ext}` where `seq` is a process-global `AtomicU64` (`ARTIFACT_SEQ.fetch_add(1, Relaxed)`). The monotonic counter guarantees a unique filename for every artifact regardless of timestamp granularity → same-millisecond clobber eliminated by construction. rust-check green. |

## Exit criterion
No two concurrent artifact writes can collide (unique filenames proven over a
1000-op burst); disk-full / unwritable-dir errors are clean, never a crash.
