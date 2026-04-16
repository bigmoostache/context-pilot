# Memory Architecture v2 — Design Document

> Living document. Each section records a design question, the options considered,
> and the captain's decision. Built iteratively through Q&A rounds.

---

## Round 1: Foundations

### Q1: How should conversation history survive across sessions?
**Options**: Full archive on disk / Summarize then discard / Selective extraction only
**Decision**: **Hybrid approach** — Mix of Mem0-style selective extraction + knowledge graph building +
summarization. Full conversation may be kept short-term, but long-term it shall never be reloaded
as raw conversation. The goal is to extract durable knowledge, not preserve raw chat logs.

### Q2: Should memories always be in-context or retrieved on demand?
**Decision**: **Dynamic + pinned + searchable** — Context Pilot itself decides what to load (dynamic),
some memories are pinned (always present), and the AI can search for more. But the captain flagged
that we need to think about **memory structure first** before deciding loading strategy:
*What do memories look like? What's their lifecycle? How and when are they loaded?*

### Q3: Who decides what to remember?
**Options**: AI-driven / System-driven / Both
**Decision**: **Both — AI hot path + system cold path**. The AI stores critical things in real-time
during conversation. A background reverie agent consolidates and promotes patterns after conversation
ends or during idle time.

### Q4: What retrieval mechanism?
**Options**: BM25 / BM25 + LLM reranking / Full vector search
**Decision**: **Full vector search (embeddings)** with rerankers for precision.
Need to research: which embedding model, which reranker, infrastructure implications.

---

## Round 2: Memory Structure & Lifecycle

### Q5: What types of memories should exist?
**Options**: Semantic facts / Episodic records / Preference profiles / Procedural knowledge
**Decision**: **All four types**:
- **Semantic facts**: Standalone extracted knowledge ('MiniMax uses Anthropic API at api.minimax.io')
- **Episodic records**: Time-bound session records capturing reasoning + outcome
- **Preference profiles**: Curated, evolving documents of user/project conventions
- **Procedural knowledge**: Learned patterns — HOW to solve, reusable across sessions

### Q6: What should a single memory item look like?
**Options**: Atomic facts / Themed documents / Graph nodes + edges
**Decision**: **Graph nodes + edges** — Memories as graph nodes linked via typed relations.
Rich structure: 'MiniMax' →uses→ 'Anthropic API', 'User' →prefers→ '#[expect]'.

### Q7: What is the lifecycle of a memory?
**Options**: Permanent / Active decay + reinforcement / TTL-based tiers
**Decision**: **Active decay + reinforcement** — Ebbinghaus-inspired. Memories weaken over time.
Frequently recalled ones stay strong. Stale ones fade to archive/deletion. Self-cleaning system.

---

## Round 3: Graph Structure & Decay Mechanics

### Q8: Node schema
**Options**: Unified / Type-specific / Minimal nodes + rich edges
**Decision**: **Unified schema** — All nodes share one schema: content, type label (semantic/episodic/
preference/procedural), importance, timestamps, decay score. Differentiation via `type` field only.

### Q9: Edge types
**Options**: Free-form labels / Typed enum / Enum + free-form fallback
**Decision**: **Typed enum** — Fixed set of relation types: RELATES_TO, CAUSED_BY, PREFERS,
CONTRADICTS, SUPERSEDES, LEARNED_FROM, PART_OF, etc. Structured and queryable.

### Q10: Decay model
**Options**: Ebbinghaus + recall boost / Weighted multi-factor / Tiered TTL
**Decision**: **Ebbinghaus + recall boost** — `score = importance × e^(−λ × days) × (1 + recall_count × 0.2)`.
Higher importance = slower decay. Each recall boosts score. Below threshold → archive/delete.

### Q11: When does LLM get involved?
**Captain's key insight**: LLM should only fire at **write time** (memory creation/update), never at read time.
- **Write path (LLM needed)**: Extract facts from conversation → create nodes → resolve conflicts → consolidate
- **Read path (NO LLM)**: Vector similarity search + decay math + graph traversal + score ranking
- **Analogy**: LLM creates the "memory anchor" once. All subsequent reads are pure computation.
  This means retrieval is instant, free, and doesn't depend on model availability.

### Q12: Decay trigger
**Decision**: Pending — need to determine if background sweep or on-access evaluation makes more sense
given the "no LLM at read time" constraint. Both are pure computation, so cost is negligible either way.

---

## Round 4: Embeddings, Storage & Compaction

### Q13: Where should embeddings be computed?
**Options**: Local ONNX / External API / Local + optional API override
**Decision**: **Local ONNX model** — Self-contained, ships with the binary. Zero API cost, works offline.
Small model (e.g., all-MiniLM-L6-v2, 384 dims, ~22M params, ~50ms/embedding on CPU).
Rust inference via `ort` (ONNX Runtime) or `candle`.

### Q14: Where should the memory graph be stored?
**Captain's constraint**: Must run **in-process in Rust** — no separate database process. A background
thread within the same binary, like the console server model.
**Decision**: Pending — need to determine: SQLite+sqlite-vec (in-process, single file) vs. pure-Rust
flat files with in-memory index rebuilt on startup vs. sled + custom index.

### Q15: How should in-conversation compaction work?
**Captain's direction**: Keep close to current conversation history panel system. Don't throw it away.
But change HOW panels get closed/archived — not manual reverie cleanup, something smarter.
**Also wanted**: Tool result collapsing as a NEW mechanism alongside the panel system.
**Decision**: Pending — need to design the new panel lifecycle and tool result collapse.

---

## Round 5: Storage Engine & Panel Lifecycle

### Q16: Storage engine
**Options**: SQLite + sqlite-vec / JSON files + in-memory index / sled
**Decision**: **SQLite + sqlite-vec** — Single .db file in .context-pilot/. In-process via rusqlite.
sqlite-vec extension for KNN vector search. ACID, battle-tested, full SQL for graph queries.

### Q17: How should conversation history panels get archived?
**Captain's architecture** — Two-phase system, layered on existing detach logic:
1. **On detach**: When a conversation history panel detaches (existing logic, don't touch), a
   **per-panel reverie** launches automatically. Its job: extract memories into the graph.
2. **Panel lingers**: The detached panel doesn't close immediately after memory extraction.
   It stays in detached state, still accessible but not actively consuming prime context.
3. **Auto-close**: The panel eventually auto-closes based on existing context fillage logic.
   By this point, all valuable knowledge has already been extracted into the memory graph.
Key: The current detach mechanism stays untouched. Memory extraction is a new layer ON TOP.

### Q18: How should tool result collapsing work?
**Captain's design**: Each tool result provides **two versions**:
- **Full result**: The current verbose output (file contents, build logs, search results)
- **TLDR result**: A compact summary, defined by the tool itself. Each tool controls its own
  compact representation because the tool knows best what matters.
When context grows, old tool results swap from full → TLDR. No LLM needed — just version swap.

---

## Round 6: Memory Extraction & Tool TLDR Design

### Q19: Which LLM for memory extraction reverie?
**Options**: Same model / Cheap secondary / Reverie model
**Decision**: **Same model as conversation** — Simplest approach, no extra infrastructure.
The extraction reverie uses whatever model is active when the panel detaches.

### Q20: How should the extraction reverie produce memory nodes?
**Options**: Structured JSON / Tool calls (agent-style) / Natural language → parser
**Decision**: **Structured JSON via a dedicated tool call**. The extraction reverie gets ONE tool
(e.g., `memory_extract`) — all other tools are forbidden for this reverie.
The tool accepts structured JSON: `[{content, type, importance, relations}]` → direct graph insertions.
**Note**: Reverie context building will need changes to support this extraction-focused mode.

### Q21: How should tool TLDRs be generated?
**Options**: Dual-return from tool / Lazy compaction function / System heuristic
**Decision**: **Dual-return from tool** — Each tool's `handle_tool_call` returns both `full_result`
and `tldr_result`. Tool author writes both versions. TLDR is stored alongside the full result from
the start. When compaction triggers, old tool results swap full → TLDR with zero computation.

---

## Round 7: Memory Retrieval & Context Loading

### Q22: How should the AI access stored memories?
**Options**: AI-triggered search / Auto-inject per turn / Hybrid
**Decision**: **Hybrid: auto-inject + explicit search** — with a novel "Memory Views" concept:
- **Memory View**: A configured hybrid search window into the knowledge graph. Has parameters
  (topics, keywords, graph traversal rules, similarity thresholds) that determine what slice
  of knowledge it shows. Multiple views can be open simultaneously.
- **Auto-refresh**: Views refresh at minimum per user turn. For long multi-tool turns,
  a reverie may reconfigure views mid-turn based on evolving context.
- **Explicit search**: AI can also search or create/modify views via tool calls.
- **Refresh mechanism**: A reverie that re-"configures" memory views, determining search
  parameters for the hybrid KB search (graph, embeddings, keywords, topics, etc.)

### Q23: Pinning model
**Captain's concept**: **Memory views replace traditional pinning.** Instead of individual pinned
memories, you open memory views with different configurations. A "pinned" memory is just one
that appears in a persistent view configured to always show critical items. Multiple views
can be open simultaneously, giving different angles into the graph.

### Q24: Memory panel design
**Decision**: **Single overview panel** showing memory system status + pinned/critical items.
Plus: **multiple Memory View panels** open alongside it, each showing a different search
configuration's results. Views are context panels that consume context space.

---

## Round 8: Memory View Architecture

### Q25: What search dimensions should a Memory View support?
**Options**: Embedding similarity / Keyword (BM25) / Graph traversal / Metadata filters
**Decision**: **All four** — each view can configure any combination:
- **Embedding similarity**: Vector search against query string or conversation topic
- **Keyword (BM25)**: Exact term matching — file paths, function names, error codes
- **Graph traversal**: Follow edges from anchor nodes — "everything connected to X"
- **Metadata filters**: Memory type, importance range, date range, tags

### Q26: Who manages Memory Views?
**Options**: AI-managed / System-managed / System default + AI customization
**Decision**: **System default + AI customization** — System auto-creates a default view
(based on conversation context). AI can create additional views, reconfigure, or close them
via tool calls. Best of both: zero-config out of the box, full control when needed.

### Q27: How often should views refresh?
**Options**: Every user turn / On topic drift / Per turn + reverie mid-turn
**Decision**: **Per turn + reverie mid-turn** — Views auto-refresh every user turn (cheap with
local embeddings + SQLite). During long autonomous runs with many tool calls, a reverie can
trigger additional mid-turn refreshes when it detects topic drift or new relevant context.

---

## Round 9: Migration Strategy & Current System Mapping

### Q28: What happens to existing memories (M1-M24)?
**Options**: Auto-migrate / Start fresh / Parallel operation
**Decision**: **Start fresh (clean slate)** — New graph starts empty. Old memories kept as legacy
but no longer primary. Clean break, no migration debt.

### Q29: What happens to the log system?
**Options**: Keep unchanged / Logs become episodic nodes / Replace with episodic records
**Decision**: **Replace logs with episodic records** — Logs are sunset entirely. The memory graph's
episodic records replace log functionality. Fewer concepts, one unified system.
Implication: the `cp-mod-logs` crate eventually gets replaced by memory graph episodic nodes.

### Q30: Build order
**Options**: Storage layer first / Tool TLDR first / Extraction first
**Decision**: **Storage layer first** — Build bottom-up:
1. Graph storage (SQLite + sqlite-vec) + embedding engine (ONNX)
2. Memory extraction (reverie + dedicated tool)
3. Retrieval (hybrid search: vector + BM25 + graph traversal + metadata)
4. Memory Views (configurable search windows + refresh)
5. Tool TLDR dual-return (can be done in parallel)
6. UI integration (overview panel + view panels)

---

## Summary of All Decisions

| # | Question | Decision |
|---|----------|----------|
| Q1 | History survival | Hybrid: extraction + graph + summarization. Raw conversations never reloaded long-term |
| Q2 | Memory loading | Dynamic (CP decides) + pinned + AI-searchable. Structure-first thinking |
| Q3 | Who decides to remember | Both: AI hot path + system cold path |
| Q4 | Retrieval mechanism | Full vector search (embeddings) with rerankers |
| Q5 | Memory types | All four: semantic, episodic, preference, procedural |
| Q6 | Memory item shape | Graph nodes + typed edges |
| Q7 | Memory lifecycle | Active decay + reinforcement (Ebbinghaus) |
| Q8 | Node schema | Unified: content, type, importance, timestamps, decay score |
| Q9 | Edge types | Typed enum (RELATES_TO, CAUSED_BY, PREFERS, etc.) |
| Q10 | Decay model | Ebbinghaus + recall boost |
| Q11 | LLM involvement | Write time only. Read path is pure computation |
| Q12 | Decay trigger | TBD |
| Q13 | Embeddings | Local ONNX model (ships with binary) |
| Q14 | Storage constraint | In-process Rust, no separate DB process |
| Q16 | Storage engine | SQLite + sqlite-vec |
| Q17 | Panel archiving | Two-phase: detach → reverie extracts → panel lingers → auto-close |
| Q18 | Tool compaction | Dual-return: full_result + tldr_result per tool |
| Q19 | Extraction LLM | Same model as conversation |
| Q20 | Extraction output | Structured JSON via dedicated tool call |
| Q21 | TLDR generation | Dual-return from tool (tool author writes both) |
| Q22 | Memory access | Hybrid: auto-inject via Memory Views + explicit AI search |
| Q23 | Pinning | Memory Views replace pinning (persistent view = pinned items) |
| Q24 | Memory panel | Overview panel + multiple Memory View panels |
| Q25 | View dimensions | All four: embedding similarity, BM25, graph traversal, metadata |
| Q26 | View management | System default + AI customization |
| Q27 | View refresh | Per user turn + reverie mid-turn |
| Q28 | Existing memories | Clean slate — start fresh |
| Q29 | Logs | Replace entirely with episodic memory nodes |
| Q30 | Build order | Storage → Extraction → Retrieval → Views → TLDR → UI |
