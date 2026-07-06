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

# The hey-api runtime core files are vendored verbatim and are not
# exactOptionalPropertyTypes-clean under the maximal-strict tsconfig
# (web-lint P1). Generated bindings are a contract BOUNDARY: our
# hand-written lib/api consumers type-check against the exported types,
# but the generated internals are excluded from strict internal checking —
# the exact analogue of a relaxed lint profile over Rust generated bindings.
# Re-stamp a `// @ts-nocheck` header onto every generated file after each
# regeneration so the committed tree and a fresh codegen stay byte-identical
# (the drift check below compares them).
echo "▸ Stamping @ts-nocheck onto generated bindings…"
GEN_HEADER='// @ts-nocheck — hey-api generated client. Type-checked at the contract boundary (our hand-written lib/api consumers validate against the exported types); the vendored runtime internals are not exactOptionalPropertyTypes-clean and regenerate verbatim, so they are excluded from strict internal checking (web-lint P1, generated-bindings exception).'
while IFS= read -r gen_file; do
  if ! head -1 "$gen_file" | grep -q "@ts-nocheck"; then
    printf '%s\n%s' "$GEN_HEADER" "$(cat "$gen_file")" > "$gen_file"
  fi
done < <(find src/lib/api/generated -name '*.ts')

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
