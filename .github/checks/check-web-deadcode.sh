#!/usr/bin/env bash
# check-web-deadcode.sh — cross-file dead-code guard for the web frontend.
#
# The knip pass: the ONE thing tsc + eslint structurally cannot catch. tsc sees
# only WITHIN a file (noUnusedLocals/Parameters), so a dead EXPORT, an orphan
# FILE never imported, or an unused package.json DEPENDENCY slips through. knip
# walks the whole module graph from the entry points in web/knip.json and fails
# on any of the three. This is the frontend twin of Rust's inter-crate
# dead_code (warn->forbid) reach.
#
# WHOLE-GRAPH, not incremental: knip must see every file to prove an export is
# dead, so — unlike the per-changed-file web-lint pass — there is NO file-scoped
# mode. The script takes no arguments and runs the identical full scan wherever
# it is invoked. That is deliberate: the CI job step and the blocking callback
# call this file with the EXACT same string (`bash .github/checks/check-web-deadcode.sh`),
# so the two are structurally equal by construction — there is only one
# implementation of the check, in one place, hash-locked in protected-files.yaml.
#
# Self-locating: resolves the repo root via git and cd's into web/ itself, so it
# is cwd-independent — neither caller needs to set a working directory. Requires
# web/node_modules (CI installs it; the callback runs on the dev machine where
# it is already present).
set -euo pipefail

cd "$(git rev-parse --show-toplevel)/web"

echo "check-web-deadcode: knip (dead files / exports / deps)…"
npx knip
