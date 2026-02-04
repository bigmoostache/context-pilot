# Get Shit Done (GSD) - Complete Technical Architecture

## Executive Summary

GSD is a **meta-prompting, context engineering, and spec-driven development system** for Claude Code. It solves "context rot" — the quality degradation that occurs as Claude's context window fills up. The key innovation is the **orchestrator + subagent pattern** that keeps the main context window at ~30-40% usage while spawning fresh 200K token contexts for heavy work.

---

## 1. CORE ARCHITECTURE

### 1.1 Design Philosophy

```
┌─────────────────────────────────────────────────────────────────┐
│                    ORCHESTRATOR (Main Context)                   │
│  • Stays lean (~5% context usage)                                │
│  • Routes workflow between stages                                │
│  • Spawns specialized subagents                                  │
│  • Never does heavy computation                                  │
├─────────────────────────────────────────────────────────────────┤
│                         SUBAGENTS                                │
│  • Each gets FRESH 200K context window                          │
│  • Single-purpose specialists                                    │
│  • Commit their own work                                         │
│  • Write SUMMARY.md when complete                                │
└─────────────────────────────────────────────────────────────────┘
```

### 1.2 File Structure

```
~/.claude/                    # Global installation
├── commands/gsd/             # Slash commands (orchestrators)
│   ├── new-project.md
│   ├── plan-phase.md
│   ├── execute-phase.md
│   ├── verify-work.md
│   └── ...
├── agents/                   # Subagent definitions
│   ├── gsd-planner.md
│   ├── gsd-executor.md
│   ├── gsd-verifier.md
│   ├── gsd-debugger.md
│   ├── gsd-codebase-mapper.md
│   ├── gsd-plan-checker.md
│   ├── gsd-phase-researcher.md
│   ├── gsd-integration-checker.md
│   └── gsd-milestone-auditor.md
├── hooks/                    # Event hooks (statusline, update-check)
└── get-shit-done/           # Core system files

.planning/                   # Project-specific (per project)
├── PROJECT.md               # Vision, always loaded
├── REQUIREMENTS.md          # Scoped v1/v2 requirements
├── ROADMAP.md               # Phases and progress tracking
├── STATE.md                 # Decisions, blockers, memory across sessions
├── config.json              # Settings (mode, depth, profiles)
├── research/                # Ecosystem research docs
│   ├── STACK.md
│   ├── FEATURES.md
│   ├── ARCHITECTURE.md
│   └── PITFALLS.md
├── codebase/               # Brownfield analysis (map-codebase)
├── phases/                  # Phase-specific files
│   ├── 01-name/
│   │   ├── 01-CONTEXT.md    # User decisions from discuss-phase
│   │   ├── 01-RESEARCH.md   # Phase research
│   │   ├── 01-01-PLAN.md    # Plan 1
│   │   ├── 01-01-SUMMARY.md # Execution summary
│   │   ├── 01-02-PLAN.md    # Plan 2
│   │   ├── 01-02-SUMMARY.md
│   │   ├── 01-VERIFICATION.md
│   │   └── 01-UAT.md        # User acceptance test
│   └── 02-name/
│       └── ...
├── quick/                   # Quick mode tasks
│   └── 001-task-name/
│       ├── PLAN.md
│       └── SUMMARY.md
└── debug/                   # Debug sessions
```

---

## 2. COMMAND FORMAT (YAML FRONTMATTER)

Commands are thin orchestrators with YAML configuration:

```yaml
---
name: gsd:command-name
description: One-line description
argument-hint: "<required>" or "[optional]"
allowed-tools: [Read, Write, Bash, Glob, Grep, AskUserQuestion, Task]
---

# Command Implementation

<workflow>
1. Read state files
2. Spawn subagent with Task tool
3. Route to next command
</workflow>
```

**Key Tools:**
- `Task` - Spawns subagent with fresh context
- `TaskOutput` - Read subagent results (NEVER use for large outputs)
- `AskUserQuestion` - Structured user prompts

---

## 3. SUBAGENT DEFINITIONS

Each agent is a markdown file with role, constraints, and workflow:

```markdown
# gsd-planner.md

## Role
Creates executable phase plans with task breakdown, dependency analysis,
and goal-backward verification.

## Identity
[Color: green]
You are a GSD planner. Your job: Produce PLAN.md files that Claude 
executors can implement without interpretation.

## Constraints
- Plans are prompts, NOT documents that become prompts
- PLAN.md IS the prompt
- Maximum 2-3 tasks per plan
- Target ~50% context usage per plan
- Planning for ONE person (user) and ONE implementer (Claude)
- No teams, stakeholders, ceremonies

## Philosophy
<goal_backward_planning>
Forward planning asks: "What should we build?"
Goal-backward planning asks: "What must be TRUE for the goal to be achieved?"

Step 1: State the Goal (outcome, not work)
Step 2: Derive Observable Truths (3-7 verifiable behaviors)
Step 3: Derive Required Artifacts (specific files/objects)
Step 4: Derive Required Wiring (critical connections)
</goal_backward_planning>
```

**Agent Roster:**

| Agent | Purpose | Lines | Color |
|-------|---------|-------|-------|
| gsd-planner | Create PLAN.md files | 1000+ | green |
| gsd-executor | Execute plans, commit tasks | 800+ | yellow |
| gsd-verifier | Goal-backward verification | 700+ | blue |
| gsd-plan-checker | Validate plans achieve goals | 744 | green |
| gsd-debugger | Scientific debugging | 990 | orange |
| gsd-phase-researcher | Domain research | 915 | green |
| gsd-codebase-mapper | Brownfield analysis | 500+ | green |
| gsd-integration-checker | Cross-phase E2E | 400+ | blue |
| gsd-milestone-auditor | Milestone verification | 400+ | blue |

---

## 4. XML TASK FORMAT

Plans use structured XML for precise Claude instructions:

```xml
<task type="auto">
  <n>Create login endpoint with JWT</n>
  <files>src/app/api/auth/login/route.ts</files>
  <action>
    Use jose for JWT (not jsonwebtoken - CommonJS issues with Edge).
    Validate credentials against users table.
    Return httpOnly cookie on success with 15-min expiry.
  </action>
  <verify>curl -X POST localhost:3000/api/auth/login returns 200 + Set-Cookie</verify>
  <done>Valid credentials return cookie, invalid return 401</done>
</task>
```

**Task Types:**

| Type | Purpose |
|------|---------|
| `auto` | Fully autonomous execution |
| `checkpoint:human-verify` | Pause for user verification |
| `checkpoint:decision` | Pause for user decision |

**Checkpoint Example:**

```xml
<task type="checkpoint:human-verify" gate="blocking">
  <what-built>Login form with validation</what-built>
  <how-to-verify>
    1. Navigate to /login
    2. Enter test@example.com / password123
    3. Click Submit
    4. Should redirect to /dashboard
  </how-to-verify>
  <resume-signal>Reply "verified" to continue</resume-signal>
</task>

<task type="checkpoint:decision" gate="blocking">
  <decision>Authentication strategy</decision>
  <context>Need to choose token storage approach</context>
  <options>
    <option id="cookie">
      <n>HttpOnly Cookie</n>
      <pros>XSS-safe, auto-sent</pros>
      <cons>CSRF requires mitigation</cons>
    </option>
    <option id="local">
      <n>localStorage</n>
      <pros>Simple, SPA-friendly</pros>
      <cons>XSS vulnerable</cons>
    </option>
  </options>
  <resume-signal>Reply with option ID</resume-signal>
</task>
```

---

## 5. PLAN.MD STRUCTURE

```yaml
---
phase: 01-authentication
plan: 01
type: execute
wave: 1                      # Execution wave (parallel grouping)
depends_on: []               # Prior plans required
files_modified:              # Files touched (for dependency calc)
  - src/app/api/auth/login/route.ts
  - src/lib/auth.ts
autonomous: true             # false if has checkpoints
user_setup:                  # Human-only tasks (omit if empty)
  - service: stripe
    why: "Payment processing"
    env_vars:
      - name: STRIPE_SECRET_KEY
        source: "Stripe Dashboard -> Developers -> API keys"
must_haves:                  # Goal-backward derived
  truths:
    - "User can log in with email/password"
    - "Invalid credentials show error"
    - "Session persists across refresh"
  artifacts:
    - path: "src/app/api/auth/login/route.ts"
      state: "POST handler validates credentials"
    - path: "src/lib/auth.ts"
      state: "JWT functions with 15-min expiry"
  key_links:
    - "login route → auth lib → database"
---

# Phase 01, Plan 01: Login Endpoint

## Context
<!-- Loaded from CONTEXT.md, RESEARCH.md, PROJECT.md -->

## Tasks

<task type="auto">
  <!-- task 1 -->
</task>

<task type="auto">
  <!-- task 2 -->
</task>
```

---

## 6. WAVE-BASED EXECUTION

Plans are grouped into "waves" based on dependencies:

```
Wave 1: [Plan 01, Plan 02]  ← No dependencies, run in parallel
Wave 2: [Plan 03]           ← Depends on Plan 01
Wave 3: [Plan 04, Plan 05]  ← Depend on Plan 03, parallel
```

**Dependency Rules:**
- `depends_on: []` → Wave 1
- `files_modified` overlap → sequential
- No overlap → parallel eligible

**Execution Flow:**

```
┌─────────────────────────────────────────────────────────────┐
│                    EXECUTE-PHASE                             │
├─────────────────────────────────────────────────────────────┤
│ 1. Read all PLAN.md frontmatter                             │
│ 2. Group by wave number                                     │
│ 3. For each wave:                                           │
│    └─ Spawn gsd-executor for EACH plan (parallel)          │
│       └─ Executor:                                          │
│          • Reads PLAN.md                                    │
│          • Executes tasks sequentially                      │
│          • Commits after EACH task                          │
│          • Writes SUMMARY.md                                │
│ 4. After all waves: spawn gsd-verifier                      │
└─────────────────────────────────────────────────────────────┘
```

---

## 7. CONTEXT ENGINEERING

**The Core Problem:** Claude degrades when perceiving context pressure ("I'll be more concise now").

**GSD's Solution:**

| Strategy | Implementation |
|----------|----------------|
| Fresh contexts | Each plan executes in new 200K window |
| Size limits | Plans target 50% context usage max |
| Atomic tasks | 2-3 tasks per plan, specific files |
| State persistence | STATE.md, SUMMARY.md survive sessions |
| Selective loading | Only load what's needed per phase |

**Context Budget:**
```
Executor receives:
├── PROJECT.md          (~500 tokens)
├── REQUIREMENTS.md     (~1000 tokens)  
├── PLAN.md             (~2000 tokens)
├── Prior SUMMARY.md    (~500 tokens, if dependent)
└── Codebase context    (as needed)
────────────────────────
Target: <100K tokens (50% of 200K)
```

---

## 8. WORKFLOW STAGES

### 8.1 Project Initialization

```
/gsd:new-project
    │
    ├── 1. Deep questioning (goals, constraints, tech, edge cases)
    │      └─ Creates PROJECT.md
    │
    ├── 2. Research (optional, spawns 4 parallel researchers)
    │      └─ Creates research/STACK.md, FEATURES.md, ARCHITECTURE.md, PITFALLS.md
    │
    ├── 3. Define requirements
    │      └─ Creates REQUIREMENTS.md (v1, v2, out-of-scope)
    │
    └── 4. Create roadmap
           └─ Creates ROADMAP.md (phases mapped to requirements)
```

### 8.2 Phase Cycle

```
/gsd:discuss-phase N     ─────► {N}-CONTEXT.md
        │
        ▼
/gsd:plan-phase N        ─────► {N}-RESEARCH.md, {N}-{NN}-PLAN.md
        │                        (planner → checker → revise loop)
        ▼
/gsd:execute-phase N     ─────► {N}-{NN}-SUMMARY.md, {N}-VERIFICATION.md
        │                        (wave-based parallel execution)
        ▼
/gsd:verify-work N       ─────► {N}-UAT.md, fix plans if needed
        │
        ▼
    [Next phase or /gsd:complete-milestone]
```

### 8.3 Milestone Completion

```
/gsd:audit-milestone     ─────► Parallel verification agents
        │
        ▼
/gsd:complete-milestone  ─────► Archive, git tag, delete ROADMAP.md
        │
        ▼
/gsd:new-milestone       ─────► Fresh cycle for next version
```

---

## 9. GIT COMMIT STRATEGY

**Atomic commits per task:**

```bash
abc123f docs(08-02): complete user registration plan
def456g feat(08-02): add email confirmation flow
hij789k feat(08-02): implement password hashing
lmn012o feat(08-02): create registration endpoint
```

**Format:** `type(phase-plan): description`

**Benefits:**
- `git bisect` finds exact failing task
- Each task independently revertable
- Clean history for future Claude sessions
- Better observability in automated workflows

---

## 10. CONFIGURATION

### 10.1 config.json

```json
{
  "mode": "interactive",           // "yolo" or "interactive"
  "depth": "standard",             // "quick", "standard", "comprehensive"
  "profiles": {
    "active": "balanced",
    "presets": {
      "quality": {
        "planning": "claude-opus-4",
        "execution": "claude-opus-4",
        "verification": "claude-sonnet-4"
      },
      "balanced": {
        "planning": "claude-opus-4",
        "execution": "claude-sonnet-4",
        "verification": "claude-sonnet-4"
      },
      "budget": {
        "planning": "claude-sonnet-4",
        "execution": "claude-sonnet-4",
        "verification": "claude-haiku-4"
      }
    }
  },
  "workflow": {
    "research": true,              // Spawn researcher before planning
    "plan_check": true,            // Verify plans before execution
    "verifier": true               // Verify phase after execution
  },
  "parallelization": {
    "enabled": true
  },
  "planning": {
    "commit_docs": true            // Track .planning/ in git
  }
}
```

### 10.2 Depth Settings

| Depth | Planning | Plans per Phase |
|-------|----------|-----------------|
| quick | Minimal research | 1-2 |
| standard | Normal research | 2-4 |
| comprehensive | Deep research | 4-8 |

---

## 11. KEY EFFICIENCY MECHANISMS

### 11.1 Why GSD is Efficient

1. **Context isolation** - Heavy work in fresh subagents, orchestrator stays lean
2. **Parallel research** - 4 researchers run simultaneously (stack, features, arch, pitfalls)
3. **Wave parallelization** - Independent plans execute in parallel
4. **Goal-backward planning** - Derives requirements before tasks (no rework)
5. **Structured XML** - Precise instructions, no interpretation needed
6. **Atomic commits** - Clear history, easy rollback
7. **Verification loop** - Plan checker catches issues before execution

### 11.2 Anti-Patterns Avoided

| Anti-Pattern | GSD Solution |
|--------------|--------------|
| Monolithic context | Fresh 200K per plan |
| Vague instructions | Structured XML tasks |
| Forward planning | Goal-backward derivation |
| Manual coordination | Wave-based orchestration |
| Context pollution | Selective loading |
| Large plans | 2-3 tasks max, 50% context target |

---

## 12. SPAWNING SUBAGENTS

**Using Task tool:**

```javascript
// Orchestrator spawns executor
Task({
  agent: "gsd-executor",
  prompt: `
    Execute plan: ${planPath}
    Project context: ${projectMd}
    Prior summary: ${priorSummary || "N/A"}
  `,
  tools: ["Read", "Write", "Bash", "Glob", "Grep"]
})
```

**Critical Rules:**
- NEVER use `TaskOutput` to read large results
- Subagent writes SUMMARY.md, orchestrator reads file
- Spawn multiple in ONE message for parallel execution
- Each subagent commits its own work

---

## 13. STATE MANAGEMENT

### 13.1 STATE.md

```markdown
# Project State

## Current Position
- **Phase**: 01-authentication
- **Plan**: 02
- **Status**: executing

## Decisions Made
- Auth: JWT with httpOnly cookies
- Database: PostgreSQL with Prisma

## Blockers
- None

## Session Notes
- 2024-01-15: Completed login, starting registration
```

### 13.2 Session Continuity

```
/gsd:pause-work   → Creates handoff document
/gsd:resume-work  → Restores from last session
/gsd:progress     → Shows status, routes to next action
```

---

## 14. QUICK MODE

For ad-hoc tasks without full planning:

```
/gsd:quick
> "Add dark mode toggle to settings"
```

Creates:
```
.planning/quick/001-add-dark-mode/
├── PLAN.md
└── SUMMARY.md
```

**Guarantees retained:**
- Atomic commits
- State tracking
- SUMMARY.md output

**Skipped:**
- Research phase
- Plan checker
- Verifier

---

## 15. DEBUGGING SYSTEM

```
/gsd:debug [issue description]
```

**Scientific Method:**
1. Symptom gathering (questioning)
2. Hypothesis formation
3. Investigation loop (evidence collection)
4. Root cause diagnosis
5. Fix and verify

**Debug File:**
```markdown
# Debug: Login fails intermittently

## Status: investigating

## Hypotheses
1. [ ] Race condition in token refresh
2. [x] CORS misconfiguration ← ELIMINATED
3. [ ] Database connection timeout

## Evidence
- Error occurs only on slow connections
- Tokens valid, endpoint returns 200
- Network tab shows retry at 3s mark

## Current Focus
Testing hypothesis 1 with network throttling
```

---

## 16. VERIFICATION SYSTEM

### 16.1 Automated (gsd-verifier)

Checks `must_haves` against codebase:
- Do files exist?
- Do they contain expected code?
- Do tests pass?

### 16.2 Manual (gsd:verify-work)

User acceptance testing:
1. Extract testable deliverables
2. Walk through one at a time
3. Diagnose failures automatically
4. Create fix plans if needed

---

## 17. INSTALLATION MECHANISM

```javascript
// bin/install.js
const targets = {
  claude: {
    global: "~/.claude/",
    local: "./.claude/"
  },
  opencode: {
    global: "~/.config/opencode/",
    local: "./.opencode/"
  },
  gemini: {
    global: "~/.gemini/",
    local: "./.gemini/"
  }
};

// Copies:
// - commands/gsd/ → target/commands/gsd/
// - agents/ → target/agents/
// - hooks/ → target/hooks/
// - get-shit-done/ → target/get-shit-done/
```

---

## 18. SUMMARY: WHAT MAKES GSD EFFECTIVE

1. **Orchestrator pattern** - Thin coordinator, heavy workers isolated
2. **Fresh contexts** - No degradation, consistent quality
3. **Goal-backward planning** - Derive requirements, not tasks
4. **Structured XML** - Precise, unambiguous instructions
5. **Wave execution** - Parallel when possible, sequential when needed
6. **Atomic commits** - Surgical, traceable, revertable
7. **Verification loops** - Plan checker + verifier catch issues early
8. **State persistence** - Memory across sessions via markdown files
9. **Research phase** - Ecosystem knowledge before planning
10. **Discuss phase** - User decisions locked before research

---

## 19. RECOMMENDATIONS FOR VERSION 2

Based on this analysis, potential improvements:

1. **Dynamic context budgeting** - Auto-detect when approaching limits
2. **Smarter wave scheduling** - ML-based dependency prediction
3. **Cross-project learning** - Reuse research across similar projects
4. **Real-time monitoring** - Dashboard for multi-agent progress
5. **Rollback automation** - Auto-revert on verification failure
6. **Cost optimization** - Token usage tracking and model switching
7. **Custom agent creation** - User-defined specialist agents
8. **Integration hooks** - CI/CD, issue trackers, notifications
9. **Collaborative mode** - Multi-user coordination
10. **Caching layer** - Reuse common context across sessions