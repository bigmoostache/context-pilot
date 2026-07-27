# Conversation Search (T671)

Hybrid keyword + semantic search over everything ever said in a thread, driven
from the thread-search palette. The read path travels
`frontend → orchestrator → agent → Meilisearch`, matching the user's chosen
transport (Option B: a full agent round-trip, not an orchestrator-direct read).

## Why the agent owns the search

The agent, not the orchestrator, owns its Meilisearch instance — the port,
master key and project hash are process-local. So a query is answered where that
knowledge lives, and only the hits cross the wire. This keeps the orchestrator
free of any Meilisearch coupling and lets the "local" leg later carry non-Meili
signals without a protocol change.

## Data model — the `cp_{hash}_conversations` index

One document per user/assistant thread message:

```
{ id: "{thread_id}-{index}", thread_id, thread_name, author, text, ts_ms, content_hash }
```

- `id` is stable per `(thread, position)`, so re-running the reconcile on an
  append-only thread upserts in place instead of duplicating.
- `searchableAttributes`: `text`, `thread_name`. `filterableAttributes`:
  `thread_id` (per-thread scope), `author`. `sortableAttributes`: `ts_ms`.
- Embedder: the existing **voyage-code-3** (reused, no new embedder), template
  `[{{author}}] {{thread_name}}: {{text | truncatewords: 100}}`, so the vector
  captures who said what in which thread.
- `content_hash` folds `author + thread_name + text` — everything the embedder
  template reads — so a rename or edit flips the hash and triggers a re-embed,
  while an unchanged message hashes identically and is skipped (zero Voyage
  cost).

`auto` (tool-trace) messages and empty-content messages are never indexed.

## Indexing — a periodic batch reconciler

Indexing is a **reconcile**, not an on-save write (the user's choice): it runs at
boot and after every turn (`on_stream_stop`), off the main loop via the existing
indexer command channel (`IndexerCmd::ReconcileConversations`).

Each pass diffs the desired doc set (built from the live `ThreadsState`) against
the index's `id → content_hash` snapshot:

- **New / hash-changed** → embed + upsert. Immutable messages embed exactly once.
- **Vanished ids** → deleted (edited-away messages, regenerated threads, and
  **deleted threads** — their messages simply stop appearing in the snapshot).

Lifecycle contract (locked with the user): Meilisearch stores **no**
archived/deleted flag. A **deleted** thread is absent from the snapshot, so its
docs are purged. An **archived** thread is still present, so its docs stay fully
searchable. No special-casing.

## Transport — multiplexing a read path onto the command socket

The agent listens on one command UDS. Queries ride that same socket without
disturbing the proven command path via the untagged `Inbound` discriminator
(tried `Query`-first, then `Command`): a command frame has a required `command`
field, a query frame a required `query` field, and the two are disjoint. So a
pre-T671 bare command frame still decodes as `Command` — the command path stays
**byte-identical**.

Wire types (cp-wire `types/payload/query.rs`):

- `Query { id, kind }`, `Kind::SearchConversations { query, limit, thread_id }`
  (internally tagged, `Unknown` catch-all for N-1 forward-compat).
- `Response { query_id, result }`, `Outcome::Hits { Vec<ConvHit> }` /
  `Outcome::Error { reason }`.
- `ConvHit { thread_id, thread_name, author, text, ts_ms, score }`.

A query is **not journaled** and assigns no `rev`: it mutates nothing, so
replaying it is meaningless and a duplicate delivery is harmless. It shares only
the bearer `cap_token` check with the command path.

## Read path end to end

1. Palette `POST /api/agent/{id}/conversations/search` with
   `{ query, limit?, thread_id? }`.
2. Orchestrator `AgentChannel::query()` writes a `QueryFrame` over the agent's
   UDS (same length+CRC framing + `cap_token` as `send()`), reads the framed
   `Response`.
3. The agent's bridge intake decodes `Inbound::Query`, checks the bearer, and
   calls an **injected responder closure**. The closure is built with the
   Meilisearch credentials resolved *before* the command-path `Intake` is
   `&mut`-borrowed, so answering a query needs no second `State` borrow.
4. The responder runs the hybrid search (`semanticRatio 0.5`) and maps hits.
   Meilisearch embeds the raw query server-side, so **no fabricated semantic
   query** is needed — the user just types.

A failed lookup (search module down, backend unreachable) comes back as a
graceful `Outcome::Error`, surfaced as `502` with the agent's own reason, so the
UI can say *why* search is unavailable rather than showing an empty result set.

## Frontend — the palette

`ThreadSearchPalette` runs two modes off one input:

- Below 2 chars: the instant client-side thread filter (name + last-message
  preview).
- At/above 2 chars: a debounced (220ms) hybrid search via TanStack `useQuery`,
  listing message-level hits. `placeholderData` keeps prior hits so the list
  never flashes empty between keystrokes.

Both sources normalize into one `Row[]`, so keyboard nav (↑/↓/⏎) and rendering
stay a single path regardless of which source is showing.

## File map

- Index + embedder: `cp-mod-search/src/types.rs` (`conversations_index_settings`),
  `meili/bootstrap.rs` (`conversations_embedder_settings`), `meili/api.rs`
  (`delete_documents` batch).
- Reconciler: `cp-mod-search/src/index/reconcile/conversations.rs`; wiring in
  `lib.rs` (`build_conversation_docs` / `queue_conversation_reconcile`) +
  `index/indexer.rs`.
- Wire types: `cp-wire/src/types/payload/query.rs`.
- Agent responder: `cp-mod-bridge/src/command/reply.rs`;
  `src/app/run/threads/query.rs` (closure) + `bridge.rs` (credential
  pre-resolve).
- Stateless search entrypoint: `cp-mod-search/src/meili/conversation_search.rs`.
- Orchestrator: `cp-orchestrator/src/registry/channel.rs` (`query()`),
  `transport/rest/threads/conversations.rs` (REST handler).
- Frontend: `web/src/lib/api/index.ts` (`searchConversations`),
  `web/src/components/threads/dialogs/ThreadSearchPalette.tsx`.
