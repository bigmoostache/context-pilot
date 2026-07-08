#!/usr/bin/env bash
# check-ts-tests.sh — the TS-TESTS family guard.
#
# One script, one entry, invoked identically by the CI frontend job and the
# ts-tests blocking callback — only the --ci / --callback flag differs.
#
# STATUS: STUB. There is no frontend test suite wired yet.
#
# TODO(playwright): wire the Playwright e2e suite here —
#     cd "$ROOT/web" && npx playwright test
#   The suite exists (web/playwright.config.ts) but drives the LIVE stack (web
#   :5175 + orchestrator :7878 + this agent's bridge) and MUTATES the single
#   live agent's roster, so it cannot run unattended in CI or on every edit
#   without a dedicated ephemeral stack. Until that stack is provisioned this
#   family is a documented no-op so the 6-family wiring stays uniform (one
#   script per family, CI + callback both call it) instead of leaving a hole.
set -uo pipefail

MODE="${1:---ci}"   # --ci | --callback

echo "check-ts-tests ($MODE): no frontend test suite wired yet (see TODO(playwright)). Skipping."
exit 0
