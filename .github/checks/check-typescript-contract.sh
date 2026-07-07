#!/usr/bin/env bash
# CI guard: regenerate openapi.json + TypeScript client, fail on drift.
#
# Any change to a Rust type or endpoint that isn't accompanied by a
# matching regeneration of the OpenAPI spec and TypeScript client will
# cause `git diff --exit-code` to fail — catching contract drift before
# it reaches production.
#
# Usage: .github/checks/check-typescript-contract.sh

set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

echo "▸ Regenerating openapi.json from Rust…"
cargo test -p cp-orchestrator --test openapi generate_openapi -- --ignored --quiet 2>&1

echo "▸ Regenerating TypeScript client from openapi.json…"
cd "$ROOT/web"
# Codegen is driven by web/openapi-ts.config.ts, whose `output.header` emits the
# `// @ts-nocheck` generated-bindings header NATIVELY into every file (web-lint
# P1). No post-codegen stamping step: the committed tree is byte-identical to a
# plain config-driven run, so the drift check below is a clean regenerate +
# `git diff --exit-code`. The @ts-nocheck excludes the vendored runtime
# internals (not exactOptionalPropertyTypes-clean) from strict internal
# checking, while our hand-written lib/api consumers still type-check against the
# exported types — the exact analogue of a relaxed lint profile over Rust
# generated bindings.
npx @hey-api/openapi-ts -f openapi-ts.config.ts 2>&1

echo "▸ Checking for uncommitted drift…"
cd "$ROOT"
if ! git diff --exit-code -- openapi.json web/src/lib/api/generated/; then
  echo ""
  echo "::error::Contract drift detected!"
  echo "The committed openapi.json or web/src/lib/api/generated/ files are"
  echo "out of sync with the Rust backend types. Run:"
  echo ""
  echo "  cargo test -p cp-orchestrator --test openapi generate_openapi -- --ignored"
  echo "  cd web && npx @hey-api/openapi-ts -f openapi-ts.config.ts"
  echo "  git add openapi.json web/src/lib/api/generated/"
  echo ""
  exit 1
fi

echo "✓ TypeScript contract is in sync with backend."
