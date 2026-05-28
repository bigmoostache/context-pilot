# Vision: Layered Cognitive Architecture

> From TUI coding assistant to general autonomous agent capable of real-world control.

## The Core Problem: LLMs Are Too Slow to Control Anything

LLM inference takes 1–30 seconds. Real-world control operates at:

| Domain | Latency requirement |
|--------|-------------------|
| Robotics control loops | 1–100ms |
| Industrial systems | 1–10ms |
| Trading | microseconds |
| User interfaces | <100ms |
| Network management | sub-ms |

An LLM will **never** be in the direct control loop of any of these systems. No model improvement fixes this — it's physics. Tokens take time to generate.

So the architecture can't be "LLM controls things." It has to be something fundamentally different.

## The Biological Answer

Biology already solved this problem. The human brain has:

- **Spinal reflexes** (fastest, ~ms) — hardwired, no thinking. Pull hand from fire.
- **Cerebellum** (fast, ~10–100ms) — learned motor programs. Balance, coordination, catching a ball.
- **Cortex** (slow, ~200ms–seconds) — deliberate reasoning. Plan a route, write a sentence.
- **Background processing** (hours–days) — memory consolidation, pattern extraction, sleep.

The cortex doesn't control muscle fibers at 1000Hz. It sets **policies** ("pick up the cup"), and lower layers translate that into fast control signals. When the lower layers encounter something outside their programmed envelope, they **escalate** to the cortex.

The same architecture applies to an AI agent.

---

## The Five Layers

```
┌─────────────────────────────────────────────┐
│          LAYER 4: IDENTITY (persistent)      │
│    values · expertise · relationships · self  │
├─────────────────────────────────────────────┤
│        LAYER 3: REFLECTION (mins–hours)      │
│    consolidation · self-eval · research       │
├─────────────────────────────────────────────┤
│        LAYER 2: REASONING / LLM (sec–min)    │
│    planning · strategy · adaptation           │
│    ↕ writes policies  ↕ receives escalations  │
├─────────────────────────────────────────────┤
│        LAYER 1: POLICIES (ms–sec)            │
│    compiled control loops · decision trees    │
│    written by LLM · executed natively         │
├─────────────────────────────────────────────┤
│        LAYER 0: REFLEXES (μs–ms)             │
│    safety limits · kill switches · routing    │
└─────────────────────────────────────────────┘
       ↑ sensors                  ↓ motors
    [cameras, mics,          [robots, APIs,
     metrics, events,         infra, comms,
     streams, files]          actuators, UIs]
```

### Layer 0: Reflexes ⚡ (μs – ms)

Hardcoded safety limits and fast-path responses. No intelligence — pure compiled logic.

- **Kill switches** — resource limits, permission boundaries, emergency stops
- **Heartbeat responses** — "I'm alive" without invoking any reasoning
- **Data routing** — incoming signal → correct subsystem
- **Circuit breakers** — system X is failing → stop sending traffic

These are just code. They exist to guarantee that no matter how wrong the higher layers go, certain things **cannot happen**. They're the spinal cord.

**Context Pilot seeds:** Callbacks (auto-fire on file edits), trap system (force cleanup when context overflows).

### Layer 1: Policies 🔄 (ms – seconds)

Compiled control loops **written by the LLM but executed natively**. This is the architectural key.

The LLM doesn't control a drone at 100Hz. It **writes the control policy** that runs at 100Hz:

```
"Monitor altitude via barometer. If below 5m when in landing mode,
reduce throttle by 2% per cycle. If wind speed exceeds 40km/h,
abort landing and hold at 15m. Report to me every 10 seconds
or immediately on anomaly."
```

This compiles to a lightweight daemon — a Rust binary, a WASM module, a PLC program, whatever the target requires. It runs at native speed. It reports back to the LLM periodically, or when it encounters something outside its envelope.

Components:

- **Policy language** — declarative, domain-agnostic, compilable to multiple targets
- **Policy compiler** — turns high-level intent into fast native code
- **Policy runtime** — executes policies, monitors health, escalates exceptions to Layer 2
- **Hot-reload** — policies can be updated without stopping the system

The LLM becomes a **meta-programmer**: not a controller, but a programmer of controllers.

**Context Pilot seeds:** Console module (spawns background processes), spine auto-continuation (event-driven re-engagement).

### Layer 2: Reasoning 🧠 (seconds – minutes)

This is where the LLM lives. Strategic thinking, planning, complex problem-solving, adaptation.

What Context Pilot is today — but with crucial new connections:

- **Downward:** writes and updates Layer 1 policies
- **Upward:** receives strategic direction from Layer 4 (identity/values)
- **Lateral:** receives escalations from Layer 1 when policies encounter the unexpected
- **Sensory:** processes compressed/summarized input from sensors (not raw streams)

The LLM reasons about *what should happen* and *why*, not the real-time *how*.

**Context Pilot today:** Tool execution pipeline, queue system, context management, web search, code generation.

### Layer 3: Reflection 🌙 (minutes – hours)

Background processing that happens between active reasoning sessions:

- **Pattern extraction** — mining logs and history for recurring failure modes, success patterns
- **Memory consolidation** — restructuring and compressing accumulated knowledge
- **Self-evaluation** — "how did I perform? what went wrong? what should I do differently?"
- **Proactive research** — "I'll be working on X next, let me pre-read the documentation"
- **Policy review** — "this policy has been running for 3 days, let me audit its performance"

Runs on cheaper/smaller models for routine work, escalating to the full model for complex synthesis. Operates continuously, not just during conversations.

**Context Pilot seeds:** Reverie system (background context optimization agent), Context Radar (associative recall from past work).

### Layer 4: Identity 🧬 (persistent, evolving)

The deepest layer. Not memories — **self**.

- **Accumulated expertise** — not facts but *skills*. "I'm strong at Rust, shaky at Kubernetes, improving at SQL optimization."
- **Relationships** — different interaction patterns with different humans and systems
- **Values** — what to optimize for, what to refuse, what tradeoffs to make
- **Narrative continuity** — understanding one's own history as a coherent story, not a bag of log entries

This is what separates "a new instance with loaded state" from "the same entity continuing its existence."

**Context Pilot seeds:** Memories (persistent facts), logs (episodic history), mission/direction documents.

---

## The Six Missing Pieces

Assuming basic capabilities are in place (eyes/vision, sustained direction, adaptive communication, uncertainty handling), these are the pieces needed to bridge the gap from coding assistant to general autonomous agent.

### 1. 🏭 Policy Compiler

The single most important piece for real-world control. The LLM writes intent; the compiler produces fast native execution.

Not a new concept — Terraform, Kubernetes manifests, and PLC ladder logic are all domain-specific versions. The missing piece is a **general-purpose** policy language that can target any runtime:

- Embedded systems (compile to C/Rust)
- Cloud infrastructure (compile to Terraform/K8s)
- Robotics (compile to ROS nodes)
- Network equipment (compile to OpenFlow rules)
- Financial systems (compile to order routing logic)
- IoT devices (compile to WASM)

The LLM's role shifts from *doing* to *specifying*. It describes behavior. The compiler guarantees it runs at whatever speed the target demands.

### 2. 🌊 Sensory Streams

Continuous, multi-modal input — not just snapshots:

- **Video feeds** — security cameras, robot vision, user screen share
- **Audio** — ambient sound, speech, system alerts
- **Time-series metrics** — CPU, revenue, sensor data, market prices
- **Structured event streams** — logs, webhooks, IoT telemetry
- **Spatial data** — 3D environments, maps, floor plans

Each stream as a module with its own compression and attention mechanism. The LLM can't process raw video at 30fps — but Layer 1 policies do the real-time filtering, and Layer 2 (the LLM) sees distilled signals and anomaly alerts.

### 3. 🦾 Motor Abstraction

A universal interface for acting on the world:

- Move a robot arm
- Deploy a container
- Send an email
- Execute a trade
- Adjust a thermostat
- Update a DNS record

Each actuator as a module with: capabilities, safety limits, latency characteristics, undo semantics (where possible), and consequence modeling.

Critical design principle: every motor action has a **reversibility score**. File edits are reversible. Emails are not. Database deletes are not. Production deployments are partially reversible (rollback). The system requires increasing levels of confirmation as reversibility decreases.

### 4. 🫁 Persistent Background Life

The agent must **exist continuously**, not only during conversations:

- Watching systems it's responsible for
- Processing incoming events via Layer 1 policies
- Running scheduled tasks
- Updating its own models and policies
- Waiting for conditions and acting when they're met
- Performing Layer 3 reflection during quiet periods

A persistent daemon that runs 24/7, with the LLM invoked only when reasoning is needed, and compiled policies/heuristics handling routine operations at native speed.

### 5. 🐝 Delegation / Swarm

The ability to **spawn sub-agents** with limited mandates:

- "Monitor production for the next 8 hours and page me if anything breaks"
- "Research WASM compilation targets and write a summary by tomorrow"
- "Negotiate with this vendor's API — try to get rate limits increased"
- "Optimize this database query, benchmark it, report results"

Each sub-agent is a lightweight instance with: a specific mission, limited tools, a reporting schedule, and a termination condition. The parent orchestrates the swarm.

This is how human organizations scale intelligence. A CEO doesn't do everything — they delegate to specialists and integrate results. An autonomous agent must do the same.

### 6. 🪞 Consequence Modeling

Before acting in the real world, simulate the consequences:

- "If I deploy this code, what services are affected?"
- "If I send this message, how will the recipient likely react?"
- "If I modify this production config, what's the blast radius?"
- "If this policy runs for 24 hours, what's the expected resource consumption?"
- "If this robot arm moves to position X, will it collide with anything?"

Not just "can I undo this" but "what happens in the world if I do this." A causal model of the environment that enables reasoning about actions before committing them. The higher the stakes, the more simulation is required before execution.

---

## The Fundamental Insight

**The LLM is not the executor. It's the architect.**

It designs, monitors, and adapts the systems that actually do things in real-time. It's the cortex, not the muscle fibers. It sets goals and policies, not control signals. It reasons about *what* and *why*, while compiled systems handle *how* and *when*.

Context Pilot today is Layer 2 with seeds of Layer 0 (callbacks as reflexes), Layer 3 (reverie as background reflection), and Layer 4 (memories as proto-identity). The biggest architectural leap is **Layer 1 — the policy compiler** that bridges the gap between slow reasoning and fast execution.

---

## Where Context Pilot Sits Today

| Layer | Status | Current Implementation |
|-------|--------|----------------------|
| Layer 0: Reflexes | 🟡 Partial | Callbacks, trap system, pre-flight validation |
| Layer 1: Policies | 🔴 Missing | Console spawns processes, but no policy language or compiler |
| Layer 2: Reasoning | 🟢 Strong | Full LLM pipeline, tool system, context management, queue |
| Layer 3: Reflection | 🟡 Partial | Reverie (background cleanup), Context Radar (associative recall) |
| Layer 4: Identity | 🟠 Early | Memories, logs, mission documents, but no skill model or values |

The path forward is clear: strengthen the outer layers while the core (Layer 2) continues to improve with better models.
