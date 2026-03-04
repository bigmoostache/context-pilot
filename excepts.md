# `#[expect]` Audit — Final Status

**110 → 9** annotations remaining. **101 slain, 1210 XP earned.**

---

## Remaining `#[expect]` Annotations (9 total)

| # | File | Line | Lint | Reason | Killable? |
|---|------|------|------|--------|-----------|
| 1 | `cast.rs` | 1 | `allow_attributes` | Macro-generated `#[allow]` can't use `#[expect]` — lint triggers depend on which type the macro expands for | ❌ Language limitation |
| 2 | `config/mod.rs` | 392 | `panic` | `yaml_invariant_panic()` — THE ONE centralized panic for all YAML/config invariant violations. Validated by tests in `lib.rs`. | ❌ Intentional design |
| 3 | `panels.rs` | 42 | `wildcard_enum_match_arm` | `scroll_key_action()` — `KeyCode` is an external enum from `crossterm`. Exhaustive matching would break on upstream updates. | ❌ External enum |
| 4 | `runtime.rs` | 19 | `struct_excessive_bools` | `State` has 12+ independent boolean flags. Decomposition possible but touches ~1000 callsites. | 🟡 Possible (B5 — State decomposition) |
| 5 | `runtime.rs` | 274 | `expect_used` | `ext()` — centralized panic for module TypeMap access. Callers don't need per-site `#[expect]`. | ❌ Intentional design |
| 6 | `runtime.rs` | 284 | `expect_used` | `ext_mut()` — same as above, mutable variant. | ❌ Intentional design |
| 7 | `server/main.rs` | 2 | `unused_crate_dependencies` | Binary target shares `Cargo.toml` with library — lib deps aren't used in the bin. | ❌ Cargo limitation |
| 8 | `server/main.rs` | 359 | `unsafe_code` | `libc::setsid()` — POSIX requirement, async-signal-safe, no preconditions. Server owns its session lifecycle. | ❌ OS requirement |
| 9 | `typst_cli.rs` | 5 | `print_stdout`, `print_stderr`, `exit` | CLI subcommands — printing and `process::exit` are the expected interface for `typst compile`/`typst recompile-watched`. Module-level annotation covers 2 functions. | ❌ CLI convention |

---

## Summary by Justification

| Category | Count | Details |
|----------|-------|---------|
| **Intentional design** | 3 | `yaml_invariant_panic` (centralized), `ext()`/`ext_mut()` (centralized TypeMap panics) |
| **Language/tooling limitation** | 3 | `allow_attributes` (macro), `unused_crate_dependencies` (Cargo bin/lib), `wildcard_enum_match_arm` (external enum) |
| **OS/platform requirement** | 1 | `unsafe_code` (libc::setsid) |
| **CLI convention** | 1 | `print_stdout`/`print_stderr`/`exit` (typst CLI) |
| **Possible future kill** | 1 | `struct_excessive_bools` (State decomposition — ~1000 callsite refactor) |

---

## Infrastructure Improvements

- **YAML validation tests** (3 tests in `cp-base/lib.rs`): All 6 config YAMLs + all 20 tool YAMLs validated at `cargo test` time. Schema drift caught before production.
- **`yaml_invariant_panic()`**: Single centralized panic function for all YAML/config invariants. Was 3 scattered `#[expect]` → 1.
- **`signal-hook` crate**: Replaced hand-rolled `libc::signal()` with safe, community-maintained signal handling.
- **Server self-daemonization**: `setsid()` moved from TUI `pre_exec` hook to server `main()`. Server owns its lifecycle.
- **`ToolTexts::parse()`**: Delegates to `parse_yaml()` — no more per-module serde_yaml dependency.

---

## ☠️ BOUNTY BOARD — Final Score

```
Started:  110 bosses
Slain:    101
Survived:   9 (all justified)
XP:       1210 / 1210
```
