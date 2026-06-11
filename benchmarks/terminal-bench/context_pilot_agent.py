"""Harbor installed-agent adapter for Context Pilot.

Run with:
    harbor run -d terminal-bench@2.0 \
        --agent-import-path benchmarks.terminal-bench.context_pilot_agent:ContextPilotAgent \
        -m anthropic/claude-sonnet-4-6 -k 5

Context Pilot runs headless inside the task container via `tui --headless`
(see HEADLESS_DESIGN.md). The adapter:
  - install():   downloads the CP release bundle (tui + cp-console-server +
                 meilisearch) and lays the binaries out where CP's runtime
                 discovery expects them.
  - run():       executes `tui --headless <instruction>` with the env-only
                 claude_code_api_key provider, writing a JSONL trajectory.
  - populate_context_post_run(): parses the trajectory's final event into the
                 Harbor AgentContext (tokens + cost).

MODEL NOTE: the leaderboard model is `anthropic/claude-sonnet-4-6`, but Sonnet
4.6 is only reachable through CP's OAuth (ClaudeCodeV2) provider. With a plain
ANTHROPIC_API_KEY in the container, CP's `claude_code_api_key` provider tops out
at Sonnet 4.5, so an unrecognised `--model` (e.g. claude-sonnet-4-6) degrades
gracefully to Sonnet 4.5. Resolve before final submission (X525/X526): either
switch the container to OAuth for true 4.6, or label the run as Sonnet 4.5.
"""

import json
import shlex
from pathlib import Path

from harbor.agents.installed.base import BaseInstalledAgent, with_prompt_template
from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext

# Release bundle URL (linux-x86_64 build from CI). Must contain, at the tarball
# root: `tui`, `cp-console-server`, and `meilisearch` (all executable).
# TODO(X525): pin a release tag instead of `latest`.
BUNDLE_URL = (
    "https://github.com/bigmoostache/context-pilot/releases/latest/download/"
    "context-pilot-linux-x86_64.tar.gz"
)

# CP runtime layout (see crates/cp-mod-console manager.rs + cp-mod-search meili
# server.rs). tui + cp-console-server must be co-located (next-to-exe discovery);
# meilisearch must live at ~/.context-pilot/meilisearch/bin/meilisearch.
CP_HOME = "$HOME/.context-pilot"
CP_BIN = f"{CP_HOME}/bin"
CP_MEILI_BIN = f"{CP_HOME}/meilisearch/bin"

# Trajectory path inside the container (JSONL, one event per line).
TRAJECTORY_PATH = "/tmp/context-pilot-trajectory.jsonl"


class ContextPilotAgent(BaseInstalledAgent):
    """Context Pilot running headless inside the task container."""

    @staticmethod
    def name() -> str:
        return "context-pilot"

    def version(self) -> str | None:
        # tui has no --version flag, so report a static version rather than
        # overriding get_version_command() with a call that would fail.
        return "0.1.0"  # TODO(X525): sync with the pinned release tag.

    async def install(self, environment: BaseEnvironment) -> None:
        # System deps needed only to fetch + unpack the bundle.
        await self.exec_as_root(
            environment,
            command="apt-get update && apt-get install -y curl ca-certificates",
            env={"DEBIAN_FRONTEND": "noninteractive"},
        )
        # Download + unpack the Context Pilot bundle as the agent user, then lay
        # the binaries out for CP's discovery:
        #   tui, cp-console-server -> ~/.context-pilot/bin/   (co-located)
        #   meilisearch            -> ~/.context-pilot/meilisearch/bin/
        # The meilisearch move is best-effort: if the bundle omits it, CP
        # auto-downloads the pinned build on first boot (needs container
        # internet; meilisearch's own GitHub, leaderboard-compliant).
        await self.exec_as_agent(
            environment,
            command=(
                "set -euo pipefail; "
                f"mkdir -p {CP_BIN} {CP_MEILI_BIN}; "
                f"curl -fsSL {shlex.quote(BUNDLE_URL)} -o /tmp/cp.tar.gz; "
                f"tar -xzf /tmp/cp.tar.gz -C {CP_BIN}; "
                f"chmod +x {CP_BIN}/tui {CP_BIN}/cp-console-server 2>/dev/null || true; "
                # Relocate a bundled meilisearch to where CP expects it.
                f"if [ -f {CP_BIN}/meilisearch ]; then "
                f"  mv {CP_BIN}/meilisearch {CP_MEILI_BIN}/meilisearch; "
                f"  chmod +x {CP_MEILI_BIN}/meilisearch; "
                "fi"
            ),
        )

    def _resolve_model(self) -> str:
        """CP `--model` value derived from Harbor's `-m provider/model`.

        Strips the provider prefix (`anthropic/claude-sonnet-4-6` ->
        `claude-sonnet-4-6`). Unrecognised models degrade to Sonnet 4.5 inside
        CP. Falls back to claude-sonnet-4-5 when Harbor supplies no model.
        """
        model_name = getattr(self, "model_name", None)
        if not model_name:
            return "claude-sonnet-4-5"
        return model_name.split("/")[-1]

    @with_prompt_template
    async def run(
        self, instruction: str, environment: BaseEnvironment, context: AgentContext
    ) -> None:
        model = self._resolve_model()

        # Pass the API key through explicitly (Harbor also merges it into env,
        # but being explicit avoids relying on inheritance details).
        run_env: dict[str, str] = {}
        api_key = self._get_env("ANTHROPIC_API_KEY")
        if api_key:
            run_env["ANTHROPIC_API_KEY"] = api_key

        # Optional guard-rail overrides for dry-runs (X524). When unset, CP's
        # built-in defaults apply ($5 / 150 messages).
        flags = [
            "--headless",
            shlex.quote(instruction),
            "--provider",
            "claude_code_api_key",  # env-only auth, no Keychain/OAuth
            "--model",
            shlex.quote(model),
            "--trajectory",
            TRAJECTORY_PATH,
        ]
        max_cost = self._get_env("CP_MAX_COST")
        if max_cost:
            flags += ["--max-cost", shlex.quote(max_cost)]
        max_messages = self._get_env("CP_MAX_MESSAGES")
        if max_messages:
            flags += ["--max-messages", shlex.quote(max_messages)]

        # `|| true`: CP exits 2 on guard-rail stop and 1 on error. Those are
        # legitimate "ran but didn't fully finish" outcomes — the verifier
        # scores the filesystem regardless — so we must NOT let exec_as_agent's
        # non-zero-exit handling raise and mark the trial as errored. The
        # trajectory's final event records the true status.
        command = f"{CP_BIN}/tui " + " ".join(flags) + " || true"

        await self.exec_as_agent(environment, command=command, env=run_env)

    def populate_context_post_run(self, context: AgentContext) -> None:
        """Parse the trajectory's `final` event into the AgentContext."""
        traj = Path(TRAJECTORY_PATH)
        try:
            lines = traj.read_text(encoding="utf-8").splitlines()
        except OSError:
            return  # No trajectory (install/run failed earlier) — leave empty.

        final_event: dict | None = None
        n_assistant = 0
        for line in lines:
            line = line.strip()
            if not line:
                continue
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            kind = event.get("event")
            if kind == "assistant":
                n_assistant += 1
            elif kind == "final":
                final_event = event

        if final_event is None:
            return

        cost = final_event.get("total_cost_usd")
        if cost is not None:
            context.cost_usd = float(cost)
        out_tokens = final_event.get("output_tokens")
        if out_tokens is not None:
            context.n_output_tokens = int(out_tokens)

        # Stash CP-specific run stats Harbor doesn't model natively.
        context.metadata = {
            "status": final_event.get("status"),
            "messages": final_event.get("messages"),
            "assistant_turns": n_assistant,
            "duration_secs": final_event.get("duration_secs"),
        }
