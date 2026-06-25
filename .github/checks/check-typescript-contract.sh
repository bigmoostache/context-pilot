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
npx @hey-api/openapi-ts -i ../openapi.json -o src/lib/api/generated 2>&1

echo "▸ Checking for uncommitted drift…"
cd "$ROOT"
if ! git diff --exit-code -- openapi.json web/src/lib/api/generated/; then
  echo ""
  echo "::error::Contract drift detected!"
  echo "The committed openapi.json or web/src/lib/api/generated/ files are"
  echo "out of sync with the Rust backend types. Run:"
  echo ""
  echo "  cargo test -p cp-orchestrator --test openapi generate_openapi -- --ignored"
  echo "  cd web && npx @hey-api/openapi-ts -i ../openapi.json -o src/lib/api/generated"
  echo "  git add openapi.json web/src/lib/api/generated/"
  echo ""
  exit 1
fi

echo "✓ TypeScript contract is in sync with backend."
