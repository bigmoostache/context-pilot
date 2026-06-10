//! Wire protocol — the two-faced contract between the Pi and the browser.
//!
//! Incoming face: [`WebCommand`] / [`WebQuery`] (browser → core, mapped to
//! `Action`s or explicit state mutations by the binary).
//! Outgoing face: pre-serialized JSON frames built by the binary's
//! `build_web_state()` mirror — this crate only routes them.
//!
//! See `docs/nestor-web-contract.md` for the full schema.

use serde::{Deserialize, Serialize};

/// A command emitted by the web frontend. Most map 1:1 onto `Action`s;
/// `AnswerQuestion`/`DismissQuestion` reproduce the TUI's direct state
/// mutations on the pending question form.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum WebCommand {
    /// Submit a message: the browser owns input editing and sends final text.
    Submit {
        /// The full message text to submit.
        text: String,
    },
    /// Interrupt the active LLM stream (web equivalent of Esc).
    Stop,
    /// Select a context panel by its display ID (e.g. `"P3"`).
    SelectPanel {
        /// Panel display ID.
        id: String,
    },
    /// Discard all messages and start fresh.
    ClearConversation,
    /// Create a new worker context.
    NewContext,
    /// Reset the session cost counters to zero.
    ResetCosts,
    /// Request a process reload (the binary re-execs itself).
    Reload,
    /// Select the LLM provider for a scope.
    SetProvider {
        /// `"primary"` or `"secondary"`.
        scope: ConfigScope,
        /// Provider ID (serde name of `LlmProvider`, e.g. `"anthropic"`).
        provider: String,
    },
    /// Select the model for the scope's current provider.
    SetModel {
        /// `"primary"` or `"secondary"`.
        scope: ConfigScope,
        /// Model ID (serde name of the provider's model enum).
        model: String,
    },
    /// Set the active theme directly.
    SetTheme {
        /// Theme ID (e.g. `"dnd"`, `"modern"`).
        theme: String,
    },
    /// Toggle spine auto-continuation.
    ToggleAutoContinue,
    /// Toggle the reverie background optimizer.
    ToggleReverie,
    /// Set the context budget in tokens (`null` = model's full window).
    SetContextBudget {
        /// Budget in tokens, or `None` to use the full context window.
        tokens: Option<usize>,
    },
    /// Set the auto-cleaning threshold (clamped 0.30–0.95).
    SetCleaningThreshold {
        /// New threshold value.
        value: f32,
    },
    /// Set the cleaning target proportion (clamped 0.30–0.95).
    SetCleaningTarget {
        /// New target value.
        value: f32,
    },
    /// Set the spine max-cost guard rail (`null` = disabled).
    SetMaxCost {
        /// Maximum session cost in USD, or `None` to disable.
        value: Option<f64>,
    },
    /// Set the think reminder threshold (capped at −1).
    SetThinkThreshold {
        /// New threshold (negative integer).
        value: i64,
    },
    /// Answer a pending `ask_user_question` form.
    AnswerQuestion {
        /// The `tool_use_id` the form was created for.
        tool_use_id: String,
        /// One entry per question, in order.
        answers: Vec<QuestionAnswerPayload>,
    },
    /// Dismiss a pending question form without answering.
    DismissQuestion {
        /// The `tool_use_id` the form was created for.
        tool_use_id: String,
    },
}

/// Which model/provider slot a config command targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigScope {
    /// Main conversation model.
    Primary,
    /// Reverie / sub-agent model.
    Secondary,
}

/// Answer payload for one question of a form.
#[derive(Debug, Clone, Deserialize)]
pub struct QuestionAnswerPayload {
    /// Selected option indices (0-based; single-select sends at most one).
    #[serde(default)]
    pub selected: Vec<usize>,
    /// Free text if the user picked "Other".
    #[serde(default)]
    pub other_text: Option<String>,
}

/// A read-only query from the web frontend, answered with a correlated
/// `{"t":"result","req_id":…}` frame.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "q", rename_all = "snake_case")]
pub enum WebQuery {
    /// Directory listing for the `@` autocomplete.
    ListDir {
        /// Directory path relative to the workspace root (empty = root).
        #[serde(default)]
        dir: String,
        /// Name prefix filter (empty = all entries).
        #[serde(default)]
        prefix: String,
    },
    /// Content of a (possibly non-selected) panel.
    PanelContent {
        /// Panel display ID.
        id: String,
    },
    /// Recent prompt history entries.
    PromptHistory {
        /// Maximum entries to return (most recent first).
        #[serde(default)]
        limit: Option<usize>,
    },
    /// Search-index status overlay text (web equivalent of Ctrl+I).
    IndexStatus,
}

/// Envelope for every message a client sends over the WebSocket.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ClientMsg {
    /// Fire-and-forget command.
    Cmd(WebCommand),
    /// Correlated read-only query.
    Query {
        /// Client-chosen correlation ID echoed back in the result frame.
        req_id: String,
        /// The query itself.
        #[serde(flatten)]
        query: WebQuery,
    },
    /// Application-level keep-alive (answered with `{"t":"pong"}`).
    Ping,
}

/// Events delivered from the web layer to the core loop (via `std::sync::mpsc`).
#[derive(Debug)]
pub enum WebEvent {
    /// A client issued a command.
    Command(WebCommand),
    /// A client issued a query; the answer must be addressed to `conn_id`.
    Query {
        /// Connection that asked (snapshot/result frames are targeted to it).
        conn_id: u64,
        /// Correlation ID to echo in the result frame.
        req_id: String,
        /// The query itself.
        query: WebQuery,
    },
    /// A new authenticated client connected and needs a snapshot.
    Connected {
        /// The new connection's ID.
        conn_id: u64,
    },
}

/// A pre-serialized JSON frame pushed from the core to web clients.
#[derive(Debug, Clone)]
pub struct WireFrame {
    /// `None` = broadcast to every client; `Some(id)` = only that connection.
    pub to: Option<u64>,
    /// The frame body (complete JSON object, e.g. `{"t":"delta",…}`).
    pub json: String,
}

impl WireFrame {
    /// Build a broadcast frame.
    #[must_use]
    pub const fn broadcast(json: String) -> Self {
        Self { to: None, json }
    }

    /// Build a frame addressed to a single connection.
    #[must_use]
    pub const fn to_conn(conn_id: u64, json: String) -> Self {
        Self { to: Some(conn_id), json }
    }
}

/// Serialize a `{"t":"result"}` frame for a query response.
#[must_use]
pub fn result_frame(req_id: &str, data: &serde_json::Value) -> String {
    serde_json::json!({ "t": "result", "req_id": req_id, "data": data }).to_string()
}

/// Serialize a `{"t":"error"}` frame.
#[must_use]
pub fn error_frame(message: &str) -> String {
    serde_json::json!({ "t": "error", "message": message }).to_string()
}

/// Login request body for `POST /api/login`.
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    /// The shared web password.
    pub password: String,
    /// Human-readable device name (e.g. `"PC Eloi — Firefox"`).
    #[serde(default)]
    pub device_name: String,
}

/// Login response body.
#[derive(Debug, Serialize)]
pub struct LoginResponse {
    /// The per-device session token (256-bit hex). Sent once, stored hashed.
    pub token: String,
    /// Server-side ID of the device entry (for revocation).
    pub device_id: String,
}
