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

# Serialize concurrent invocations with a portable mkdir spin-lock. The
# TS-codegen step regenerates the hey-api client into a SINGLE shared output dir
# (web/src/lib/api/generated), and hey-api WIPES that dir before rewriting it. A
# commit touching BOTH a .rs and a web .ts fires the api-contract-rust AND
# api-contract-ts callbacks in parallel — two concurrent regenerations of the
# same dir race, one deleting a core file the other's replaceImports is reading
# ("ENOENT … core/auth.gen.ts"). The lock makes the second invocation wait, then
# regenerate cleanly (idempotent: same openapi.json ⇒ byte-identical output, no
# drift). CI is a single serial invocation, so the lock is uncontended there.
#
# `mkdir` (not flock) because flock is util-linux — ABSENT on the macOS dev box
# where the callbacks actually run and the race actually bites (flock's presence
# check silently skipped the guard there). `mkdir` is an atomic test-and-create
# on every POSIX filesystem, so it is a true portable mutex. An EXIT trap removes
# the lockdir on any normal/interrupted termination; a stale lock from a hard
# kill is force-broken after LOCK_TIMEOUT so a crashed run can't wedge the guard.
LOCK_DIR="$ROOT/.git/cp-api-contract.lock.d"
LOCK_TIMEOUT=180
waited=0
until mkdir "$LOCK_DIR" 2>/dev/null; do
  # Break a stale lock whose owning run died without cleanup (older than the
  # timeout) so the guard self-heals instead of spinning forever.
  if [ -d "$LOCK_DIR" ]; then
    age=$(( $(date +%s) - $(stat -f %m "$LOCK_DIR" 2>/dev/null || stat -c %Y "$LOCK_DIR" 2>/dev/null || date +%s) ))
    if [ "$age" -ge "$LOCK_TIMEOUT" ]; then
      rmdir "$LOCK_DIR" 2>/dev/null || true
      continue
    fi
  fi
  sleep 0.3
  waited=$((waited + 1))
  [ "$waited" -ge $((LOCK_TIMEOUT * 4)) ] && break
done
trap 'rmdir "$LOCK_DIR" 2>/dev/null || true' EXIT

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
