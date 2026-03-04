crates/cp-base/src/cast.rs:
  1: #![expect(
  2      clippy::allow_attributes,

crates/cp-base/src/panels.rs:
  41  #[must_use]
  42: #[expect(clippy::wildcard_enum_match_arm, reason = "KeyCode is an external enum — new variants are not scroll keys")]
  43  pub const fn scroll_key_action(key: &KeyEvent) -> Option<Action> {

crates/cp-base/src/config/mod.rs:
  379  /// Deserialize a YAML string into `T`, panicking with a descriptive message on failure.
  380: #[expect(clippy::panic, reason = "invariant violation is unrecoverable")]
  381  fn parse_yaml<T: for<'de> Deserialize<'de>>(name: &str, content: &str) -> T {

  454  /// Panics if the themes map contains zero entries.
  455: #[expect(clippy::expect_used, reason = "themes.yaml is embedded at compile time — empty map is a build-time bug")]
  456  pub fn active_theme() -> &'static Theme {

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

crates/cp-base/src/tools/mod.rs:
   27      #[must_use]
   28:     #[expect(
   29          clippy::expect_used,

  239      #[must_use]
  240:     #[expect(clippy::panic, reason = "invariant violation is unrecoverable")]
  241      pub fn from_yaml<'a>(id: &str, texts: &'a ToolTexts) -> ToolDefBuilder<'a> {

crates/cp-mod-console/src/manager.rs:
  147          // Must be done in pre_exec (before exec), not after spawn.
  148:         #[expect(unsafe_code, reason = "pre_exec requires unsafe — setsid() is safe to call pre-fork")]
  149          // SAFETY: setsid() is async-signal-safe (POSIX) and has no preconditions.

crates/cp-mod-console/src/server/main.rs:
    1  //! Console Server: persistent daemon that owns child processes.
    2: #![expect(unused_crate_dependencies, reason = "bin target shares Cargo.toml with lib — lib deps aren't used here")]
    3  //!

  402  /// Install SIGTERM and SIGINT handlers that set `SHUTDOWN_REQUESTED`.
  403: #[expect(
  404      unsafe_code,

src/typst_cli.rs:
  4  //! printing and `process::exit` are the expected interface.
  5: #![expect(
  6      clippy::print_stdout,

---

## 🔍 Remaining `#[expect]` — Deep Analysis

10 production annotations remain (down from 110). Each one analyzed: why it exists today, and what changes — to the code, infrastructure, or Rust language — could eliminate it.

### B1 — `allow_attributes` (cast.rs)

**Lint:** `clippy::allow_attributes`
**Location:** Module-level `#![expect(clippy::allow_attributes)]` on `cast.rs`
**What it guards:** The `SafeCast` derive macro generates `#[allow(clippy::cast_possible_truncation)]` (and similar) inside `impl` blocks. Clippy's `allow_attributes` lint wants `#[expect]` instead of `#[allow]`, but whether the lint fires depends on **which concrete types** the macro expands for — some casts truncate, some don't. The macro can't conditionally emit `#[expect]` vs `#[allow]` per expansion.

**Why it's needed today:**
Stable Rust's proc macros cannot query type information at expansion time. The macro doesn't know if `u64 as u32` truncates but `u32 as u64` doesn't — it emits the same `#[allow]` for both. Using `#[expect]` would cause "unfulfilled expectation" errors on non-truncating expansions.

**Elimination strategies:**
1. **Rust language evolution:** If `#[expect]` gains a `soft` mode (fire only if the lint triggers, no error if it doesn't), the macro could emit `#[expect(soft, ...)]` universally. Track [rust-lang/rust #54503](https://github.com/rust-lang/rust/issues/54503) for `lint_reasons` stabilization progress.
2. **Split the macro:** Generate separate `impl` blocks for truncating vs. non-truncating cast directions, with `#[expect]` only on the truncating ones. Requires manually maintaining a type-size table in the macro.
3. **Abandon the macro:** Write each `SafeCast` impl by hand. Verbose (~20 impls) but each gets precise `#[expect]` annotations. Trade-off: maintenance burden vs. lint purity.
4. **Use `TryFrom` everywhere:** Replace `SafeCast` with fallible conversions (`TryFrom`/`TryInto`) returning `Result`. Eliminates cast lints entirely but changes semantics — callers must handle errors instead of getting silent clamping.

---

### B2 — `panic` (config/mod.rs `parse_yaml`)

**Lint:** `clippy::panic`
**Location:** `fn parse_yaml<T>(name: &str, content: &str) -> T`
**What it guards:** `serde_yaml::from_str(content).unwrap_or_else(|e| panic!("Failed to parse {name}: {e}"))` — YAML deserialization that panics on malformed input.

**Why it's needed today:**
`parse_yaml` deserializes compile-time-embedded YAML (`include_str!`). The content is baked into the binary — if it's malformed, the build produced a broken artifact. Returning `Result` would force 6 `LazyLock` static initializers to handle errors they can never encounter in a valid build.

**Elimination strategies:**
1. **Build-time validation:** Add a `build.rs` step that deserializes all YAML files during compilation. If it passes, the runtime `parse_yaml` can never fail — but you still need to handle the `Result` at the type level (or use `unreachable!()`).
2. **Integration test gate:** A `#[test]` that deserializes every YAML file. Weaker than `build.rs` (doesn't block the build) but catches regressions in CI. The runtime panic becomes unreachable-by-test-coverage.
3. **Const evaluation (future Rust):** When `serde` supports `const fn` deserialization, the YAML can be parsed at compile time with a compile error on failure. No runtime panic needed. Years away.
4. **Accept it:** This is the textbook use case for `panic!` — an invariant violation indicating a corrupted binary. The Rust docs explicitly endorse panicking for unrecoverable logic bugs.

---

### B5 — `struct_excessive_bools` (runtime.rs `State`)

**Lint:** `clippy::struct_excessive_bools`
**Location:** `pub struct State` — the central runtime state with 12+ boolean fields.
**What it guards:** Clippy warns that structs with many bools suggest they should be an enum or bitflags.

**Why it's needed today:**
`State` is the God struct of the TUI — it holds all runtime state. The booleans are genuinely independent flags: `is_streaming`, `is_tooling`, `user_scrolled`, `dirty`, `dev_mode`, `perf_enabled`, `config_view`, `config_secondary_mode`, `reverie_enabled`, `api_check_in_progress`, `reload_pending`, `waiting_for_panels`. They represent orthogonal concerns — no enum-like grouping is possible because many are simultaneously true.

**Elimination strategies:**
1. **State decomposition:** Split `State` into sub-structs by domain: `StreamState { is_streaming, is_tooling, estimated_tokens, last_stop_reason }`, `UiState { dirty, dev_mode, perf_enabled, config_view, config_secondary_mode, sidebar_mode }`, `LifecycleState { reload_pending, waiting_for_panels, api_check_in_progress }`. Each sub-struct has ≤3 bools — below clippy's threshold. Massive refactor touching every file that reads `state.is_streaming` (now `state.stream.active`). Improves architectural clarity but adds indirection.
2. **Bitflags:** `bitflags! { struct StateFlags: u16 { const STREAMING = 0x01; const TOOLING = 0x02; ... } }`. Semantic downgrade — bools are self-documenting, bitflags require lookup. Saves memory (12 bytes → 2 bytes) but memory is irrelevant for a singleton.
3. **Partial enums:** Where booleans ARE mutually exclusive, replace with enums. `config_view` + `config_secondary_mode` → `enum ConfigOverlay { Hidden, Primary, Secondary }`. Only saves 1-2 bools — likely not enough alone.
4. **Raise the threshold:** `clippy.toml`: `excessive-bools-threshold = 15`. Tolerates the God struct, still catches genuinely problematic smaller structs. Technically a ceasefire, not elimination.

---

### B7 / B8 — `expect_used` (runtime.rs `ext()` / `ext_mut()`)

**Lint:** `clippy::expect_used`
**Location:** `State::ext<T>()` and `State::ext_mut<T>()` — centralized TypeMap accessors.
**What it guards:** `.expect("module state not initialized — was init_state() called?")` on `HashMap<TypeId, Box<dyn Any>>` lookups.

**Why they're needed today:**
These are the **centralized panic points** for the module TypeMap pattern. Every module registers its state at startup via `init_state()`. The alternative — `get_ext::<T>()` returning `Option` — exists and is public, but callers would need `unwrap`/`expect` at 50+ call sites, spreading the lint instead of centralizing it.

**Elimination strategies:**
1. **Compile-time module registration:** A proc macro or const generic enforcing that every `Module` impl that declares a state type must register it. Something like `trait Module { type State: Default; }` where the framework auto-inserts `state.set_ext(Self::State::default())`. Requires significant trait redesign.
2. **Builder pattern with proof tokens:** `register_state()` returns a `ModuleStateHandle<T>` (ZST). `ext::<T>()` requires the handle as proof the state exists. Compile-time guarantee, zero runtime cost. Moderate refactor.
3. **`LazyLock` per module:** Each module stores its state in a crate-level `static LazyLock<Mutex<MyState>>` instead of the TypeMap. No centralized accessor — each module owns its state. Downside: loses the single `&mut State` borrow model, introduces lock contention.
4. **Accept it:** `ext()`/`ext_mut()` exist precisely so 50+ call sites don't each need `#[expect(clippy::expect_used)]`. The panic message is clear and actionable. This is textbook centralized error handling.

---

### B10 — `expect_used` (tools/mod.rs `ToolTexts::parse()`)

**Lint:** `clippy::expect_used`
**Location:** `ToolTexts::parse(yaml: &str) -> Self` — `.expect("embedded tool YAML is malformed")`
**What it guards:** YAML deserialization of compile-time-embedded tool descriptions.

**Why it's needed today:**
Same pattern as B2 — the YAML is `include_str!`'d at compile time. `parse()` centralizes the panic so 10+ `LazyLock` statics across module crates don't each need `#[expect]`.

**Elimination strategies:**
Same as B2 (build-time validation, const evaluation). Additionally:
1. **Merge with `parse_yaml()`:** Unify `ToolTexts::parse()` and `parse_yaml()` into a single `embedded_yaml::<T>(content)` function with one `#[expect]`. Reduces 2 annotations to 1.
2. **Infallible deserialization type:** Implement `Default` for `ToolTexts` so `serde_yaml::from_str(yaml).unwrap_or_default()` returns an empty tool set on bad YAML. No panic, no expect. Downside: silent failure — tools just vanish instead of crashing loudly.

---

### B11 — `panic` (tools/mod.rs `from_yaml()`)

**Lint:** `clippy::panic`
**Location:** `ToolDefinition::from_yaml(id, texts)` — panics if tool ID not found in YAML.
**What it guards:** `texts.tools.get(id).unwrap_or_else(|| panic!("Tool '{id}' not found in YAML"))` — tool definition lookup at startup.

**Why it's needed today:**
Tool IDs are hardcoded string literals in each module's `tools()` method. A missing key means a code/config mismatch that should crash immediately with a clear message.

**Elimination strategies:**
1. **Build-time cross-reference:** A `build.rs` or integration test that parses all YAML files and verifies every `from_yaml("X", ...)` call has a matching key. Catches bugs before runtime.
2. **Code generation:** Generate Rust tool definition stubs from YAML at build time. If the YAML key exists, the Rust code exists. No runtime lookup needed.
3. **Typed tool IDs:** Replace string IDs with `enum ToolId { Open, Edit, Write, ... }` generated from YAML keys. `from_yaml(ToolId::Open, texts)` cannot reference a missing key. Significant refactor.
4. **Fallback to empty:** Return a `ToolDefinition` with an empty description. Bad idea — a silently broken tool is worse than a loud crash.

---

### B17 — `unsafe_code` (manager.rs `pre_exec` + `setsid`)

**Lint:** `unsafe_code`
**Location:** `cmd.pre_exec(|| { libc::setsid(); ... })` in `find_or_create_server()`.
**What it guards:** Runs between `fork()` and `exec()` in the child process. `setsid()` makes the server a session leader so children get SIGHUP on server death.

**Why it's needed today:**
`CommandExt::pre_exec` is inherently `unsafe` — the closure runs in a forked-but-not-yet-exec'd context where most operations are UB. Only async-signal-safe functions are allowed. `libc::setsid()` is async-signal-safe (POSIX). No safe API exists in Rust's stdlib.

**Elimination strategies:**
1. **Server self-daemonizes:** Move `setsid()` into the server binary's `main()`. The TUI spawns a normal `Command` without `pre_exec`. The unsafe moves but doesn't disappear.
2. **`process_group` stabilization:** Rust's unstable `CommandExt::process_group()` provides a safe API. Track [rust-lang/rust #93857](https://github.com/rust-lang/rust/issues/93857). Once stable, replaces `pre_exec` + `setsid()`.
3. **Spawn via `setsid(1)` command:** `Command::new("setsid").arg("--fork").arg(&binary).spawn()`. The util-linux `setsid` binary does the same thing. Eliminates all unsafe but adds an external tool dependency.
4. **`nix` crate:** `nix::unistd::setsid()` is safe Rust, but `pre_exec` itself is still `unsafe` — this only reduces the unsafe surface, doesn't eliminate it.

---

### B43 — `unsafe_code` (server/main.rs `install_signal_handlers`)

**Lint:** `unsafe_code`
**Location:** `libc::signal(libc::SIGINT, handler)` and `libc::signal(libc::SIGHUP, handler)`.
**What it guards:** C-level signal handlers that set `AtomicBool` for graceful shutdown.

**Why it's needed today:**
Rust's stdlib has no safe signal handling API. `libc::signal()` requires `unsafe` because the handler runs in interrupt context. Our handler only does `AtomicBool::store(true, Relaxed)` — async-signal-safe.

**Elimination strategies:**
1. **`signal-hook` crate:** Safe API, drop-in replacement. `signal_hook::flag::register(SIGINT, arc_clone)` does exactly what our code does. Zero unsafe. Idiomatic Rust solution. Trade-off: adds one dependency.
2. **`ctrlc` crate:** Simpler API for SIGINT/SIGTERM. Doesn't cover SIGHUP — would need `signal-hook` for that.
3. **Socket-based shutdown:** Server detects shutdown when TUI closes the Unix socket / sends a `"shutdown"` command (already supported). Remove signal handlers entirely, rely on protocol-level shutdown. Loses standalone Ctrl+C support.
4. **`tokio::signal` (async):** Safe future-based API — overkill for a synchronous server. Only relevant if the server goes async.

---

### B_new — `expect_used` (config/mod.rs `active_theme()`)

**Lint:** `clippy::expect_used`
**Location:** `.expect("themes.yaml has no themes")` — fallback after index + HashMap lookup.
**What it guards:** Triple fallback chain: requested → default → any theme → panic. Only panics if `themes.yaml` has zero entries.

**Why it's needed today:**
Called 50+ times per render frame. Must return `&'static Theme`, not `Option`. The themes map is `include_str!`'d at compile time — an empty map is artifact corruption.

**Elimination strategies:**
Same as B2/B10 (build-time validation). Additionally:
1. **Hardcoded fallback theme:** `const FALLBACK: Theme = Theme { ... }` with sensible defaults. If map is empty, return `&FALLBACK`. Verbose (Theme has 6 nested structs, ~30 fields) but eliminates the panic. App works with ugly defaults instead of crashing.
2. **Merge with B2:** Validate non-emptiness inside `parse_yaml` for `ThemesConfig`. Then `active_theme()` can trust the map is populated — but Rust's type system doesn't encode "non-empty HashMap" so the expect is still needed at the type level.
3. **`NonEmpty<HashMap>` wrapper:** Custom type that guarantees `.values().next()` always succeeds. Construction panics (same as B2), but `active_theme()` becomes infallible.

---

### Sentinel — `wildcard_enum_match_arm` (panels.rs `scroll_key_action`)

**Lint:** `clippy::wildcard_enum_match_arm`
**Location:** `_ => None` in `scroll_key_action()` — catches all non-scroll keys.
**What it guards:** The function only cares about 4 keys (Up/Down/PageUp/PageDown). Everything else returns `None`.

**Why it's needed today:**
`KeyCode` is an **external enum** from `crossterm` with 25+ variants (including `F(u8)` = 256 values). Exhaustively matching all of them would be ~20 lines of `=> None` arms that add zero information and break on every `crossterm` update.

**Elimination strategies:**
1. **Exhaustive match:** List all 25+ variants explicitly. Breaks on every crossterm version bump. Actively harmful for maintainability.
2. **`matches!` macro:** `if matches!(key.code, KeyCode::Up | KeyCode::Down | ...) { ... } else { None }`. Avoids the match arm entirely — but clippy may still flag it depending on version.
3. **Accept it:** This IS the correct use of wildcards. The lint catches forgotten variants in *your own* enums. For external enums where you intentionally ignore most variants, the wildcard is correct. The `#[expect]` with its reason string documents this intent.

---

### Summary: Most Promising Elimination Paths

| Priority | Effort | Impact | Strategy |
|----------|--------|--------|----------|
| 🟢 Easy | Low | -1 annotation | **B43:** Add `signal-hook` crate, delete `install_signal_handlers` unsafe block |
| 🟢 Easy | Low | -1 annotation | **B17:** Spawn via `setsid(1)` command, or move `setsid()` into server `main()` |
| 🟡 Medium | Medium | -2 annotations | **B2 + B10:** `build.rs` YAML validation or merge into single `embedded_yaml()` |
| 🟡 Medium | Medium | -1 annotation | **B5:** Decompose `State` into domain sub-structs |
| 🔴 Hard | High | -2 annotations | **B7/B8:** Builder proof-token pattern for module registration |
| 🔴 Hard | High | -1 annotation | **B1:** Rewrite SafeCast as hand-written impls |
| ⏳ Wait | — | -2+ annotations | **Rust evolution:** `process_group` stabilization (B17), `#[expect(soft)]` (B1), const serde (B2/B10) |
| 🚫 Don't | — | — | **B11 / B_new / Sentinel:** Current approach is genuinely optimal |
