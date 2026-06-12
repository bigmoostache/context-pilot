---
name: sumup
description: Summarize sequential test runs as a table (Task, Description, Run Id, Reward, N_turns)
---
Sum up, as markdown tables, the terminal-bench task runs from the clean sequential batches under `benchmarks/terminal-bench/jobs/`. Include TWO batches:

1. **Yesterday evening** — job dir `2026-06-11__21-48-44` (v0.2.3 batch).
2. **Tonight's v0.2.10 validation** — job dirs from `2026-06-12__08-42-41` onward (began with `log-summary-date-ranges`).

Gather the data fresh from the job directories (do NOT rely on memory) — for each run, read:
- the task name (from the `<task>__<id>` job subdir)
- a one-line Description of what the task asks for
- the Run Id (the `<id>` suffix on the task dir, e.g. `qVTujKm`)
- the Reward (`verifier/reward.txt`; show "n/a" if unscored / still running)
- N_turns = count of `assistant` events in `agent/context-pilot-trajectory.jsonl`

Output ONE table PER batch (label each with its date/batch), each with EXACTLY these columns, one row per run, in chronological order:

| Task | Description | Run Id | Reward | N_turns |

After each table, add a one-line tally (e.g. "3/4 scored passed, mean reward X.XX"). Then end with a combined grand-total tally across both batches.