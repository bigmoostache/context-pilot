107 bosses, 116 results - 56 files
crates/cp-base/src/cast.rs:
  2      clippy::allow_attributes,
  3:     reason = "macro-generated #[allow] can't use #[expect] — some lint triggers depend on which type the macro expands for"
  4  )]

  417  #[must_use]
  418: [B3] #[expect(clippy::expect_used, reason = "infallible based on prior validation")]
  419  pub fn get_theme(theme_id: &str) -> &'static Theme {

  439  /// Get the currently active theme (single atomic load — no locking, no allocation)
  440: [B4] #[expect(unsafe_code, reason = "atomic pointer deref from static LazyLock — always valid")]
  441  pub fn active_theme() -> &'static Theme {

crates/cp-base/src/state/runtime.rs:
   18  /// Runtime state (messages loaded in memory)
   19: [B5] #[expect(clippy::struct_excessive_bools, reason = "Runtime state legitimately needs many boolean flags")]
   20  pub struct State {

  267      /// Prefer this over `get_ext().expect()` — the panic lives here once,
  268:     /// so callers don't need `[B6] #[expect(clippy::expect_used)]`.
  269      ///

  273      #[must_use]
  274:     [B7] #[expect(clippy::expect_used, reason = "centralized panic — callers use ext() to avoid per-site #[expect]")]
  275      pub fn ext<T: 'static + Send + Sync>(&self) -> &T {

  283      /// Panics if module state `T` was never registered via [`set_ext`](Self::set_ext).
  284:     [B8] #[expect(clippy::expect_used, reason = "centralized panic — callers use ext_mut() to avoid per-site #[expect]")]
  285      pub fn ext_mut<T: 'static + Send + Sync>(&mut self) -> &mut T {

  239      #[must_use]
  240:     [B11] #[expect(clippy::panic, reason = "invariant violation is unrecoverable")]
  241      pub fn from_yaml<'a>(id: &str, texts: &'a ToolTexts) -> ToolDefBuilder<'a> {

crates/cp-mod-brave/src/api.rs:
  62      #[must_use]
  63:     [B12] #[expect(clippy::expect_used, reason = "infallible based on prior validation")]
  64      pub fn new(api_key: String) -> Self {

crates/cp-mod-brave/src/panel.rs:
  89  
  90:     [B13] #[expect(clippy::wildcard_enum_match_arm, reason = "remaining variants are handled uniformly")]
  91      fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {

crates/cp-mod-callback/src/lib.rs:
  32  static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> = std::sync::LazyLock::new(|| {
  33:     [B14] #[expect(clippy::expect_used, reason = "infallible based on prior validation")]
  34      serde_yaml::from_str(include_str!("../../../yamls/tools/callback.yaml"))

crates/cp-mod-console/src/lib.rs:
   55  
   56:     [B15] #[expect(clippy::print_stderr, reason = "TUI stderr logging is intentional")]
   57      fn init_state(&self, state: &mut State) {

  112      }
  113:     [B16] #[expect(clippy::print_stderr, reason = "TUI stderr logging is intentional")]
  114      fn load_module_data(&self, data: &serde_json::Value, state: &mut State) {

crates/cp-mod-console/src/manager.rs:
  147          // Must be done in pre_exec (before exec), not after spawn.
  148:         [B17] #[expect(unsafe_code, reason = "pre_exec requires unsafe — setsid() is safe to call pre-fork")]
  149          // SAFETY: setsid() is async-signal-safe (POSIX) and has no preconditions.

  223  
  224: [B18] #[expect(unsafe_code, reason = "SessionHandle fields are Arc<Mutex<T>> — safe to send across threads")]
  225  // SAFETY: All fields are either Arc<Mutex<T>>, Arc<AtomicBool>, String, u64, or RingBuffer

  227  unsafe impl Send for SessionHandle {}
  228: [B19] #[expect(unsafe_code, reason = "SessionHandle fields are Arc<Mutex<T>> — safe to share across threads")]
  229  // SAFETY: All shared access goes through Arc<Mutex<T>> or Arc<AtomicBool>, providing

  306      #[must_use]
  307:     [B20] #[expect(clippy::significant_drop_tightening, reason = "lock scope is intentional")]
  308      pub fn reconnect(

  408          });
  409:         [B21] #[expect(clippy::branches_sharing_code, reason = "factoring out shared code would reduce clarity")]
  410          if server_request(&req).is_ok() {

  420      /// Kill the process via the server.
  421:     [B22] #[expect(clippy::significant_drop_tightening, reason = "lock scope is intentional")]
  422      pub fn kill(&self) {

crates/cp-mod-console/src/pollers.rs:
  18  
  19: [B23] #[expect(clippy::needless_pass_by_value, reason = "thread::spawn requires owned values")]
  20  pub(crate) fn file_poller_from_offset(path: PathBuf, buffer: RingBuffer, stop: Arc<AtomicBool>, mut offset: u64) {

  60  /// Periodically poll the server for process status.
  61: [B24] #[expect(clippy::needless_pass_by_value, reason = "thread::spawn requires owned values")]
  62: [B25] #[expect(clippy::significant_drop_tightening, reason = "lock scope is intentional")]
  63  pub(crate) fn poll_server_status(

crates/cp-mod-console/src/server/main.rs:
  106  
  107: [B26] #[expect(clippy::unwrap_used, reason = "infallible based on prior validation")]
  108  fn handle_create(sessions: &Sessions, key: &str, command: &str, cwd: Option<&str>, log_path: &str) -> Response {

  160  
  161: [B27] #[expect(clippy::significant_drop_tightening, reason = "lock scope is intentional")]
  162: [B28] #[expect(clippy::unwrap_used, reason = "infallible based on prior validation")]
  163  fn handle_send(sessions: &Sessions, key: &str, input: &str) -> Response {

  185  
  186: [B29] #[expect(clippy::significant_drop_tightening, reason = "lock scope is intentional")]
  187: [B30] #[expect(clippy::unwrap_used, reason = "infallible based on prior validation")]
  188  fn handle_kill(sessions: &Sessions, key: &str) -> Response {

src/app/run/input.rs:
   10      /// Mutates `AutocompleteState` and state.input directly.
   11:     [B80] #[expect(clippy::wildcard_enum_match_arm, reason = "remaining variants are handled uniformly")]
   12      pub(super) fn handle_autocomplete_event(&mut self, event: &event::Event) {

   36  
   37:                 [B81] #[expect(clippy::branches_sharing_code, reason = "factoring out shared code would reduce clarity")]
   38                  if is_dir {

  152      /// Mutates the `PendingQuestionForm` directly in state.
  153:     [B82] #[expect(clippy::wildcard_enum_match_arm, reason = "remaining variants are handled uniformly")]
  154      pub(super) fn handle_question_form_event(&mut self, event: &event::Event) {

  210      /// Handle keyboard events when command palette is open
  211:     [B83] #[expect(clippy::wildcard_enum_match_arm, reason = "remaining variants are handled uniformly")]
  212      pub(super) fn handle_palette_event(&mut self, event: &event::Event) -> Option<Action> {

src/app/run/lifecycle.rs:
  23  impl App {
  24:     [B84] #[expect(clippy::needless_pass_by_value, reason = "thread::spawn requires owned values")]
  25      pub(crate) fn run(

src/app/run/streaming.rs:
  150  
  151:     [B85] #[expect(clippy::wildcard_enum_match_arm, reason = "remaining variants are handled uniformly")]
  152      pub(super) fn finalize_stream(&mut self) {

src/infra/tools.rs:
  24  /// Perform the actual TUI reload (called from app.rs after tool result is saved)
  25: [B86] #[expect(clippy::exit, reason = "process exit is intentional here")]
  26  pub(crate) fn perform_reload(state: &State) {

src/llms/openai_streaming.rs:
  45  #[derive(Debug, Deserialize)]
  46: [B87] #[expect(clippy::struct_field_names, reason = "Field names mirror the OpenAI API response")]
  47  pub(crate) struct StreamUsage {

src/llms/claude_code/mod.rs:
  158  /// Convert content (string or array) to an array of content blocks.
  159: [B88] #[expect(clippy::needless_pass_by_value, reason = "thread::spawn requires owned values")]
  160  fn content_to_blocks(content: Value) -> Vec<Value> {

  280  #[derive(Debug, Deserialize)]
  281: [B89] #[expect(clippy::struct_field_names, reason = "Field names mirror the Anthropic API response")]
  282  struct StreamUsage {

src/llms/claude_code/stream.rs:
  18  impl ClaudeCodeClient {
  19:     [B90] #[expect(clippy::needless_pass_by_value, reason = "thread::spawn requires owned values")]
  20      pub(super) fn do_stream(&self, request: LlmRequest, tx: Sender<StreamEvent>) -> Result<(), LlmError> {

src/llms/claude_code_api_key/helpers.rs:
  145  /// Convert content (string or array) to an array of content blocks.
  146: [B91] #[expect(clippy::needless_pass_by_value, reason = "thread::spawn requires owned values")]
  147  pub(crate) fn content_to_blocks(content: Value) -> Vec<Value> {

src/llms/claude_code_api_key/streaming.rs:
  45  #[derive(Debug, Deserialize)]
  46: [B92] #[expect(clippy::struct_field_names, reason = "Field names mirror the Anthropic API response")]
  47  pub(super) struct StreamUsage {

src/modules/mod.rs:
   14  static CORE_TOOL_TEXTS: std::sync::LazyLock<ToolTexts> = std::sync::LazyLock::new(|| {
   15:     [B93] #[expect(clippy::expect_used, reason = "infallible based on prior validation")]
   16      serde_yaml::from_str(include_str!("../../yamls/tools/core.yaml")).expect("Failed to parse core tool YAML")

  228  /// Execute the `module_toggle` tool.
  229: [B94] #[expect(clippy::expect_used, reason = "infallible based on prior validation")]
  230  fn execute_module_toggle(tool: &ToolUse, state: &mut State) -> ToolResult {

src/modules/conversation/panel.rs:
  268  
  269:     [B95] #[expect(clippy::wildcard_enum_match_arm, reason = "remaining variants are handled uniformly")]
  270      fn handle_key(&self, key: &KeyEvent, state: &State) -> Option<Action> {

src/modules/conversation/render.rs:
  23  /// Render a single message to lines (without caching logic)
  24: [B96] #[expect(clippy::wildcard_enum_match_arm, reason = "remaining variants are handled uniformly")]
  25  pub(crate) fn render_message(

  91  
  92:             [B97] #[expect(clippy::branches_sharing_code, reason = "factoring out shared code would reduce clarity")]
  93              if let Some(vis_lines) = custom_lines {

src/modules/overview/panel.rs:
  13  impl Panel for OverviewPanel {
  14:     [B98] #[expect(clippy::wildcard_enum_match_arm, reason = "remaining variants are handled uniformly")]
  15      fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {

src/modules/overview/tools_panel.rs:
  11  impl Panel for ToolsPanel {
  12:     [B99] #[expect(clippy::wildcard_enum_match_arm, reason = "remaining variants are handled uniformly")]
  13      fn handle_key(&self, key: &KeyEvent, _state: &State) -> Option<Action> {

src/modules/questions/mod.rs:
  10  static TOOL_TEXTS: std::sync::LazyLock<ToolTexts> = std::sync::LazyLock::new(|| {
  11:     [B100] #[expect(clippy::expect_used, reason = "infallible based on prior validation")]
  12      serde_yaml::from_str(include_str!("../../../yamls/tools/questions.yaml"))

src/state/persistence/mod.rs:
  407  /// Prefer `build_save_batch` + `PersistenceWriter::send_batch` in the main event loop.
  408: [B101] #[expect(clippy::print_stderr, reason = "TUI stderr logging is intentional")]
  409  pub(crate) fn save_state(state: &State) {

src/state/persistence/writer.rs:
   62      /// Create a new persistence writer with a background thread
   63:     [B102] #[expect(clippy::expect_used, reason = "infallible based on prior validation")]
   64      pub(crate) fn new() -> Self {

   90      /// Used on app exit to ensure all state is persisted.
   91:     [B103] #[expect(clippy::significant_drop_tightening, reason = "lock scope is intentional")]
   92      pub(crate) fn flush(&self) {

  133  /// The writer thread's main loop
  134: [B104] #[expect(clippy::needless_pass_by_value, reason = "thread::spawn requires owned values")]
  135: [B105] #[expect(clippy::significant_drop_tightening, reason = "lock scope is intentional")]
  136  fn writer_loop(rx: Receiver<WriterMsg>, flush_sync: Arc<(Mutex<bool>, Condvar)>) {

  199  /// Execute a batch of write/delete operations
  200: [B106] #[expect(clippy::print_stderr, reason = "TUI stderr logging is intentional")]
  201  fn execute_batch(batch: Option<WriteBatch>) {

  227  /// Logs errors instead of silently swallowing them.
  228: [B107] #[expect(clippy::print_stderr, reason = "TUI stderr logging is intentional")]
  229  fn write_file(path: &PathBuf, content: &[u8]) {

src/ui/perf/mod.rs:
  153      /// Record operation timing
  154:     [B108] #[expect(clippy::significant_drop_tightening, reason = "lock scope is intentional")]
  155      pub(crate) fn record_op(&self, name: &'static str, duration_us: u64) {

  199      /// Refresh CPU and memory stats
  200:     [B109] #[expect(clippy::significant_drop_tightening, reason = "lock scope is intentional")]
  201      fn refresh_system_stats(&self) {

  221      /// Get snapshot of metrics for display
  222:     [B110] #[expect(clippy::significant_drop_tightening, reason = "lock scope is intentional")]
  223      pub(crate) fn snapshot(&self) -> PerfSnapshot {

---

## ☠️ BOUNTY BOARD — Kill Record

| Boss | Lint | File | XP | Status |
|------|------|------|----|--------|
| B1 | allow_attributes | cast.rs | 5 | 🔴 ALIVE |
| B2 | panic | config/mod.rs | 10 | 🔴 ALIVE |
| B3 | expect_used | config/mod.rs | 5 | ☠️ SLAIN |
| B4 | unsafe_code | config/mod.rs | 25 | 🔴 ALIVE |
| B5 | struct_excessive_bools | runtime.rs | 50 | 🔴 ALIVE |
| B6 | *(doc comment ref)* | runtime.rs | — | ⚪ N/A |
| B7 | expect_used | runtime.rs | 5 | 🔴 ALIVE |
| B8 | expect_used | runtime.rs | 5 | 🔴 ALIVE |
| B9 | needless_pass_by_value | runtime.rs | 15 | 💀 SLAIN |
| B10 | expect_used | tools/mod.rs | 5 | 🔴 ALIVE |
| B11 | panic | tools/mod.rs | 10 | 🔴 ALIVE |
| B12 | expect_used | brave/api.rs | 5 | ☠️ SLAIN |
| B13 | wildcard_enum_match_arm | brave/panel.rs | 5 | 💀 SLAIN |
| B14 | expect_used | callback/lib.rs | 5 | ☠️ SLAIN |
| B15 | print_stderr | console/lib.rs | 5 | ☠️ SLAIN |
| B16 | print_stderr | console/lib.rs | 5 | ☠️ SLAIN |
| B17 | unsafe_code | manager.rs | 25 | 🔴 ALIVE |
| B18 | unsafe_code | manager.rs | 25 | 🔴 ALIVE |
| B19 | unsafe_code | manager.rs | 25 | 🔴 ALIVE |
| B20 | significant_drop_tightening | manager.rs | 20 | 💀 SLAIN |
| B21 | branches_sharing_code | manager.rs | 40 | 💀 SLAIN |
| B22 | significant_drop_tightening | manager.rs | 20 | 💀 SLAIN |
| B23 | needless_pass_by_value | pollers.rs | 15 | 🔴 ALIVE |
| B24 | needless_pass_by_value | pollers.rs | 15 | 🔴 ALIVE |
| B25 | significant_drop_tightening | pollers.rs | 20 | 💀 SLAIN |
| B26 | unwrap_used | server/main.rs | 10 | 💀 SLAIN |
| B27 | significant_drop_tightening | server/main.rs | 20 | 🔴 ALIVE |
| B28 | unwrap_used | server/main.rs | 10 | 💀 SLAIN |
| B29 | significant_drop_tightening | server/main.rs | 20 | 🔴 ALIVE |
| B30 | unwrap_used | server/main.rs | 10 | 💀 SLAIN |
| B31 | significant_drop_tightening | server/main.rs | 20 | 💀 SLAIN |
| B32 | unwrap_used | server/main.rs | 10 | 💀 SLAIN |
| B33 | significant_drop_tightening | server/main.rs | 20 | 💀 SLAIN |
| B34 | unwrap_used | server/main.rs | 10 | 💀 SLAIN |
| B35 | significant_drop_tightening | server/main.rs | 20 | 💀 SLAIN |
| B36 | unwrap_used | server/main.rs | 10 | 💀 SLAIN |
| B37 | needless_pass_by_value | server/main.rs | 15 | ☠️ SLAIN |
| B38 | significant_drop_tightening | server/main.rs | 20 | 💀 SLAIN |
| B39 | unwrap_used | server/main.rs | 10 | 💀 SLAIN |
| B40 | exit | server/main.rs | 10 | 🔴 ALIVE |
| B41 | print_stderr | server/main.rs | 5 | ☠️ SLAIN |
| B42 | expect_used | server/main.rs | 5 | ☠️ SLAIN |
| B43 | unsafe_code | server/main.rs | 25 | 🔴 ALIVE |
| B44 | unwrap_used | server/main.rs | 10 | 💀 SLAIN |
| B45 | wildcard_enum_match_arm | files/panel.rs | 5 | 💀 SLAIN |
| B46 | expect_used | firecrawl/api.rs | 5 | ☠️ SLAIN |
| B47 | expect_used | firecrawl/lib.rs | 5 | ☠️ SLAIN |
| B48 | wildcard_enum_match_arm | firecrawl/panel.rs | 5 | 💀 SLAIN |
| B49 | expect_used | git/cache_invalidation.rs | 5 | ☠️ SLAIN |
| B50 | expect_used | git/lib.rs | 5 | ☠️ SLAIN |
| B51 | wildcard_enum_match_arm | git/result_panel.rs | 5 | 💀 SLAIN |
| B52 | wildcard_enum_match_arm | git/result_panel.rs | 5 | 💀 SLAIN |
| B53 | expect_used | github/cache_invalidation.rs | 5 | ☠️ SLAIN |
| B54 | wildcard_enum_match_arm | github/panel.rs | 5 | 💀 SLAIN |
| B55 | wildcard_enum_match_arm | github/panel.rs | 5 | 💀 SLAIN |
| B56 | needless_pass_by_value | github/watcher.rs | 15 | ☠️ SLAIN |
| B57 | wildcard_enum_match_arm | memory/panel.rs | 5 | 💀 SLAIN |
| B58 | print_stderr | preset/builtin.rs | 5 | ☠️ SLAIN |
| B59 | print_stderr | preset/builtin.rs | 5 | ☠️ SLAIN |
| B60 | struct_field_names | preset/lib.rs | 15 | ☠️ SLAIN |
| B61 | expect_used | queue/types.rs | 5 | ☠️ SLAIN |
| B62 | expect_used | queue/types.rs | 5 | ☠️ SLAIN |
| B63 | expect_used | scratchpad/lib.rs | 5 | ☠️ SLAIN |
| B64 | wildcard_enum_match_arm | scratchpad/panel.rs | 5 | 💀 SLAIN |
| B65 | wildcard_enum_match_arm | spine/panel.rs | 5 | 💀 SLAIN |
| B66 | wildcard_enum_match_arm | todo/panel.rs | 5 | 💀 SLAIN |
| B67 | wildcard_enum_match_arm | tree/panel.rs | 5 | 💀 SLAIN |
| B68 | print_stdout | main.rs | 5 | ☠️ SLAIN |
| B69 | print_stderr | main.rs | 5 | ☠️ SLAIN |
| B70 | exit | main.rs | 10 | ☠️ SLAIN |
| B71 | print_stdout | main.rs | 5 | ☠️ SLAIN |
| B72 | print_stderr | main.rs | 5 | ☠️ SLAIN |
| B73 | exit | main.rs | 10 | ☠️ SLAIN |
| B74 | wildcard_enum_match_arm | events.rs | 5 | 💀 SLAIN |
| B75 | wildcard_enum_match_arm | events.rs | 5 | 💀 SLAIN |
| B76 | wildcard_enum_match_arm | actions/config.rs | 5 | 💀 SLAIN |
| B77 | expect_used | actions/helpers.rs | 5 | ☠️ SLAIN |
| B78 | expect_used | actions/helpers.rs | 5 | ☠️ SLAIN |
| B79 | needless_pass_by_value | context/mod.rs | 15 | 💀 SLAIN |
| B80 | wildcard_enum_match_arm | run/input.rs | 5 | 💀 SLAIN |
| B81 | branches_sharing_code | run/input.rs | 40 | 💀 SLAIN |
| B82 | wildcard_enum_match_arm | run/input.rs | 5 | 💀 SLAIN |
| B83 | wildcard_enum_match_arm | run/input.rs | 5 | 💀 SLAIN |
| B84 | needless_pass_by_value | run/lifecycle.rs | 15 | ☠️ SLAIN |
| B85 | wildcard_enum_match_arm | run/streaming.rs | 5 | 💀 SLAIN |
| B86 | exit | infra/tools.rs | 10 | 🔴 ALIVE |
| B87 | struct_field_names | openai_streaming.rs | 15 | ☠️ SLAIN |
| B88 | needless_pass_by_value | claude_code/mod.rs | 15 | ☠️ SLAIN |
| B89 | struct_field_names | claude_code/mod.rs | 15 | ☠️ SLAIN |
| B90 | needless_pass_by_value | claude_code/stream.rs | 15 | ☠️ SLAIN |
| B91 | needless_pass_by_value | claude_code_api_key/helpers.rs | 15 | ☠️ SLAIN |
| B92 | struct_field_names | claude_code_api_key/streaming.rs | 15 | ☠️ SLAIN |
| B93 | expect_used | modules/mod.rs | 5 | ☠️ SLAIN |
| B94 | expect_used | modules/mod.rs | 5 | ☠️ SLAIN |
| B95 | wildcard_enum_match_arm | conversation/panel.rs | 5 | 💀 SLAIN |
| B96 | wildcard_enum_match_arm | conversation/render.rs | 5 | 💀 SLAIN |
| B97 | branches_sharing_code | conversation/render.rs | 40 | 💀 SLAIN |
| B98 | wildcard_enum_match_arm | overview/panel.rs | 5 | 💀 SLAIN |
| B99 | wildcard_enum_match_arm | overview/tools_panel.rs | 5 | 💀 SLAIN |
| B100 | expect_used | questions/mod.rs | 5 | ☠️ SLAIN |
| B101 | print_stderr | persistence/mod.rs | 5 | ☠️ SLAIN |
| B102 | expect_used | persistence/writer.rs | 5 | ☠️ SLAIN |
| B103 | significant_drop_tightening | persistence/writer.rs | 20 | 🔴 ALIVE |
| B104 | needless_pass_by_value | persistence/writer.rs | 15 | ☠️ SLAIN |
| B105 | significant_drop_tightening | persistence/writer.rs | 20 | 🔴 ALIVE |
| B106 | print_stderr | persistence/writer.rs | 5 | ☠️ SLAIN |
| B107 | print_stderr | persistence/writer.rs | 5 | ☠️ SLAIN |
| B108 | significant_drop_tightening | perf/mod.rs | 20 | 🔴 ALIVE |
| B109 | significant_drop_tightening | perf/mod.rs | 20 | 💀 SLAIN |
| B110 | significant_drop_tightening | perf/mod.rs | 20 | 💀 SLAIN |

### XP Legend
| XP | Difficulty | Lint Types |
|----|-----------|------------|
| 5 | 🟢 Minion | expect_used, wildcard_enum_match_arm, print_stderr, print_stdout, allow_attributes |
| 10 | 🟡 Grunt | panic, unwrap_used, exit |
| 15 | 🟠 Lieutenant | needless_pass_by_value, struct_field_names |
| 20 | 🔴 Captain | significant_drop_tightening |
| 25 | 💀 Quartermaster | unsafe_code |
| 40 | 👹 Raid Boss | branches_sharing_code |
| 50 | 🐙 Kraken | struct_excessive_bools |

### Score
```
Slain: 43 / 110
XP earned: 335 / 1215
```
