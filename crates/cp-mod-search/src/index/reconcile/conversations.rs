//! Conversation (thread-message) ⇄ index reconciliation (T671).
//!
//! The thread-search palette hybrid-searches everything ever said in a thread,
//! so every user/assistant thread message is mirrored as one Meilisearch
//! document in `cp_{hash}_conversations`. Unlike the file indexer (event-driven
//! off an fs watcher), thread messages have no filesystem to watch — they live
//! in the agent's in-memory `ThreadsState`. So this runs as a periodic + boot
//! **batch reconcile**: the caller snapshots the live threads into a
//! [`ConversationDoc`] list, and this module diffs that desired set against
//! what's already indexed, embedding only what changed and purging what's gone.
//!
//! Design contract (locked with the user):
//! - **Re-embed only changed messages.** Each doc carries a `content_hash`; a
//!   doc whose hash still matches the index is left untouched (no Voyage call).
//! - **Purge deprecated / deleted.** Any indexed id absent from the desired set
//!   is deleted — this covers edited-away messages, regenerated threads, and
//!   **deleted threads** (their messages simply stop appearing in the snapshot).
//! - **No archived/deleted bookkeeping in Meili.** An *archived* thread is still
//!   present in the snapshot, so its docs stay searchable; a *deleted* thread is
//!   absent, so its docs are purged. Tool-trace (`auto`) and empty messages are
//!   never handed to us by the builder, so they never enter the index.

use serde::Serialize;

use crate::meili::api::MeiliClient;
use crate::meili::tasks;

/// One indexed thread message.
///
/// The `id` is stable per `(thread, position)` so re-running the reconcile on an
/// append-only thread upserts in place rather than duplicating. `content_hash`
/// folds in everything the embedding template reads (author, thread name, text)
/// so a rename or edit flips the hash and triggers a re-embed, while an
/// unchanged message hashes identically and is skipped.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct ConversationDoc {
    /// Meilisearch primary key — `"{thread_id}-{index}"`.
    pub id: String,
    /// Owning thread id (filterable — lets a search scope to one thread).
    pub thread_id: String,
    /// Owning thread's display name (searchable + embedded for context).
    pub thread_name: String,
    /// `"user"` or `"assistant"` (filterable).
    pub author: String,
    /// The message body (searchable + embedded).
    pub text: String,
    /// Message timestamp, ms since epoch (sortable — recency tie-break).
    pub ts_ms: u64,
    /// Hash over `author` + `thread_name` + `text` — the re-embed trigger.
    pub content_hash: String,
}

/// Borrowed inputs for building one [`ConversationDoc`].
///
/// A single params struct (instead of six positional arguments) keeps the
/// builder call-site readable and dodges `too_many_arguments`. All fields are
/// borrowed — the builder owns copies only for the fields it stores.
pub(crate) struct DocParts<'src> {
    /// Owning thread id.
    pub thread_id: &'src str,
    /// Message position in the thread (stable for append-only threads).
    pub index: usize,
    /// Owning thread's display name.
    pub thread_name: &'src str,
    /// `"user"` or `"assistant"`.
    pub author: &'src str,
    /// The message body.
    pub text: &'src str,
    /// Message timestamp, ms since epoch.
    pub ts_ms: u64,
}

impl ConversationDoc {
    /// Build a doc, computing its `content_hash` from the embedding-relevant
    /// fields. The id is `"{thread_id}-{index}"` where `index` is the message's
    /// position in the thread (stable for append-only threads).
    pub(crate) fn from_parts(p: &DocParts<'_>) -> Self {
        // Everything the embedder template reads goes into the hash, so any
        // change that would alter the embedding (edit, rename, author) forces a
        // re-embed, and nothing else does. `\u{1}` is a field separator that
        // can't occur in the source text.
        let hash_input = format!("{}\u{1}{}\u{1}{}", p.author, p.thread_name, p.text);
        let content_hash = cp_mod_utilities::hash::compute_str(&hash_input);
        Self {
            id: format!("{}-{}", p.thread_id, p.index),
            thread_id: p.thread_id.to_owned(),
            thread_name: p.thread_name.to_owned(),
            author: p.author.to_owned(),
            text: p.text.to_owned(),
            ts_ms: p.ts_ms,
            content_hash,
        }
    }
}

/// The reconcile delta: which desired docs to (re)upsert, which indexed ids to
/// drop. Deterministic (ids sorted) so [`plan`] is unit-testable without a live
/// Meilisearch.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ConvPlan {
    /// Indices into the desired slice whose hash is new or changed.
    pub upsert: Vec<usize>,
    /// Indexed ids no longer present in the desired set.
    pub delete: Vec<String>,
}

impl ConvPlan {
    /// Whether the plan would change anything.
    pub(crate) const fn is_empty(&self) -> bool {
        self.upsert.is_empty() && self.delete.is_empty()
    }
}

/// Pure diff between the index's `id → content_hash` snapshot and the desired
/// docs. Upserts any desired doc whose hash is absent or differs; deletes any
/// indexed id not in the desired set. Sorted output for deterministic tests.
pub(crate) fn plan(existing: &std::collections::HashMap<String, String>, desired: &[ConversationDoc]) -> ConvPlan {
    let desired_ids: std::collections::HashSet<&str> = desired.iter().map(|d| d.id.as_str()).collect();

    // Indexed but no longer desired → delete (edited-away, regenerated, or
    // deleted-thread messages). Keys sorted for determinism.
    let mut delete: Vec<String> = existing.keys().filter(|id| !desired_ids.contains(id.as_str())).cloned().collect();
    delete.sort();

    // Desired but absent or hash-changed → (re)embed. Everything else skipped
    // (identical hash ⇒ zero Voyage cost).
    let mut upsert: Vec<usize> = Vec::new();
    for (i, d) in desired.iter().enumerate() {
        if existing.get(&d.id).is_none_or(|h| *h != d.content_hash) {
            upsert.push(i);
        }
    }
    upsert.sort_unstable();

    ConvPlan { upsert, delete }
}

/// Snapshot the conversations index as an `id → content_hash` map.
fn index_hashes(client: &MeiliClient, conv_uid: &str) -> Result<std::collections::HashMap<String, String>, String> {
    let rows = tasks::fetch_projection(client, conv_uid, &["id", "content_hash"])?;
    let mut map = std::collections::HashMap::new();
    for row in rows {
        let Some(id) = row.get("id").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let hash = row.get("content_hash").and_then(serde_json::Value::as_str).unwrap_or_default();
        let _prev = map.insert(id.to_owned(), hash.to_owned());
    }
    Ok(map)
}

/// Reconcile the conversations index against the desired doc set.
///
/// Fetches the current `id → hash` snapshot, computes the [`plan`], then applies
/// it: batch-upsert the changed docs (Meilisearch re-embeds exactly those via
/// the configured Voyage embedder) and batch-delete the vanished ids. Returns
/// `(upserted, deleted)` counts. A no-op plan issues zero HTTP writes.
///
/// # Errors
///
/// Returns an error if the snapshot fetch or either write task fails.
pub(crate) fn reconcile(
    client: &MeiliClient,
    conv_uid: &str,
    desired: &[ConversationDoc],
) -> Result<(usize, usize), String> {
    let existing = index_hashes(client, conv_uid)?;
    let plan = plan(&existing, desired);
    if plan.is_empty() {
        return Ok((0, 0));
    }

    if !plan.upsert.is_empty() {
        let docs: Vec<&ConversationDoc> = plan.upsert.iter().filter_map(|&i| desired.get(i)).collect();
        let body = serde_json::to_value(&docs).map_err(|e| format!("conversations serialize failed: {e}"))?;
        let task = client.add_documents(conv_uid, &body)?;
        tasks::wait_for_task(client, task)?;
    }
    if !plan.delete.is_empty() {
        let task = client.delete_documents(conv_uid, &plan.delete)?;
        tasks::wait_for_task(client, task)?;
    }

    Ok((plan.upsert.len(), plan.delete.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(thread: &str, idx: usize, text: &str) -> ConversationDoc {
        ConversationDoc::from_parts(&DocParts {
            thread_id: thread,
            index: idx,
            thread_name: "T",
            author: "user",
            text,
            ts_ms: 0,
        })
    }

    #[test]
    fn plan_all_new_when_index_empty() {
        let empty = std::collections::HashMap::new();
        let desired = vec![doc("T1", 0, "a"), doc("T1", 1, "b")];
        let p = plan(&empty, &desired);
        assert_eq!(p.upsert, vec![0, 1]);
        assert!(p.delete.is_empty());
    }

    #[test]
    fn plan_skips_unchanged_hash() {
        let d = doc("T1", 0, "a");
        let existing = std::collections::HashMap::from([(d.id.clone(), d.content_hash.clone())]);
        let p = plan(&existing, std::slice::from_ref(&d));
        assert!(p.is_empty(), "identical hash must be skipped");
    }

    #[test]
    fn plan_reupserts_changed_hash() {
        let old = doc("T1", 0, "a");
        let existing = std::collections::HashMap::from([(old.id.clone(), old.content_hash)]);
        let edited = doc("T1", 0, "a-edited"); // same id, new text ⇒ new hash
        let p = plan(&existing, std::slice::from_ref(&edited));
        assert_eq!(p.upsert, vec![0]);
        assert!(p.delete.is_empty());
    }

    #[test]
    fn plan_deletes_vanished_id() {
        // Index holds a message the desired set no longer has (thread deleted
        // or message edited away).
        let existing = std::collections::HashMap::from([("T9-0".to_owned(), "h".to_owned())]);
        let p = plan(&existing, &[]);
        assert_eq!(p.delete, vec!["T9-0".to_owned()]);
        assert!(p.upsert.is_empty());
    }

    #[test]
    fn plan_mixed_upsert_and_delete_sorted() {
        let keep = doc("T1", 0, "keep");
        let existing = std::collections::HashMap::from([
            (keep.id.clone(), keep.content_hash.clone()), // unchanged
            ("T1-9".to_owned(), "stale".to_owned()),      // vanished
            ("T2-0".to_owned(), "stale".to_owned()),      // vanished
        ]);
        let desired = vec![keep, doc("T1", 1, "new")]; // T1-1 new
        let p = plan(&existing, &desired);
        assert_eq!(p.upsert, vec![1]);
        assert_eq!(p.delete, vec!["T1-9".to_owned(), "T2-0".to_owned()]);
    }
}
