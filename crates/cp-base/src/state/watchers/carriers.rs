//! Watcher result carriers: `WatcherResult`, `DeferredPanel`, `DynPanel`.
//!
//! Extracted from `watchers.rs` for the 500-line structure cap.

/// Result of a satisfied watcher condition.
#[derive(Debug)]
pub struct WatcherResult {
    /// Human-readable description of what happened.
    pub description: String,
    /// Panel ID associated with this watcher (if any).
    pub panel_id: Option<String>,
    /// Tool use ID for blocking watchers that need sentinel replacement.
    pub tool_use_id: Option<String>,
    /// If true, the panel should be auto-closed (removed from context).
    /// Used by callback watchers to clean up console panels on success.
    pub close_panel: bool,
    /// If set, `tool_cleanup` should create a console panel for this session.
    /// Used by callback watchers that defer panel creation until failure.
    /// Contains (`session_key`, `display_name`, command, description, cwd).
    pub create_panel: Option<DeferredPanel>,
    /// If true, the spine notification is created already processed (no auto-continuation).
    /// Used for success notifications that don't need attention.
    pub processed_already: bool,
    /// If set, kill and remove this console session after processing.
    /// Used by `easy_bash` inline path to clean up sessions that have no panel.
    pub kill_session: Option<String>,
    /// When `true`, the cleanup code does NOT break tempo for this watcher result.
    /// Used by blocking watchers whose resolution did not create or modify any panel
    /// (e.g., `easy_bash` inline path with short output).
    pub preserves_tempo: bool,
    /// If set, create a generic dynamic panel when this watcher fires.
    /// Unlike `create_panel` (console-specific), this works for any panel type.
    pub create_dyn_panel: Option<DynPanel>,
}

impl WatcherResult {
    /// Start a result carrying only a description; every other field defaults
    /// to its inert value (`None` / `false`). Chain the setters below to fill
    /// in the fields a given watcher outcome actually needs.
    #[must_use]
    pub fn new<S>(description: S) -> Self
    where
        S: Into<String>,
    {
        Self {
            description: description.into(),
            panel_id: None,
            tool_use_id: None,
            close_panel: false,
            create_panel: None,
            processed_already: false,
            kill_session: None,
            preserves_tempo: false,
            create_dyn_panel: None,
        }
    }

    /// Associate a panel ID with this result.
    #[must_use]
    pub fn panel_id<S>(mut self, id: S) -> Self
    where
        S: Into<String>,
    {
        self.panel_id = Some(id.into());
        self
    }

    /// Set the tool-use ID for sentinel replacement (blocking watchers).
    #[must_use]
    pub fn tool_use_id<S>(mut self, id: S) -> Self
    where
        S: Into<String>,
    {
        self.tool_use_id = Some(id.into());
        self
    }

    /// Set the tool-use ID directly from an `Option`, for callers whose source
    /// field is already `Option<String>` (e.g. console watchers). A `None`
    /// leaves the result without a sentinel target.
    #[must_use]
    pub fn tool_use_id_opt(mut self, id: Option<String>) -> Self {
        self.tool_use_id = id;
        self
    }

    /// Request the associated panel be auto-closed.
    #[must_use]
    pub const fn close_panel(mut self) -> Self {
        self.close_panel = true;
        self
    }

    /// Defer console-panel creation until this watcher fires.
    #[must_use]
    pub fn create_panel(mut self, panel: DeferredPanel) -> Self {
        self.create_panel = Some(panel);
        self
    }

    /// Create the spine notification already processed (no auto-continuation).
    #[must_use]
    pub const fn processed_already(mut self) -> Self {
        self.processed_already = true;
        self
    }

    /// Kill and remove this console session after processing.
    #[must_use]
    pub fn kill_session<S>(mut self, key: S) -> Self
    where
        S: Into<String>,
    {
        self.kill_session = Some(key.into());
        self
    }

    /// Mark that resolving this result does NOT break tempo.
    #[must_use]
    pub const fn preserves_tempo(mut self) -> Self {
        self.preserves_tempo = true;
        self
    }

    /// Attach a generic dynamic panel to create when this watcher fires.
    #[must_use]
    pub fn create_dyn_panel(mut self, panel: DynPanel) -> Self {
        self.create_dyn_panel = Some(panel);
        self
    }
}

/// Info needed to create a console panel after a watcher fires.
#[derive(Debug)]
pub struct DeferredPanel {
    /// Console session key for reconnection.
    pub session_key: String,
    /// Human-readable name for the panel tab.
    pub display_name: String,
    /// Shell command that was executed.
    pub command: String,
    /// Short description for the panel header.
    pub description: String,
    /// Working directory (None = project root).
    pub cwd: Option<String>,
    /// ID of the callback that created this panel.
    pub callback_id: String,
    /// Display name of the callback.
    pub callback_name: String,
}

impl DeferredPanel {
    /// Start a deferred console-panel spec from the fields every caller sets:
    /// session key, display name, command, and header description. `cwd`
    /// defaults to project root (`None`) and the callback fields to empty;
    /// use the builder setters for the callback-originated variant.
    #[must_use]
    pub fn new<K, N, C, D>(session_key: K, display_name: N, command: C, description: D) -> Self
    where
        K: Into<String>,
        N: Into<String>,
        C: Into<String>,
        D: Into<String>,
    {
        Self {
            session_key: session_key.into(),
            display_name: display_name.into(),
            command: command.into(),
            description: description.into(),
            cwd: None,
            callback_id: String::new(),
            callback_name: String::new(),
        }
    }

    /// Set the working directory (`None` = project root) (builder).
    #[must_use]
    pub fn cwd(mut self, cwd: Option<String>) -> Self {
        self.cwd = cwd;
        self
    }

    /// Tag the panel with the callback that created it (builder).
    #[must_use]
    pub fn callback<I, N>(mut self, id: I, name: N) -> Self
    where
        I: Into<String>,
        N: Into<String>,
    {
        self.callback_id = id.into();
        self.callback_name = name.into();
        self
    }
}

/// Info needed to create a generic dynamic panel when a watcher fires.
///
/// Unlike [`DeferredPanel`] (console-specific), this works for any panel type
/// (brave results, firecrawl results, search results, etc.).
/// Used by async tool execution to create panels after HTTP/subprocess completion.
#[derive(Debug)]
pub struct DynPanel {
    /// Context type string (e.g., `"brave_result"`, `"firecrawl_result"`).
    pub context_type: String,
    /// Human-readable panel title.
    pub display_name: String,
    /// Key-value metadata to set via `Entry::set_meta`.
    pub metadata: Vec<(String, String)>,
    /// Panel content to set as `cached_content` immediately.
    /// When set, the panel displays content without waiting for a cache restore cycle.
    pub content: Option<String>,
}

impl DynPanel {
    /// Start a dynamic-panel spec with its context type and title; metadata
    /// empty and content absent until the builder setters fill them.
    #[must_use]
    pub fn new<C, D>(context_type: C, display_name: D) -> Self
    where
        C: Into<String>,
        D: Into<String>,
    {
        Self {
            context_type: context_type.into(),
            display_name: display_name.into(),
            metadata: Vec::new(),
            content: None,
        }
    }

    /// Attach the key-value metadata set via `Entry::set_meta` (builder).
    #[must_use]
    pub fn metadata(mut self, metadata: Vec<(String, String)>) -> Self {
        self.metadata = metadata;
        self
    }

    /// Set the immediate `cached_content` for the panel (builder).
    #[must_use]
    pub fn content<S>(mut self, content: S) -> Self
    where
        S: Into<String>,
    {
        self.content = Some(content.into());
        self
    }
}
