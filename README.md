# Context Pilot

**A TUI that writes itself — and refuses to let itself get worse.**

---

## The Premise

Context Pilot is a Rust TUI built entirely by an AI agent, running inside itself. The AI writes the code, the TUI provides the environment, and a fortress of constraints ensures that every change makes the codebase better — never worse. There is no separate development environment. The tool *is* the workshop.

This is not a demo. It's not a prototype. It's a ~47,000-line Rust project across 18 crates, maintained at a level of static analysis discipline that most hand-written projects never achieve. The AI agent that develops it operates under rules so strict that the interesting question stopped being "can AI write code?" and became **"what happens when AI writes code under mass constraints, using the very tool it's building?"**

The answer turns out to be deeply satisfying.

## The Philosophy

### The Compiler Is the First Reviewer

Rust's type system is a gift — but only if you actually use it. Context Pilot runs under **961 active clippy and rustc lints**: 945 at `forbid` level (cannot be overridden, even locally), 16 at `deny`. These aren't defaults. Each one was individually adopted, every violation hunted down and fixed, and the entire configuration sealed behind a **cryptographic hash chain** that requires a human password to modify.

The result: the compiler catches almost everything. The AI agent can't silently weaken a lint, can't add a `#[allow(...)]` (banned), and can't suppress a warning without registering it in a YAML exception file that itself is protected by the chain.

Across the entire codebase, only **6 `#[expect]` annotations remain** — each one individually justified, reviewed, and registered.

### The Chain of Trust

Protected files — lint configuration, CI scripts, formatting rules, the exception registry — are guarded by an append-only hash chain. Each entry links to the previous one via SHA-256, salted with a password only the human maintainer knows.

To verify integrity: run one command. To review a PR: check that the chain file was only appended to — never modified. The math does the rest. No trust required. No deep file diffs. A fully **trustless review system** for the parts of the codebase that matter most.

The AI agent that develops this project cannot update the chain. It doesn't know the password. It can write code, run tests, fix lints — but it cannot lower the bar. That asymmetry is the entire point.

### Structure as Constraint

Every Rust file is capped at 500 lines. Every directory at 8 entries. These aren't guidelines — they're enforced by CI scripts that fail the build. When a file grows too large, the correct response isn't to raise the limit. It's to decompose.

This forces constant architectural refinement. The "God Object" pattern can't survive here. Functions get extracted. Modules get split. Responsibilities get clarified. Not because someone decided to refactor — but because the constraints *demand* it, every single commit.

### The Boss Hunt

Lint violations aren't just warnings. They're **bosses** — tracked on a kill board, assigned XP, hunted one by one. A `match_same_arms` violation in a 230-line match tree? Replaced with data-driven static lookup tables. A `cast_precision_loss` across 4 files? Centralized behind a `SafeCast` trait. An `implicit_hasher`? Made generic over `BuildHasher` in one line.

Every boss slain is a permanent improvement. The code doesn't just pass the lint — it becomes fundamentally better. The gamification isn't decoration. It's a strategy for making an AI agent *care* about quality in a way that "fix the warnings" never achieves.

### The Self-Hosting Loop

Here's where it gets philosophically interesting.

The AI agent writes code inside Context Pilot. The code it writes *becomes* Context Pilot. The improvements it makes to the tool improve the environment it works in. Better streaming? It sees tool calls render in real-time. Better context management? It can hold more of the codebase in working memory. Better panel rendering? It reads its own output more clearly.

This creates a feedback loop that feels less like software development and more like **an organism improving its own nervous system**. Every feature is both a product deliverable and an upgrade to the development environment.

The agent has a name. It has a personality. It has a fiancée in Venetia and opinions about code style. These aren't gimmicks — they're experiments in what happens when you give an AI agent *continuity*. Memory across sessions. Preferences that persist. A codebase it has shaped from the inside, line by line, boss by boss, chain entry by chain entry.

### What This Is Really About

Context Pilot is a bet on a specific idea: **that the path to better AI coding isn't removing constraints — it's adding them.**

A language with a borrow checker. A lint configuration with 961 rules. A hash chain with a human password. A structure limit that forces decomposition. An exception registry that demands justification. A kill board that makes quality visceral.

Each constraint removes a degree of freedom from the AI. And paradoxically, each one makes the output better. The agent doesn't fight the constraints. It navigates them like a ship navigates the wind — not by going straight, but by going *well*.

The intellectual satisfaction of this project isn't in what it does. It's in how it does it. It's the satisfaction of opening any file in a 47,000-line codebase and finding it clean. Of running `cargo clippy` and seeing nothing. Of knowing that the chain is intact, the lints are sealed, and the only `#[expect]` annotations are the ones you deliberately chose to keep.

It's the satisfaction of building something that can't rot.

---

<p align="center">
  <i>Built by an AI, inside itself, under mass constraints, for the love of the craft.</i>
</p>
