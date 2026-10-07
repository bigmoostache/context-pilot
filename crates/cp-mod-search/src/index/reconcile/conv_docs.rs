//! Conversation-doc builder for the T671 conversations index.
//!
//! Extracted from `lib.rs` to keep that file under the 500-line cap. Builds the
//! desired-doc set the reconciler diffs against the Meilisearch conversations
//! index; the queueing side (`queue_conversation_reconcile`) stays in `lib.rs`.

use cp_base::state::runtime::State;

use crate::index;

/// Build the desired conversation-doc set from the live threads (T671).
///
/// One [`index::reconcile::conversations::ConversationDoc`] per non-`auto`,
/// non-empty thread message. `auto` (tool-trace) and empty-content messages are
/// dropped here so they never enter the index — matching the reconcile
/// contract. The doc `index` is the message's position in its thread, giving a
/// stable id (`"{thread_id}-{index}"`) for append-only threads. Archived threads
/// are still included (their docs stay searchable); a deleted thread simply
/// stops appearing, so the reconciler purges its docs.
pub(crate) fn build_conversation_docs(state: &State) -> Vec<index::reconcile::conversations::ConversationDoc> {
    use index::reconcile::conversations::{ConversationDoc, DocParts};

    let threads = &cp_mod_threads::types::ThreadsState::get(state).threads;
    let mut docs = Vec::new();
    for thread in threads {
        for (index, msg) in thread.messages.iter().enumerate() {
            if msg.auto {
                continue; // tool-trace — never indexed
            }
            let Some(text) = msg.content.as_deref() else {
                continue; // file-only / empty message — nothing to search
            };
            if text.is_empty() {
                continue;
            }
            let author = msg.author.to_string(); // "user" / "assistant"
            docs.push(ConversationDoc::from_parts(&DocParts {
                thread_id: &thread.id,
                index,
                thread_name: &thread.name,
                author: &author,
                text,
                ts_ms: msg.timestamp,
            }));
        }
    }
    docs
}
