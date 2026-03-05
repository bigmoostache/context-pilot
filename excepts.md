crates/cp-base/src/cast.rs:
  1: #![expect(
  2      clippy::allow_attributes,

crates/cp-base/src/config/mod.rs:
  387  /// invariant panics route through here. One `#[expect]` to rule them all.
  388: #[expect(clippy::panic, reason = "invariant violation is unrecoverable — validated by tests")]
  389  pub fn yaml_invariant_panic(msg: &str) -> ! {

crates/cp-console-server/src/main.rs:
  355      #[cfg(unix)]
  356:     #[expect(unsafe_code, reason = "setsid() requires unsafe — async-signal-safe, no preconditions")]
  357      // SAFETY: setsid() is async-signal-safe (POSIX), has no preconditions,


# `#[expect]` Audit — Final Status

**110 → 3** annotations remaining. **107 slain.**

---

## Remaining `#[expect]` Annotations (3 total)

### 1. `cast.rs:1` — `allow_attributes`

```rust
#![expect(
    clippy::allow_attributes,
    reason = "macro-generated #[allow] can't use #[expect] — some lint triggers depend on which type the macro expands for"
)]
```

**Justification:** The `SafeCast` trait uses macros (`impl_safe_cast_unsigned!`, `impl_safe_cast_signed!`) that expand into `#[allow(trivial_numeric_casts, cast_possible_truncation, ...)]` on each impl block. These `#[allow]` annotations can't be `#[expect]` because whether a given lint fires depends on the concrete type the macro expands for — e.g. `u32 as u32` triggers `trivial_numeric_casts` but `u32 as u64` doesn't. An `#[expect]` on the non-triggering expansion would itself become a lint violation ("unfulfilled expectation"). This is a fundamental Rust language limitation with macro-generated lint attributes.

**Strategies to eliminate:**
- **Proc macro with per-type conditionals:** Write a proc macro that introspects the source and target types at compile time and only emits `#[expect]` on the specific cast expressions that actually trigger each lint. Complex, but precisely targets the root cause.
- **`num` crate or `TryFrom`-based casts:** Replace the hand-rolled `SafeCast` macros with `num::cast::NumCast` or blanket `TryFrom` impls with saturating fallbacks. Eliminates macro-generated casts entirely — no raw `as` means no lint to suppress.
- **Wait for Rust RFC:** There are open discussions about allowing `#[expect]` to tolerate unfulfilled expectations in macro expansions. If stabilized, a one-line change removes this annotation.

---

### 2. `config/mod.rs:392` — `panic`

```rust
#[expect(clippy::panic, reason = "invariant violation is unrecoverable — validated by tests")]
pub fn yaml_invariant_panic(msg: &str) -> ! {
    panic!("{msg}")
}
```

**Justification:** This is THE centralized panic for all YAML/config invariant violations across the entire codebase. All config YAMLs are compile-time embedded via `include_str!()` and validated by 3 tests in `cp-base/lib.rs` (6 config YAMLs + 20 tool YAMLs). If this panic fires, it means a developer broke a YAML schema — a bug that should be caught in CI before any binary is produced. The function signature `-> !` (never returns) makes it impossible to accidentally ignore. Having ONE suppression instead of scattered panics across all config call sites is the correct centralization.

**Strategies to eliminate:**
- **`build.rs` compile-time validation:** Move YAML parsing into a `build.rs` script that runs `serde_yaml::from_str()` at compile time. If any YAML is malformed, compilation fails with `compile_error!()`. The runtime `yaml_invariant_panic()` function becomes dead code and can be deleted — the invariant is enforced at build time, not runtime.
- **`const fn` parsing (future Rust):** Once `const fn` supports enough of the serde machinery, YAML validation could happen at const-eval time. Currently blocked by Rust's const-eval limitations, but on the roadmap.
- **Return `Result` everywhere:** Thread `Result<T, ConfigError>` through all config access paths. Each call site handles the error explicitly. Eliminates panics entirely but adds `?` boilerplate to ~50+ call sites that currently rely on LazyLock infallibility.

---

### 3. `server/main.rs:356` — `unsafe_code`

```rust
#[expect(unsafe_code, reason = "setsid() requires unsafe — async-signal-safe, no preconditions")]
// SAFETY: setsid() is async-signal-safe (POSIX), has no preconditions,
// and is called once at startup before any child processes are spawned.
{
    unsafe { let _ = libc::setsid(); }
}
```

**Justification:** `setsid()` creates a new POSIX session so spawned child processes get SIGHUP when the server dies — essential for process cleanup. The `libc` crate exposes it as `unsafe` because it's a raw FFI call, even though `setsid()` has zero preconditions and is async-signal-safe. There's no safe wrapper in the Rust ecosystem because it's too trivial to warrant one.

**Strategies to eliminate:**
- **Irreducible.** `setsid()` is a kernel syscall — the only way to invoke it is through the FFI boundary, which requires `unsafe` in Rust. No crate can eliminate the unsafety; it can only hide it behind a wrapper. The `nix` crate provides `nix::unistd::setsid()` but internally calls the same `unsafe { libc::setsid() }`. Adding 35 transitive dependencies to move the `unsafe` into someone else's crate is not an improvement.
- **Full audit confirms necessity:** 10 process-death scenarios analyzed. `setsid()` is the ONLY mechanism that catches child orphans when the server receives SIGKILL or is OOM-killed — all other cleanup paths (explicit kill, orphan detection, signal handlers) are bypassed. Defense-in-depth for an edge case no other code path covers.

---

## Summary

| # | File | Lint | Killable? | Best Strategy |
|---|------|------|-----------|---------------|
| 1 | `cast.rs` | `allow_attributes` | 🟡 | `num` crate or `TryFrom` blanket impls |
| 2 | `config/mod.rs` | `panic` | 🟢 | `build.rs` compile-time YAML validation |
| 3 | `server/main.rs` | `unsafe_code` | 🔴 | Irreducible — FFI boundary to kernel syscall |
