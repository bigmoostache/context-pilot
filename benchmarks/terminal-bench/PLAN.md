# Terminal-Bench 2.0 Submission Plan

Goal: get **Context Pilot** on the [Terminal-Bench 2.0 leaderboard](https://www.tbench.ai/leaderboard/terminal-bench/2.0).

## Benchmark facts (researched 2026-06-11)

- Terminal-Bench 2.0 is a benchmark for terminal agents. **[Harbor](https://www.harborframework.com) is the official harness** — submissions must run `terminal-bench@2.0` via Harbor.
- Run command for a custom agent:
  ```
  harbor run -d terminal-bench@2.0 --agent-import-path path.to.agent:ContextPilotAgent -k 5
  ```
- `-k 5`: each task must be evaluated with a **minimum of five trials**.
- Requires Docker running locally (or Daytona cloud sandboxes via `DAYTONA_API_KEY` + `--env daytona -n 32`).
- Harness sanity check: `harbor run -d terminal-bench/terminal-bench-2 -a oracle`.

### Leaderboard validation rules (auto-checked by bot)

- `timeout_multiplier` must equal `1.0`; **no** agent/verifier timeout overrides, **no** resource overrides (cpus/memory/storage).
- Every trial directory needs a valid `result.json` **plus** the other run artifacts.
- **Agents may not access the Terminal-Bench website or GitHub repo** (reward-hacking rule) — keep this in mind for CP's web tools (brave/firecrawl must not hit tbench.ai / the TB GitHub).

### Submission process — ⚠️ currently CLOSED

- The HF repo [`harborframework/terminal-bench-2-leaderboard`](https://huggingface.co/datasets/harborframework/terminal-bench-2-leaderboard) shows **SUBMISSIONS CLOSED** since May 14.
- A **new process is expected "by end of June"** enforcing the [leaderboard-integrity policies](https://www.tbench.ai/news/leaderboard-integrity-update).
- **Existing job files/trajectories will remain usable** in the new process → we can run now, submit when the window reopens.
- Old format (likely similar going forward): fork the HF repo, PR adding
  ```
  submissions/terminal-bench/2.0/<agent>__<model>/
    metadata.yaml        # agent_url, display names, models list
    <job-folder>/
      config.json
      <trial-1>/result.json
      ...
  ```

## Architecture decisions (confirmed with captain, 2026-06-11)

| Decision | Choice |
|---|---|
| Model | **claude-sonnet-4-6** via plain `ANTHROPIC_API_KEY` (no OAuth/Keychain in container) |
| Headless mode | **`tui --headless "<instruction>"` flag** on the existing binary |
| Run environment | **Local Docker**, low concurrency, overnight runs |
| Daemons | **Bundle everything** (Meilisearch + cp-console-server) — benchmark the full-capability agent |

## Integration: Harbor *installed agent*

Context Pilot integrates as a `BaseInstalledAgent` (installed into the task container, executed headless — how most agents integrate):

```python
class ContextPilotAgent(BaseInstalledAgent):
    async def install(self, environment):   # exec_as_root / exec_as_agent
        ...  # download linux-x86_64 CP bundle, install daemons
    @with_prompt_template
    async def run(self, instruction, environment, context):
        ...  # exec_as_agent: tui --headless '<instruction>'
    def populate_context_post_run(self, context):
        ...  # parse trajectory/logs into AgentContext
```

Adapter lives in `benchmarks/terminal-bench/` (this repo).

## Work plan

1. **Headless mode** (`--headless`) — instruction in, autonomous loop (auto-continuation until done / guard rails), no rendering, exit 0, trajectory written to a known path. **Design validated → [HEADLESS_DESIGN.md](HEADLESS_DESIGN.md)**
2. **Container-ready boot** — bare-Linux friendly: bundled daemons, env-only API keys, no Keychain, no global config.
3. **Harbor adapter** — `context_pilot_agent.py` + install script.
4. **Oracle sanity run** — validate Harbor + Docker locally.
5. **Dry-run** — a few tasks end-to-end, iterate.
6. **Official run** — `-k 5`, Sonnet 4.6, local Docker, keep all artifacts.
7. **Submit** when the window reopens (~end of June).
