"""Harbor installed-agent adapter for Context Pilot.

Run with:
    harbor run -d terminal-bench@2.0 \
        --agent-import-path benchmarks/terminal-bench/context_pilot_agent.py:ContextPilotAgent \
        -m anthropic/claude-sonnet-4-6 -k 5

Context Pilot runs headless inside the task container via `tui --headless`
(see HEADLESS_DESIGN.md). The adapter:
  - install():   downloads the CP release bundle (tui + cp-console-server +
                 meilisearch) and lays the binaries out where CP's runtime
                 discovery expects them. If CLAUDE_CREDENTIALS_JSON is set it
                 also writes ~/.claude/.credentials.json so the OAuth providers
                 (claude_code / claude_code_v2) can authenticate without the
                 macOS Keychain.
  - run():       executes `tui --headless <instruction>` with the provider
                 selected by CP_PROVIDER (default: claude_code_api_key for
                 submission; set to claude_code_v2 for OAuth-based local tests).
  - populate_context_post_run(): parses the trajectory's final event into the
                 Harbor AgentContext (tokens + cost).

PROVIDER / MODEL NOTES:
  Submission:  CP_PROVIDER=claude_code_api_key  ANTHROPIC_API_KEY=<key>
               → Sonnet 4.5 (API-key provider tops out here)
  Local test:  CP_PROVIDER=claude_code_v2  CLAUDE_CREDENTIALS_JSON=<json>
               → Sonnet 4.6 via OAuth (same model as leaderboard metadata)

ENV VAR OVERRIDES (for local dev / CI):
  CP_BUNDLE_URL           - override the tarball download URL
  CP_PROVIDER             - override the LLM provider (default: claude_code_api_key)
  CLAUDE_CREDENTIALS_JSON - full ~/.claude/.credentials.json JSON blob for OAuth
"""

import json
import os as _os
import shlex
from pathlib import Path

from harbor.agents.installed.base import BaseInstalledAgent, with_prompt_template
from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext

# Release bundle URL (linux-x86_64 build from CI). Must contain, at the tarball
# root: `tui`, `cp-console-server`, and `meilisearch` (all executable).
# Override via CP_BUNDLE_URL env var for local testing without a real release.
# TODO(X525): pin a release tag instead of `latest`.
BUNDLE_URL = _os.environ.get(
    "CP_BUNDLE_URL",
    "https://github.com/bigmoostache/context-pilot/releases/latest/download/"
    "context-pilot-linux-x86_64.tar.gz",
)

# CP runtime layout (see crates/cp-mod-console manager.rs + cp-mod-search meili
# server.rs). tui + cp-console-server must be co-located (next-to-exe discovery);
# meilisearch must live at ~/.context-pilot/meilisearch/bin/meilisearch.
CP_HOME = "$HOME/.context-pilot"
CP_BIN = f"{CP_HOME}/bin"
CP_MEILI_BIN = f"{CP_HOME}/meilisearch/bin"

# Harbor mounts /logs/agent in the container as a bind mount to the host-side
# trial_dir/agent — exactly the path passed to the agent as self.logs_dir
# (harbor trial.py: logs_dir=self.paths.agent_dir). Writing here means the
# trajectory + stdout are persisted to the trial directory automatically (no
# /logs/artifacts copy needed) and are readable host-side in
# populate_context_post_run via self.logs_dir.
CP_LOG_DIR = "/logs/agent"
# Trajectory path inside the container (JSONL, one event per line).
TRAJECTORY_PATH = f"{CP_LOG_DIR}/context-pilot-trajectory.jsonl"
# CP's captured stdout/stderr — invaluable for diagnosing instant-exit failures
# (e.g. the GLIBC load error) where no trajectory is produced.
STDOUT_LOG = f"{CP_LOG_DIR}/cp-stdout.log"
# Filename of the trajectory as seen host-side, relative to self.logs_dir.
TRAJECTORY_FILENAME = "context-pilot-trajectory.jsonl"


class ContextPilotAgent(BaseInstalledAgent):
    """Context Pilot running headless inside the task container."""

    @staticmethod
    def name() -> str:
        return "context-pilot"

    def version(self) -> str | None:
        # tui has no --version flag, so report a static version rather than
        # overriding get_version_command() with a call that would fail.
        return "0.2.4"  # TODO(X525): sync with the pinned release tag.

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
        # If OAuth credentials are provided, write them to the standard
        # credentials file so CP's claude_code / claude_code_v2 providers can
        # authenticate without the macOS Keychain.
        creds_json = self._get_env("CLAUDE_CREDENTIALS_JSON")
        if creds_json:
            escaped = shlex.quote(creds_json)
            await self.exec_as_agent(
                environment,
                command=(
                    "mkdir -p ~/.claude && "
                    f"echo {escaped} > ~/.claude/.credentials.json && "
                    "chmod 600 ~/.claude/.credentials.json"
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

        # Provider selection — defaults to the env-only API-key provider for
        # submission. Override with CP_PROVIDER=claude_code_v2 for OAuth tests.
        provider = self._get_env("CP_PROVIDER") or "claude_code_api_key"

        # Pass auth credentials through explicitly.
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
            provider,
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

        # `set +e` + `tee`: CP exits 2 on guard-rail stop and 1 on error. Those
        # are legitimate "ran but didn't fully finish" outcomes — the verifier
        # scores the filesystem regardless — so we must NOT let exec_as_agent's
        # non-zero-exit handling raise and mark the trial as errored. We capture
        # CP's stdout+stderr into /logs/agent/cp-stdout.log (persisted to the
        # trial dir) and record its true exit code there for diagnosis, then
        # exit 0 so the trial proceeds to verification. The trajectory's final
        # event records the true status.
        cp_cmd = f"{CP_BIN}/tui " + " ".join(flags)
        command = (
            f"mkdir -p {CP_LOG_DIR}; "
            "set +e; "
            f"{cp_cmd} 2>&1 | tee {STDOUT_LOG}; "
            'cp_exit="${PIPESTATUS[0]}"; '
            f'echo "[cp-headless] tui exit=$cp_exit" >> {STDOUT_LOG}; '
            "true"
        )

        await self.exec_as_agent(environment, command=command, env=run_env)

    def populate_context_post_run(self, context: AgentContext) -> None:
        """Parse the trajectory's `final` event into the AgentContext.

        The trajectory is written inside the container to /logs/agent/, which
        Harbor bind-mounts to the host-side trial agent dir == self.logs_dir.
        We read it from there (NOT the container path, which is gone post-run).
        """
        traj = Path(self.logs_dir) / TRAJECTORY_FILENAME
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
