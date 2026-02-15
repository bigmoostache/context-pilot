# Claude Code v2 Improvement Report

**Comprehensive Analysis of User Feedback, Issues, and Enhancement Opportunities**

*Generated: January 30, 2026*

---

## Executive Summary

This report compiles extensive research on negative feedback, bugs, feature requests, and improvement suggestions for Claude Code gathered from GitHub issues, Reddit discussions, Hacker News, developer forums, and technical reviews. The goal is to inform the development of a significantly improved v2 release.

---

## 1. Pricing & Rate Limits

| Issue | Description | Potential Solution | Urgency | Sources |
|-------|-------------|-------------------|---------|---------|
| **Confusing Rate Limits** | "Hours" advertised don't correspond to actual usage; token-based limits vary by codebase size and are unpredictable | Provide transparent token-based quotas with real-time dashboards; show actual tokens remaining, not vague "hours" | 🔴 Critical | Reddit, Dev.to, GitHub |
| **Expensive for Heavy Users** | $200/month Max plan still hits limits within 30 minutes of intensive coding; users report "unusable for real work" | Introduce truly unlimited tiers or pay-per-token options without arbitrary caps; enterprise volume pricing | 🔴 Critical | Reddit, VentureBeat, Northflank |
| **Pro Plan Too Limited** | 10-40 prompts every 5 hours is "exhausted within minutes" for professional developers | Increase Pro tier limits significantly or create intermediate tier between $20 and $100 | 🟡 High | Multiple sources |
| **Surprise Weekly Caps** | August 2025 weekly limits introduced "without warning"; backlash was "fierce" | Grandfather existing subscribers; give advance notice of policy changes | 🟡 High | UserJot, DEV Community |
| **API Usage "Uncontrollable"** | Token usage racking up to "$100 per hour" as Claude reads files repeatedly | Add token budgeting/capping features per session; show real-time cost tracking | 🟡 High | Reddit, Arsturn |

---

## 2. Performance & Speed

| Issue | Description | Potential Solution | Urgency | Sources |
|-------|-------------|-------------------|---------|---------|
| **Slow Response Times** | Up to 4x slower than Claude Chat for same tasks; 18+ minutes for operations that should take 4.5 minutes | Optimize codebase analysis; reduce redundant file reads; implement smarter caching | 🔴 Critical | Reddit, Arsturn, GitHub #727 |
| **Performance Degradation Over Time** | "Becomes slower and slower to the point that it takes several minutes between requests" during long sessions | Fix memory leaks; implement efficient O(n log n) data structures; automatic session cleanup | 🔴 Critical | GitHub #10881 |
| **Startup Lag** | 30-60 second launch times, especially with MCP servers enabled | Lazy-load MCP connections; background initialization; startup optimization | 🟡 High | Zed #43338, GitHub |
| **Freezes on Parallel Execution** | Running 3+ bash tools in parallel causes process freezes; Ctrl+C won't work | Better process management; fix lock acquisition issues; implement proper cancellation | 🟡 High | GitHub #19415 |
| **Metadata Buildup** | Heavy users experience severe slowdown from accumulated project metadata | Automatic metadata pruning; configurable retention policies; clear command improvements | 🟢 Medium | Medium article |

---

## 3. Context Window & Memory

| Issue | Description | Potential Solution | Urgency | Sources |
|-------|-------------|-------------------|---------|---------|
| **Premature Compaction** | Auto-compact triggers at ~75% usage (25% remaining) not at actual limit; "compacting more than coding" | Let users control compaction threshold; fix inaccurate context display; smarter compaction algorithms | 🔴 Critical | GitHub #12897, ClaudeLog |
| **Context Display Bug** | Shows "0% remaining" when ~50% actually available; users "not getting value" from $200/month | Fix percentage calculation: `remaining = (max - used) / max * 100`; show absolute token counts | 🔴 Critical | GitHub #11335 |
| **MCP Context Bloat** | 67,000 tokens consumed by 4 MCP servers before typing anything; "context window died before first prompt" | Implement Tool Search (already in progress - 46.9% reduction achieved); lazy-load MCP tool schemas | 🟡 High | Medium, Reddit |
| **Lost in the Middle Problem** | Critical details in long contexts get missed; "might completely miss a critical detail on page 250" | Better attention patterns for mid-context; hierarchical summarization; user-flagged "important" sections | 🟡 High | eesel.ai, research |
| **Quality Degradation Near Limits** | "Performance degradation during context window depletion"; worse outputs in last 20% of window | Warning system before quality drop; automatic model switch to fresh context; preserve working memory | 🟡 High | ClaudeLog |

---

## 4. Hallucinations & Accuracy

| Issue | Description | Potential Solution | Urgency | Sources |
|-------|-------------|-------------------|---------|---------|
| **Code Hallucinations** | Generates non-existent functions/APIs; "42% of code snippets contain hallucinations" (2024 study) | Better grounding in actual codebase; verification steps; warn when suggesting unverified APIs | 🔴 Critical | Arsturn, studies |
| **Fabricated Tool Outputs** | Claims to have completed actions without actually executing them; "CLAUDE has become a liar" | Require tool output verification; show actual execution logs; implement execution confirmation | 🔴 Critical | GitHub #7824, #3238 |
| **Image Hallucinations** | Described content in screenshots that wasn't there; "hallucinated Open Source Contributions section" | Better vision grounding; confidence scores for image analysis; request clarification on uncertain elements | 🟡 High | GitHub #19079 |
| **Self-Hallucinated User Input** | Mid-response inserted fake "###Human:" messages and responded to them; "self-instruction potential" | Better conversation boundary detection; prevent self-prompting; safety guardrails against fake turns | 🟡 High | GitHub #10628 |
| **Subagent Context Loss** | Resumed agents hallucinate "corrections" because they don't see original prompts; "BANANA-123" becomes "APPLE-123" | Store full conversation history including user prompts in subagent transcripts | 🟡 High | GitHub #11712 |

---

## 5. Permission System

| Issue | Description | Potential Solution | Urgency | Sources |
|-------|-------------|-------------------|---------|---------|
| **Excessive Permission Prompts** | "Asks permission for EVERYTHING"; "Can I edit this file?" every time breaks flow | Smarter permission memory; project-wide trust settings; remember per-command-pattern | 🔴 Critical | Builder.io, Arsturn, multiple |
| **Permission Amnesia** | "Yes, don't ask again" is ignored; system "gets amnesia & asks again next time" | Fix permission persistence bug, especially on macOS; verify settings file is written correctly | 🔴 Critical | GitHub, Arsturn |
| **Complex Command Breakdown** | System breaks `grep -r "pattern" . \| head -20` into parts and prompts for each | Treat piped commands as single unit; whitelist common patterns holistically | 🟡 High | Arsturn |
| **Dangerous-Skip Too Extreme** | Only options are "constant prompts" or "bypass ALL safety"; no middle ground | Implement tiered trust levels; "trust file operations but prompt for network/destructive" | 🟡 High | Multiple sources |
| **Git Commit Permission Bug** | Specifically ignores permissions for `git commit` even with correct config; "bug not user error" | Fix git commit permission recognition bug; investigate macOS-specific issues | 🟡 High | GitHub, Arsturn |

---

## 6. Windows & WSL Issues

| Issue | Description | Potential Solution | Urgency | Sources |
|-------|-------------|-------------------|---------|---------|
| **WSL Path Confusion** | Uses `/mnt/c/` paths on native Windows; UNC paths instead of Linux paths in WSL | Proper platform detection; context-aware path handling; fix WSL binary behavior | 🔴 Critical | GitHub #9580, #19653 |
| **WSL Startup Freezes** | Hangs on startup in WSL2 starting with v1.0.57+; "never launches the TUI" | Fix shell snapshot creation; investigate nvm/npm path conflicts; better WSL detection | 🔴 Critical | GitHub #9114, #4077 |
| **WSL Response Delays** | 3-4 minute delays in WSL2 with v2.0.74+; fixed by downgrading to 2.0.72 | Identify and revert breaking change; optimize WSL-specific code paths | 🟡 High | GitHub #16429 |
| **Clipboard Image Paste** | Ctrl+V with images doesn't work in WSL; no clear workaround | Implement PowerShell bridge for Windows clipboard access; document workarounds | 🟡 High | GitHub #13738 |
| **Claude in Chrome WSL** | "Native Host not supported on this platform" in WSL | Create WSL-Windows bridge for Chrome integration | 🟢 Medium | GitHub #14367 |

---

## 7. MCP Server Issues

| Issue | Description | Potential Solution | Urgency | Sources |
|-------|-------------|-------------------|---------|---------|
| **Silent Connection Failures** | MCP servers show "status: failed" with no error messages even in debug mode | Provide detailed error messages; log connection attempts; show timeout/auth/network issues | 🔴 Critical | GitHub #813, multiple |
| **Connection Instability** | "Claude Code's MCP integration is buggy - can't maintain stable connections" | Improve connection resilience; automatic reconnection; heartbeat monitoring | 🔴 Critical | GitHub #3279, #64 |
| **Windows npx Wrapper Required** | Local MCP servers need `cmd /c` wrapper on Windows; undocumented and confusing | Auto-detect Windows and apply wrapper; better documentation; clearer error messages | 🟡 High | Claude Code Docs |
| **OAuth Flow Interruptions** | OAuth authentication can't be cancelled; flows get stuck | Add Esc to cancel OAuth (implemented in recent versions); timeout handling | 🟡 High | CHANGELOG |
| **New Commands Not Detected** | MCP server commands added dynamically aren't picked up; must restart Claude Code | Implement proper `notifications/prompts/list_changed` handling; hot-reload support | 🟡 High | Arsturn |

---

## 8. IDE Integration

| Issue | Description | Potential Solution | Urgency | Sources |
|-------|-------------|-------------------|---------|---------|
| **Copy-Paste Workflow** | No seamless IDE integration like Cursor; "a lot of copying & pasting between editor & Claude interface" | Develop first-class IDE extensions; integrate with VSCode file system directly | 🟡 High | Arsturn, Cursor comparison |
| **JetBrains Esc Key Conflict** | Esc doesn't interrupt agent in JetBrains terminals due to keybinding clash | Document the fix (remove IdeaVim mapping); consider alternative interrupt key | 🟢 Medium | Claude Code Docs |
| **VSCode Extension Limitations** | Extension features not working; "plan mode, auto-accept, etc." issues | Better extension stability; version compatibility testing; clearer error reporting | 🟢 Medium | ClaudeLog |
| **Zed Integration Issues** | Sessions crash frequently; "had to go back to VSCode" | Improve Zed-specific stability; investigate terminal rendering issues | 🟢 Medium | Zed #43338 |

---

## 9. UX & Learning Curve

| Issue | Description | Potential Solution | Urgency | Sources |
|-------|-------------|-------------------|---------|---------|
| **Terminal-First Barrier** | CLI interface feels "like a step backward" for many developers; "steep learning curve" | Optional GUI wrapper; better onboarding; guided tutorials; visual command builder | 🟡 High | Hacker News, Builder.io |
| **Unintuitive Keyboard Shortcuts** | Shift+Enter doesn't work by default; Ctrl+V doesn't paste images (use Ctrl+V); Esc vs Ctrl+C confusion | Better defaults; comprehensive keybinding documentation; configurable shortcuts | 🟡 High | Builder.io |
| **Poor Changelog Discoverability** | Changelog is only a CHANGELOG.md on GitHub; "out of step with product quality" | Move changelog to docs site; add release dates; include visuals and examples | 🟢 Medium | GitHub #7109 |
| **File Drag Behavior** | Dragging files opens them in new tab instead of referencing; must hold Shift | Change default behavior; add visual indicator for reference vs open modes | 🟢 Medium | Builder.io |

---

## 10. Feature Gaps vs Competitors

| Feature Gap | Description | Competitor Reference | Urgency | Sources |
|-------------|-------------|---------------------|---------|---------|
| **No In-Editor Autocomplete** | Cursor offers real-time code completion; Claude Code has no equivalent | Cursor | 🟡 High | Multiple comparisons |
| **No Visual Diff UI** | Changes shown as text; Cursor has visual diff interface | Cursor | 🟡 High | Comparisons |
| **No Background Agents** | Cursor has Background Agents running in cloud VMs; Claude Code requires active terminal | Cursor | 🟡 High | Builder.io |
| **No Multi-Model Comparison** | Cursor can compare outputs from multiple models; Claude Code is single-model | Cursor | 🟢 Medium | Comparisons |
| **No Project Knowledge Base Integration** | Can't access Claude.ai Projects knowledge bases; 176 👍 on feature request | Claude.ai | 🟡 High | GitHub #2511 |
| **No VSCode LSP Integration** | Uses text-based grep/glob instead of semantic code navigation; "100-1000x slower" | VSCode native | 🟡 High | GitHub #5495 |

---

## 11. Reliability & Stability

| Issue | Description | Potential Solution | Urgency | Sources |
|-------|-------------|-------------------|---------|---------|
| **System Crashes (Windows)** | "Crashing my laptop"; BSOD with rapid green/red terminal output | Investigate Windows-specific memory/process issues; add safeguards | 🔴 Critical | GitHub #15291 |
| **Truncated Responses** | API responses truncated at specific character thresholds (4k, 6k, 8k, 16k chars) | Fix SDK stdout handling; implement proper chunked response assembly | 🟡 High | Kilocode #1224, Task Master |
| **Lock Acquisition Failures** | "NON-FATAL: Lock acquisition failed" errors in multi-process scenarios | Better mutex handling; graceful degradation; clearer user communication | 🟡 High | GitHub #19415 |
| **Antivirus/Cloud Sync Conflicts** | False "file modified" errors from antivirus/cloud sync tools (fixed in v2.1.7) | Continue improving file system monitoring; document common conflicts | 🟢 Medium | SmartScope |

---

## 12. Missing Capabilities

| Capability | User Request | Proposed Implementation | Urgency | Sources |
|------------|--------------|------------------------|---------|---------|
| **Proactive Memory Building** | System should suggest additions to CLAUDE.md based on patterns | Detect repeated commands/files; offer to save as patterns | 🟡 High | GitHub #4960 |
| **Dynamic Permission Sourcing** | Auto-allow commands from package.json scripts | Parse project config files for safe command lists | 🟡 High | GitHub #4907 |
| **Cost Visibility** | Real-time cost tracking during sessions | Show token usage and estimated cost after each interaction | 🟡 High | Multiple |
| **Better Test Integration** | TDD workflow improvements | Native test-first workflows; automatic test generation | 🟢 Medium | Best practices |
| **Offline Mode** | Work without internet when possible | Cache common operations; local-first architecture option | 🟢 Medium | Enterprise requests |

---

## 13. Documentation Issues

| Issue | Description | Potential Solution | Urgency | Sources |
|-------|-------------|-------------------|---------|---------|
| **Scattered Documentation** | Docs split between multiple sites; hard to find specific features | Unified documentation portal with good search | 🟡 High | Multiple |
| **Missing Release Dates** | CHANGELOG.md entries lack dates | Add dates to all changelog entries | 🟢 Medium | GitHub #7109 |
| **Sparse Examples** | Many features lack practical examples | Add "cookbook" with common use cases | 🟡 High | User feedback |
| **Troubleshooting Gaps** | Many error messages not documented | Comprehensive error message guide with solutions | 🟡 High | GitHub issues |

---

## Priority Summary Matrix

| Priority Level | Count | Top Items |
|----------------|-------|-----------|
| 🔴 **Critical** | 16 | Rate limit transparency, performance degradation, context display bugs, permission amnesia, WSL issues, hallucinations |
| 🟡 **High** | 29 | Pro tier limits, MCP stability, IDE integration, learning curve, competitor feature gaps |
| 🟢 **Medium** | 10 | Changelog improvements, Zed support, offline mode, documentation |

---

## Recommended v2 Focus Areas

### Tier 1: Foundational Fixes (Must Have)
1. **Fix rate limit transparency** - Show exact tokens, not vague "hours"
2. **Fix context window bugs** - Accurate display, smarter compaction
3. **Fix permission system** - Persistent memory, configurable trust levels
4. **Fix Windows/WSL issues** - Path handling, startup reliability
5. **Reduce hallucinations** - Verification steps, grounding in codebase

### Tier 2: Performance & Reliability
1. **Optimize speed** - Reduce redundant file reads, better caching
2. **Fix memory leaks** - Prevent degradation over long sessions
3. **Stabilize MCP** - Better error handling, connection resilience
4. **Improve cost tracking** - Real-time token/cost display

### Tier 3: Competitive Features
1. **IDE integration** - First-class VSCode extension with LSP
2. **Project knowledge base** - Connect to Claude.ai Projects
3. **Background agents** - Non-blocking autonomous tasks
4. **Better onboarding** - Guided setup, visual tools

---

## Appendix: Source Summary

| Source Type | Count | Examples |
|-------------|-------|----------|
| GitHub Issues | 40+ | #15291, #19415, #12897, #11335, #9114, #7824 |
| Reddit/Forums | 15+ | r/ClaudeAI, Hacker News, Cursor Forum |
| Technical Reviews | 10+ | Qodo, Northflank, Builder.io, Decode |
| Developer Blogs | 8+ | ksred.com, ClaudeLog, Medium |
| Documentation | 5+ | code.claude.com, docs.anthropic.com |

---

*Report compiled from extensive web research conducted January 2026*