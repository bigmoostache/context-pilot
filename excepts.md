crates/cp-base/src/cast.rs:
  1: #![expect(
  2      clippy::allow_attributes,

crates/cp-base/src/config/mod.rs:
  387  /// invariant panics route through here. One `#[expect]` to rule them all.
  388: #[expect(clippy::panic, reason = "invariant violation is unrecoverable — validated by tests")]
  389  pub fn yaml_invariant_panic(msg: &str) -> ! {

crates/cp-base/src/state/runtime.rs:
   18  /// Runtime state (messages loaded in memory)
   19: #[expect(clippy::struct_excessive_bools, reason = "Runtime state legitimately needs many boolean flags")]
   20  pub struct State {

  267      /// Prefer this over `get_ext().expect()` — the panic lives here once,
  268:     /// so callers don't need `#[expect(clippy::expect_used)]`.
  269      ///

  273      #[must_use]
  274:     #[expect(clippy::expect_used, reason = "centralized panic — callers use ext() to avoid per-site #[expect]")]
  275      pub fn ext<T: 'static + Send + Sync>(&self) -> &T {

  283      /// Panics if module state `T` was never registered via [`set_ext`](Self::set_ext).
  284:     #[expect(clippy::expect_used, reason = "centralized panic — callers use ext_mut() to avoid per-site #[expect]")]
  285      pub fn ext_mut<T: 'static + Send + Sync>(&mut self) -> &mut T {

crates/cp-console-server/src/main.rs:
  355      #[cfg(unix)]
  356:     #[expect(unsafe_code, reason = "setsid() requires unsafe — async-signal-safe, no preconditions")]
  357      // SAFETY: setsid() is async-signal-safe (POSIX), has no preconditions,

src/main.rs:
  128  /// `Ok(msg)` prints to stdout (if non-empty) and exits 0.
  129  /// `Err((msg, code))` prints to stderr (if non-empty) and exits with `code`.
  130: #[expect(
  131      clippy::exit,
  132      clippy::print_stdout,


# `#[expect]` Audit — Final Status

**110 → 7** annotations remaining. **103 slain.**

---

## Remaining `#[expect]` Annotations (7 total)

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

### 3. `runtime.rs:19` — `struct_excessive_bools`

```rust
#[expect(clippy::struct_excessive_bools, reason = "Runtime state legitimately needs many boolean flags")]
pub struct State {
    // ... 60+ fields, 12+ booleans:
    // is_streaming, is_tooling, user_scrolled, dirty, dev_mode, perf_enabled,
    // config_view, config_secondary_mode, reverie_enabled, api_check_in_progress,
    // reload_pending, waiting_for_panels
}
```

**Justification:** `State` is the God struct — the single mutable root for all runtime state. Its 12+ boolean flags are genuinely independent status bits: `is_streaming` (LLM active), `dirty` (UI redraw needed), `dev_mode` (debug overlay), `reload_pending` (hot-reload queued), etc. They don't encode a hidden enum (the classic bool-smell). They're independent toggles checked in different subsystems. The struct is large because it's the root of a TUI application, not because of poor design.

**Strategies to eliminate:**
- **Domain sub-structs:** Decompose State into `StreamState { is_streaming, is_tooling, last_stop_reason, streaming_estimated_tokens }`, `UiState { dirty, dev_mode, perf_enabled, config_view, user_scrolled, scroll_offset, ... }`, `LifecycleState { reload_pending, waiting_for_panels, api_check_in_progress }`. Each sub-struct has ≤3 bools (under clippy's threshold). State becomes a composition of typed domains. Touches ~1000 call sites (`state.is_streaming` → `state.stream.is_streaming`) but is scriptable.
- **Bitflags crate:** Replace independent bools with a `bitflags! { struct StateFlags: u32 { const STREAMING = 0x01; const DIRTY = 0x02; ... } }`. Single field, no bool count. Access via `state.flags.contains(StateFlags::STREAMING)`. Slightly less readable at call sites but eliminates the lint entirely.
- **ECS-style components:** Move boolean flags into the `module_data` TypeMap as typed marker structs (e.g., `state.set_ext(Streaming)` / `state.get_ext::<Streaming>().is_some()`). Radical decomposition that leverages existing infrastructure. Probably over-engineered for simple flags.

---

### 4. `runtime.rs:274` — `expect_used` (`ext()`)

```rust
#[expect(clippy::expect_used, reason = "centralized panic — callers use ext() to avoid per-site #[expect]")]
pub fn ext<T: 'static + Send + Sync>(&self) -> &T {
    self.get_ext::<T>().expect("module state not initialized — was init_state() called?")
}
```

**Justification:** This is the centralized TypeMap accessor that 24+ call sites use instead of `get_ext().expect()`. Without this helper, every single module state access would need its own `#[expect(clippy::expect_used)]`. The panic message is specific ("was init_state() called?") and only fires on a programming error (module forgot to register its state during initialization). The `get_ext()` fallible alternative exists for cases where absence is expected.

**Strategies to eliminate:**
- **Init-time registration with compile-time proof:** Use a type-state pattern where `Module::init_state()` returns a `Registered<T>` token. `ext::<T>()` requires `Registered<T>` as a parameter, making it impossible to call without prior registration. The token proves initialization happened — no runtime check needed. Complex lifetime/generics work.
- **`Default` fallback:** Change `ext<T>()` to `ext<T: Default>()` that calls `get_ext::<T>().unwrap_or_default()`. Never panics. Requires all module state types to implement `Default` (most already do since they derive it). Silently returns empty state on programming errors instead of crashing — trades safety for debuggability.
- **Lazy initialization:** Replace `ext()` with `ext_or_init<T: Default>()` that inserts `T::default()` if missing, then returns the reference. Combines accessor + initialization. Eliminates the panic at the cost of masking forgotten `init_state()` calls.

---

### 5. `runtime.rs:284` — `expect_used` (`ext_mut()`)

```rust
#[expect(clippy::expect_used, reason = "centralized panic — callers use ext_mut() to avoid per-site #[expect]")]
pub fn ext_mut<T: 'static + Send + Sync>(&mut self) -> &mut T {
    self.get_ext_mut::<T>().expect("module state not initialized — was init_state() called?")
}
```

**Justification:** Mutable variant of #5. Same centralization rationale — 20+ call sites use this instead of scattered `.expect()` calls. Same panic condition (programming error: module state not registered).

**Strategies to eliminate:** Same as #4 — any strategy applied to `ext()` would be applied identically to `ext_mut()`. These two annotations live and die together.

---

### 6. `server/main.rs:356` — `unsafe_code`

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

### 7. `main.rs:130` — `print_stdout`, `exit`

```rust
#[expect(
    clippy::exit,
    clippy::print_stdout,
    reason = "CLI entry point — printing and process::exit are the correct interface"
)]
fn handle_cli_result(result: Result<String, (String, i32)>) -> ! {
```

**Justification:** `handle_cli_result()` is the single CLI bridge function — all subcommand results flow through it. It prints success messages to stdout and error messages to stderr, then exits with the appropriate code. This is the correct interface for a CLI tool. The annotation covers one 15-line function (previously a module-level `#![expect]` covering an entire file).

**Strategies to eliminate:**
- **`ExitCode` return from main:** Change `main()` from `-> io::Result<()>` to `-> ExitCode`. Handle the CLI subcommand result inline and return `ExitCode::from(code)`. Requires restructuring main's control flow — currently, CLI subcommands diverge early and the TUI path assumes `io::Result<()>`.
- **Separate binary:** Extract the typst CLI subcommands into a standalone `cpilot-typst` binary. Being its own binary, it naturally prints and exits. The main binary drops the subcommand routing entirely.

---

## Summary

| # | File | Lint | Killable? | Best Strategy |
|---|------|------|-----------|---------------|
| 1 | `cast.rs` | `allow_attributes` | 🟡 | `num` crate or `TryFrom` blanket impls |
| 2 | `config/mod.rs` | `panic` | 🟢 | `build.rs` compile-time YAML validation |
| 3 | `runtime.rs` | `struct_excessive_bools` | 🟡 | Domain sub-structs (scriptable, ~1000 callsites) |
| 4–5 | `runtime.rs` | `expect_used` (×2) | 🟡 | `Default` fallback or lazy init |
| 6 | `server/main.rs` | `unsafe_code` | 🔴 | Irreducible — FFI boundary to kernel syscall |
| 7 | `main.rs` | `print_stdout` + `exit` | 🟡 | `ExitCode` return from main |
