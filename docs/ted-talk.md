# Rebuilding my own Claude Code
### The Whys, the Hows and the Wows

*GenAI Days 2026 — Agentic at Scale Conference — April 16, Paris*
*25 minutes. 4 slides (title + 3 content).*

---

## Ligne Rouge (narrative thread)

> **AI doesn't need more freedom — it needs a better workshop, stricter rules, and a compiler that won't let it cheat.**

Three pillars: **Workshop. Rules. Compiler.**

Emotional arc: curiosity → frustration → building → awe → insight.

---

## OPENING (30s)

*[Slide 0: Title]*
*[Slide 1: 762 / 65K / 22 / 0]*

> "762 commits. 65,000 lines of Rust. 22 crates. Zero crashes."

*(pause)*

*[Slide 2: "I didn't write most of it. An AI did."]*

> "I didn't write most of it. An AI did."

*(let it land)*

> "How? That's what I'm here to tell you."

*[Slide 3: THE WHYS]*

---

## Slide 1: THE WHYS (~7 min)

### Beat 1 — Context Amnesia (2 min)

*[Slide 4: "Every AI coding tool starts every session from zero."]*

> "Who here has used Claude Code, Cursor, Copilot?"

*(scan room)*

> "You've all had this moment. You open a new session. And the AI has no idea who you are. Your architecture? Gone. The bug you fixed yesterday? Forgotten."

> "Every tool on the market has the same problem: **context amnesia**."

*[Slide 5: "The industry's answer? A static markdown file."]*

> "The industry's answer is CLAUDE.md. A static markdown file. You dump your project conventions in there and hope the AI reads it."

> "People on Reddit are calling themselves '*context engineers*' now. They're reinventing filing cabinets. In 2026."

### Beat 2 — The Vibe Coding Crisis (2 min)

*[Slide 6: 41%]*

> "Meanwhile."

*(pause, let the number land)*

> "41% of all code pushed to production is now AI-generated."

*[Slide 7: 3×]*

> "And it accumulates technical debt three times faster."

*[Slide 8: "Senior devs were 19% slower. They believed they were 20% faster."]*

> "A study from METR. Senior developers. 19% slower with AI tools."

*(beat)*

> "But they *predicted* they'd be faster. And even after seeing the data, they still *believed* they were faster. The placebo effect of AI coding."

### Beat 3 — No Feedback Loops (2 min)

*[Slide 9: "AI writes code. You review it. Where is the compiler?"]*

> "Here's the real gap. AI writes code. You review it. You tell it to fix things. It fixes them. You review again."

> "But where in this loop is the compiler? Where is the linter? Where is *any* automated quality gate?"

*[Slide 10: "Never send an LLM to do a linter's job."]*

*(gesture to slide, pause)*

### Transition (1 min)

*[Slide 11: "What if the problem isn't the AI?"]*

> "So I was using Claude Code. Context amnesia. No quality gates. And one day I asked myself..."

*[Slide 12: "What if it's the workshop?"]*

> "What if, instead of giving the AI more freedom... I gave it a **better workshop**?"

*[Slide 13: THE HOWS]*

---

## Slide 2: THE HOWS (~10 min)

### Beat 1 — Panels, Not Chat (3 min)

*[Slide 14: Comparison — Everyone else vs Context Pilot]*

> "Every AI coding tool works the same way. You have a conversation. Messages pile up. Context overflows. You start over."

> "I flipped this completely. In my tool, context is not a conversation — it's a **workspace**."

> "Panels — files, search results, git status, memories — discrete chunks the AI can see, manage, and control."

*[Slide 15: "The AI manages its own context."]*

> "The AI opens files it needs. Closes panels it doesn't. Creates memories that persist across sessions. Searches its own codebase."

> "It's not a chat. It's a workshop."

> "And because panels don't change between turns, they get **cache hits** — 90% cheaper. The architecture *is* the cost optimization."

### Beat 2 — The Feedback Loop (3 min)

*[Slide 16: "6 — automated checks fire on every single edit"]*

> "And every time the AI touches a Rust file..."

*[Slide 17: The 6 checks list]*

*(don't read the list — gesture to it)*

> "Six checks. Automatic. **Blocking**. The AI cannot continue until every single one passes."

*[Slide 18: "Edit → Error → Fix → Pass. 200 ms."]*

> "If the compiler finds an error, the AI sees it and fixes it. Same response. Sub-second."

*[Slide 19: "A junior developer who never argues..."]*

> "Imagine a junior dev who never gets defensive. Never argues with the linter. Never says '*I'll fix it later*.' And retries in 200 milliseconds."

> "That's what this is."

### Beat 3 — The Trust Architecture (2 min)

*[Slide 20: "962 lints. 942 at forbid level."]*

> "962 rules. 942 at the strictest level — *forbid*. The AI cannot override them. No tricks. No workarounds."

*[Slide 21: "In 65,000 lines: 6 exceptions."]*

> "In the entire codebase — 65,000 lines — six exceptions. Each one justified and registered."

*[Slide 22: "The AI cannot weaken its own rules."]*

> "And the configuration is protected by a cryptographic hash chain, sealed with a human password."

> "The AI literally cannot weaken its own rules. I have to type a password to change them."

### Beat 4 — Self-Hosting (2 min)

*[Slide 23: "Context Pilot is built with Context Pilot."]*

> "And here's where it gets recursive."

*(pause)*

> "The tool builds itself. The AI edits its own codebase. Those six checks fire on its own code. The compiler reviews the AI's edits to the AI's own tool."

*[Slide 24: "Every improvement makes the next one better. Compound returns."]*

> "Every time it improves its own search, its own context management, its own feedback loops — the *next* time it edits itself, it works better."

> "Compound returns."

*Transition:*

> "That's the workshop. Let me show you what happened."

*[Slide 25: THE WOWS]*

---

## Slide 3: THE WOWS (~7 min)

### Beat 1 — The Lint Migration (2 min)

*[Slide 26: "I raised every lint to forbid..."]*

> "One day, I raised the bar. I took nearly a thousand available lints and set every single one to the strictest level. Military grade."

> "Then I told the AI: *make the codebase comply.*"

*[Slide 27: "0 crashes since that day"]*

> "It did. By itself. In a day. It rewrote its own entire codebase."

*(pause)*

> "And since that day — not a single crash."

### Beat 2 — Beyond Coding (2 min)

*[Slide 28: "Then the tool transcended coding."]*

*(softer tone)*

> "But here's what I didn't expect."

*[Slide 29: "CAT 4 FDA drug interaction."]*

> "I used it to map the medical history of people close to me. Years of diagnostic wandering. Doctors couldn't find what was wrong."

> "The AI analyzed the files... and flagged a **category 4 FDA drug interaction**. Potentially dangerous. The doctors had missed it for years."

*(3-second silence)*

> "The tool I built to write Rust code found a dangerous drug interaction."

*[Slide 30: "Collège de France... publish a paper."]*

*(new energy)*

> "My mom is a chemist at the Collège de France. We used the tool for analysis of metallic catalysts. She was — her words — *completely blown away*. We discovered novel chemical descriptors."

> "We're going to publish a paper."

*(pause)*

> "A coding tool. Publishing chemistry research."

### Beat 3 — One More Thing (1 min)

*[Slide 31: "One more thing."]*

*(smile)*

*[Slide 32: "This was a side project."]*

> "This was a **side project**."

> "All of it. The 762 commits. The 65,000 lines. The 22 crates. The zero crashes. Built while running two to four other projects at the same time."

> "In under three months, it became the center of everything I do — professionally and personally."

> "It's not just a tool I built. It's a tool that **multiplied everything else I do**."

### Beat 4 — The Insight (1 min)

*[Slide 33: "The breakthrough isn't the model. It's the workspace."]*

> "I want to leave you with one thought."

> "Everyone in this room is thinking about better models. Bigger context windows. More parameters."

> "I think that's the wrong question."

---

## CLOSE (30s)

*[Slide 34: "Build the harness. Add the rules. Trust the compiler."]*

*(standing still, simple delivery)*

*[Slide 35: "Merci."]*

*(nod)*

---

## 📋 Q&A Reserve

Material intentionally held for audience questions:

| Topic | Key data |
|-------|----------|
| Token economics | Claude Code $6/day avg, cache expiry 1h, conversation cost spiral |
| Vendor dependency | Feb 2026 quality regression, 32% sentiment collapse |
| Competitor deep dive | Cursor RAG, Copilot indexing, market shares |
| FPGA + Schrödinger | AI installed drivers on Puzhi board, ran quantum simulations |
| Event intelligence | 100 famous guests mapped in 1 hour |
| Platform emergence | CP being forked as basis for other AI projects |
| Prompt caching details | 5-min TTL, 4 breakpoints, prefix-based, freeze optimization |
| Context Radar | Meilisearch + Voyage AI + task signals + log-count decay |
| Memory architecture | Episodic (logs) + semantic (memories) + working (scratchpad) + reverie |
| Self-hosting history | GCC, Rust, Lisp, Java, Roslyn bootstrapping precedents |
| IR pipeline | Semantic styling, ratatui decoupling, future web frontend |
| Velocity curve | Acceleration over time, compound returns inflection point |

---

## 📋 Logistics & Prep

### Slides / Visuals
- [ ] Title slide with Context Pilot logo
- [ ] Slide 1: "THE WHYS" — could show CLAUDE.md screenshot, vibe coding stats
- [ ] Slide 2: "THE HOWS" — panel workspace screenshot, callback pipeline diagram, lint count
- [ ] Slide 3: "THE WOWS" — git log scrolling, side-by-side before/after

### Demo Candidates (recorded, not live)
- [ ] 30s screen recording: AI edits file → callbacks fire → error → auto-fix → pass
- [ ] 15s: panel workspace view showing structured context

### Tone Notes
- Enthusiastic but grounded. Not "AI will replace programmers" — "here's what's actually possible right now."
- Technical enough to impress developers, accessible enough for CTOs/PMs.
- Self-deprecating where useful: "I built a hash chain to stop my own AI from weakening the linter. Yes, I have trust issues."
- The Rust evangelism should feel earned, not preachy. Show, don't tell.
- REX culture — concrete, honest, lessons learned. Not a sales pitch.
- The medical story needs genuine emotion. Don't rush it. Pause after the drug interaction line.
