//! Backend → agent **read-path** query envelope (T671).
//!
//! A [`Query`] is a read-only request the orchestrator sends an agent to
//! interrogate its in-process state (currently: a hybrid keyword+semantic
//! search over the agent's own conversation index in Meilisearch). Unlike a
//! [`Command`](crate::types::command::Command) it never mutates anything and is
//! never journaled — the reply carries data, not a durable [`Ack`].
//!
//! ## Multiplexing on the command socket
//!
//! The agent listens on a single command UDS. To carry queries on that same
//! socket without disturbing the proven command path, the on-wire discriminator
//! is [`Inbound`] — an **untagged** union tried `Query`-first, then `Command`.
//! The two frame shapes are disjoint by *required* field: a command frame has a
//! required `command`, a query frame has a required `query`, and neither field
//! is optional. So a bare command frame fails the `Query` arm (no `query` key)
//! and falls through to `Command`, while a query frame fails the `Command` arm
//! (no `command` key). This keeps the command frame **byte-identical** to
//! pre-T671 — the orchestrator still writes a bare
//! [`command::Frame`](crate::types::command::Frame); only the agent's decode
//! wraps in [`Inbound`] to tell the two apart.

use serde::{Deserialize, Serialize};

use crate::types::command::Frame as CommandFrame;

/// A single read-only query from the backend to an agent.
///
/// Authn is handled by the enclosing [`Frame`] (bearer `auth`), mirroring the
/// command path — the credential lives outside this struct so a receiver checks
/// the bearer before trusting the payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Query {
    /// Wire-schema revision for this struct (always `1` today).
    pub schema_version: u32,

    /// Transport-level identifier, echoed back in [`Response::query_id`]
    /// so a caller can match a reply to its request.
    pub id: String,

    /// What the query asks the agent to look up.
    pub kind: Kind,
}

impl Query {
    /// Build a query, stamping the current `schema_version`.
    ///
    /// A constructor keeps [`Query`] `#[non_exhaustive]` across the orchestrator
    /// (which builds queries cross-crate); the schema revision is filled in here
    /// rather than at every call site.
    #[must_use]
    pub const fn new(id: String, kind: Kind) -> Self {
        Self { schema_version: 1, id, kind }
    }
}

/// The lookup a [`Query`] requests.
///
/// Internally-tagged (`"kind"` field) with an [`Unknown`](Kind::Unknown)
/// catch-all, so an N-1 receiver folds a variant it has never seen to `Unknown`
/// instead of failing hard — the same forward-compat contract as
/// [`command::Kind`](crate::types::command::Kind).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
#[expect(
    clippy::exhaustive_enums,
    reason = "wire-protocol contract: query Kind carries an Unknown catch-all for N-1 tolerance; its variant set is otherwise closed and constructed cross-crate (orchestrator builds queries, agent matches them exhaustively), so #[non_exhaustive] would forbid that construction"
)]
pub enum Kind {
    /// Hybrid (keyword + semantic) search over the agent's conversation index.
    #[serde(rename = "search_conversations")]
    SearchConversations {
        /// Free-text query — Meilisearch embeds it server-side, so no separate
        /// semantic query string is needed.
        query: String,
        /// Maximum number of hits to return.
        limit: u32,
        /// Optional single-thread scope (filters on `thread_id`). `None`
        /// searches every thread.
        thread_id: Option<String>,
    },

    /// Catch-all for variants added in a newer protocol version.
    ///
    /// An N-1 receiver deserialises any unrecognised `"kind"` tag here rather
    /// than returning a hard error.
    #[serde(other)]
    Unknown,
}

/// Transport envelope carrying a [`Query`] plus its bearer credential.
///
/// Mirrors [`command::Frame`](crate::types::command::Frame): the bearer lives
/// outside the [`Query`] so the agent checks it before trusting the payload.
/// The required `query` field is what distinguishes this from a command frame
/// under the [`Inbound`] untagged discriminator.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Frame {
    /// Wire-schema revision for this struct.
    pub schema_version: u32,

    /// Bearer credential — the agent's `cap_token`. An empty string is a
    /// missing bearer and is always rejected.
    pub auth: String,

    /// The query to run once the bearer is verified.
    pub query: Query,
}

impl Frame {
    /// Wrap a [`Query`] with its bearer credential.
    ///
    /// A constructor keeps [`Frame`] `#[non_exhaustive]` across the orchestrator
    /// (which builds query frames); `schema_version` is stamped here.
    #[must_use]
    pub const fn new(auth: String, query: Query) -> Self {
        Self { schema_version: 1, auth, query }
    }
}

/// Borrowed parts for building one [`ConvHit`].
///
/// A single params struct (rather than six positional arguments to
/// [`ConvHit::from_parts`]) keeps the cross-crate builder call-site readable and
/// dodges `too_many_arguments`. Deliberately **exhaustive** so the agent crate
/// can build it by struct literal — a constructor would itself take six
/// arguments and re-trip the very lint this struct exists to avoid. All fields
/// are borrowed; the builder owns copies only for what it stores.
#[derive(Debug)]
#[expect(
    clippy::exhaustive_structs,
    reason = "borrowed builder params for ConvHit: built by struct literal cross-crate (the agent maps a Meilisearch row into it); a constructor would take six positional arguments and re-trip too_many_arguments, which is exactly what this params struct exists to avoid, so #[non_exhaustive] is impossible"
)]
pub struct HitParts<'src> {
    /// Owning thread id — the navigation target.
    pub thread_id: &'src str,
    /// Owning thread's display name.
    pub thread_name: &'src str,
    /// `"user"` or `"assistant"`.
    pub author: &'src str,
    /// The matched message body.
    pub text: &'src str,
    /// Message timestamp, ms since epoch.
    pub ts_ms: u64,
    /// Meilisearch ranking score (0.0–1.0) — higher is a better match.
    pub score: f64,
}

/// One conversation-search hit returned to the backend.
///
/// A flattened, transport-ready view of a Meilisearch conversation document:
/// enough to render a palette result row (thread name + author + snippet) and
/// to navigate to the source thread (`thread_id`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[non_exhaustive]
pub struct ConvHit {
    /// Owning thread id — the navigation target.
    pub thread_id: String,
    /// Owning thread's display name.
    pub thread_name: String,
    /// `"user"` or `"assistant"`.
    pub author: String,
    /// The matched message body.
    pub text: String,
    /// Message timestamp, ms since epoch.
    pub ts_ms: u64,
    /// Meilisearch ranking score (0.0–1.0) — higher is a better match.
    pub score: f64,
}

impl ConvHit {
    /// Build a hit from its borrowed [`HitParts`].
    ///
    /// Taking a params struct (not six positional arguments) keeps the
    /// cross-crate builder within the argument-count budget while [`ConvHit`]
    /// stays `#[non_exhaustive]`.
    #[must_use]
    pub fn from_parts(p: &HitParts<'_>) -> Self {
        Self {
            thread_id: p.thread_id.to_owned(),
            thread_name: p.thread_name.to_owned(),
            author: p.author.to_owned(),
            text: p.text.to_owned(),
            ts_ms: p.ts_ms,
            score: p.score,
        }
    }
}

/// The outcome of a [`Query`], returned on the same stream as the request.
///
/// Internally-tagged so a `Hits` reply and an `Error` reply are distinguished
/// by a `result` discriminant. `Error` carries a human-readable reason (e.g.
/// the search backend was unreachable) rather than tearing down the connection,
/// so the caller can surface a graceful "search unavailable" state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "result")]
#[expect(
    clippy::exhaustive_enums,
    reason = "wire-protocol contract: query Outcome is a closed reply taxonomy (hits or error) constructed by the agent and matched exhaustively by the orchestrator; a new arm is a deliberate breaking change, and #[non_exhaustive] would force cross-crate wildcard arms the forbidden wildcard_enum_match_arm lint rejects"
)]
pub enum Outcome {
    /// The query succeeded; `hits` may be empty (no match).
    #[serde(rename = "hits")]
    Hits {
        /// Matching conversation documents, best-scored first.
        hits: Vec<ConvHit>,
    },

    /// The query failed; `reason` explains why (backend down, bad request).
    #[serde(rename = "error")]
    Error {
        /// Human-readable failure cause.
        reason: String,
    },
}

/// The reply to a [`Query`], carrying its result payload.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Response {
    /// Wire-schema revision for this struct.
    pub schema_version: u32,

    /// Echoes [`Query::id`] so a caller matches this reply to its request.
    pub query_id: String,

    /// The query outcome — hits or an error.
    pub result: Outcome,
}

impl Response {
    /// Build a response, stamping the current `schema_version`.
    #[must_use]
    pub const fn new(query_id: String, result: Outcome) -> Self {
        Self { schema_version: 1, query_id, result }
    }
}

/// On-wire discriminator multiplexing commands and queries on the one command
/// socket (T671).
///
/// Untagged and tried `Query`-first: a frame with a required `query` field
/// decodes as [`Query`](Inbound::Query); a frame with a required `command`
/// field falls through to [`Command`](Inbound::Command). The two required
/// fields are disjoint, so the match is unambiguous and a pre-T671 bare command
/// frame still decodes as `Command` (no `query` key) — the command path stays
/// byte-identical.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
#[expect(
    clippy::exhaustive_enums,
    reason = "closed 2-variant on-wire discriminator: an inbound frame is either a query or a command, matched exhaustively by the agent's intake; a third inbound kind is a deliberate protocol change, and #[non_exhaustive] would force a cross-crate wildcard arm the forbidden wildcard_enum_match_arm lint rejects"
)]
pub enum Inbound {
    /// A read-path query frame (has a required `query`).
    Query(Frame),
    /// A command frame (has a required `command`) — the existing path.
    Command(CommandFrame),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::command::{Command, Frame as CmdFrame, Kind as CmdKind};

    fn sample_query_frame() -> Frame {
        Frame::new(
            "tok".to_owned(),
            Query::new(
                "q-1".to_owned(),
                Kind::SearchConversations { query: "auth bug".to_owned(), limit: 10, thread_id: None },
            ),
        )
    }

    #[test]
    fn query_frame_round_trip() {
        let frame = sample_query_frame();
        let json = serde_json::to_string(&frame).expect("serialize");
        let back: Frame = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(frame, back);
    }

    #[test]
    fn query_response_round_trip() {
        let resp = Response::new(
            "q-1".to_owned(),
            Outcome::Hits {
                hits: vec![ConvHit::from_parts(&HitParts {
                    thread_id: "T3",
                    thread_name: "Auth",
                    author: "user",
                    text: "the token check",
                    ts_ms: 42,
                    score: 0.87,
                })],
            },
        );
        let json = serde_json::to_string(&resp).expect("serialize");
        let back: Response = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(resp, back);
    }

    #[test]
    fn unknown_query_kind_tolerant_decode() {
        let json = r#"{"schema_version":1,"id":"q","kind":{"kind":"future_lookup","x":1}}"#;
        let q: Query = serde_json::from_str(json).expect("tolerant decode");
        assert_eq!(q.kind, Kind::Unknown);
    }

    #[test]
    fn inbound_discriminates_query_frame() {
        // A query frame's bytes decode to Inbound::Query, never Command.
        let json = serde_json::to_string(&sample_query_frame()).expect("serialize");
        let inbound: Inbound = serde_json::from_str(&json).expect("decode");
        assert!(matches!(inbound, Inbound::Query(_)), "query frame must decode as Query");
    }

    #[test]
    fn inbound_discriminates_bare_command_frame() {
        // A pre-T671 bare command frame (required `command`, no `query`) falls
        // through the untagged union to Inbound::Command — byte-compat proof.
        let cmd = CmdFrame::new("tok".to_owned(), Command::new("c-1".to_owned(), 1, "dt".to_owned(), CmdKind::Stop));
        let json = serde_json::to_string(&cmd).expect("serialize");
        let inbound: Inbound = serde_json::from_str(&json).expect("decode");
        assert!(matches!(inbound, Inbound::Command(_)), "command frame must decode as Command");
    }
}
