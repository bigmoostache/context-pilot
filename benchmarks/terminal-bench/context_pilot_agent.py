"""Harbor installed-agent adapter for Context Pilot.

Run with:
    harbor run -d terminal-bench@2.0 \
        --agent-import-path benchmarks.terminal-bench.context_pilot_agent:ContextPilotAgent \
        -m anthropic/claude-sonnet-4-6 -k 5

Status: SKELETON — headless mode (`tui --headless`) not yet implemented in the
Rust binary. See PLAN.md for the work plan.
"""

import shlex

from harbor.agents.installed.base import BaseInstalledAgent, with_prompt_template
from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext

# Release bundle URL (linux-x86_64 build from CI). TODO: pin a release tag.
BUNDLE_URL = (
    "https://github.com/bigmoostache/context-pilot/releases/latest/download/"
    "context-pilot-linux-x86_64.tar.gz"
)

TRAJECTORY_PATH = "/tmp/context-pilot-trajectory.jsonl"


class ContextPilotAgent(BaseInstalledAgent):
    """Context Pilot running headless inside the task container."""

    @staticmethod
    def name() -> str:
        return "context-pilot"

    def version(self) -> str | None:
        return "0.1.0"  # TODO: sync with release tag

    async def install(self, environment: BaseEnvironment) -> None:
        # System deps for the CP bundle (binary + daemons).
        await self.exec_as_root(
            environment,
            command="apt-get update && apt-get install -y curl ca-certificates",
        )
        # Download + unpack the Context Pilot bundle:
        # tui binary, cp-console-server, meilisearch. TODO: build install.sh
        # that lays these out and writes a minimal headless config.
        await self.exec_as_agent(
            environment,
            command=(
                f"curl -fsSL {BUNDLE_URL} -o /tmp/cp.tar.gz"
                " && mkdir -p ~/.context-pilot/bin"
                " && tar -xzf /tmp/cp.tar.gz -C ~/.context-pilot/bin"
            ),
        )

    @with_prompt_template
    async def run(
        self, instruction: str, environment: BaseEnvironment, context: AgentContext
    ) -> None:
        # ANTHROPIC_API_KEY is merged into the env by Harbor.
        await self.exec_as_agent(
            environment,
            command=(
                "~/.context-pilot/bin/tui --headless "
                f"{shlex.quote(instruction)} "
                f"--trajectory {TRAJECTORY_PATH}"
            ),
        )

    def populate_context_post_run(self, context: AgentContext) -> None:
        # TODO: parse TRAJECTORY_PATH (token usage, n turns, final status)
        # once the headless trajectory format exists.
        pass
