#!/usr/bin/env bash
# check-api-contract.sh — the API-CONTRACT family guard.
#
# One script, one entry, invoked identically by the CI contract job and the
# api-contract blocking callback — only the --ci / --callback flag differs. Per
# the T518 rigorous-equality mandate the callback covers the ENTIRETY of the CI
# contract check. The flag is reserved for a future CI-vs-local tweak.
#
# The Rust↔TypeScript wire contract, end to end:
#   1. OpenAPI spec + route exhaustiveness — regenerates openapi.json from the
#      Rust types and asserts every route is represented
#      (cargo test -p cp-orchestrator --test openapi -- --ignored).
#   2. TS client codegen + drift — delegates to check-typescript-contract.sh
#      (regenerate hey-api client from openapi.json, git diff --exit-code the
#      committed generated/ tree; the config emits the @ts-nocheck header
#      natively so a clean regenerate is byte-identical, M145).
#   3. Manual-fetch audit — the api layer must use the generated SDK; the only
#      allowed raw fetch is downloadFile (binary) + the // ok:manual escape.
#
# Repo root via git so it is cwd-independent.
set -uo pipefail

MODE="${1:---ci}"   # --ci | --callback (behaviourally identical today)
ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"
fail=0

echo "=== OpenAPI spec + route exhaustiveness ==="
cargo test -p cp-orchestrator --test openapi -- --ignored 2>&1 || fail=1

echo "=== TS client codegen + drift ==="
bash "$ROOT/.github/checks/check-typescript-contract.sh" || fail=1

echo "=== manual fetch() audit (api layer must use the generated SDK) ==="
# The api layer must call the generated SDK. The only acceptable raw fetch is a
# site explicitly marked `// ok:manual` — Prettier reflows multi-arg fetch()
# calls so the marker legitimately lands on the LINE AFTER `fetch(` (e.g.
# downloadFile's binary-blob GET), which a per-line grep -v would miss and
# false-flag. So the audit tolerates the marker on the fetch line OR the next
# line: a `fetch(` occurrence is only reported when NEITHER carries `ok:manual`.
# generated/ is excluded (codegen owns its own transport).
MANUAL_FETCH=$(find "$ROOT/web/src/lib/api" -type f -name '*.ts' -not -path '*/generated/*' -print0 \
  | xargs -0 awk '
      pending {
        if ($0 !~ /ok:manual/) print pfile ":" pno ": " ptext
        pending = 0
      }
      /fetch\(/ && $0 !~ /ok:manual/ {
        pending = 1; pfile = FILENAME; pno = FNR; ptext = $0
      }
      END { if (pending) print pfile ":" pno ": " ptext }
    ')
if [ -n "$MANUAL_FETCH" ]; then
  echo "::error::Manual fetch() calls found in API layer — use the generated SDK (or mark // ok:manual):"
  echo "$MANUAL_FETCH"
  fail=1
fi

if [ "$fail" -eq 0 ]; then
  echo "check-api-contract OK ($MODE): openapi · ts-drift · manual-fetch ✓"
fi
exit "$fail"
