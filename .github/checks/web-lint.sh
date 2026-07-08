#!/usr/bin/env bash
# web-lint.sh — incremental frontend lint gate (the edit-time twin of the CI
# frontend job), the CB8 web-lint callback delegate.
#
# Runs on every web/src edit: eslint --max-warnings 0 + prettier --check over
# ONLY the changed .ts/.tsx files (excluding the generated OpenAPI bindings).
# eslint still loads the full type-aware projectService, so cross-file type
# rules stay correct on a single-file run. Project-wide tsc + type-coverage are
# too slow per-edit and live CI-side instead (.github/workflows/ci.yml frontend
# job).
#
# Standalone in .github/checks/ (like every other guard) so the executed logic
# is hash-locked in protected-files.yaml, not carried inline in the mutable
# callback registry. The CB8 script_content is a one-line delegate:
#   bash "$CP_PROJECT_ROOT/.github/checks/web-lint.sh"
#
# MUST run with cwd = web/ (the callback sets it) so npx resolves web's
# eslint/prettier + local config. Change-file paths arrive in $CP_CHANGED_FILES
# in either repo-relative (web/src/…) or absolute (/…/web/src/…) form; both are
# normalised to a web-relative path here.
set -uo pipefail

files=()
for f in $CP_CHANGED_FILES; do
  rel="${f#web/}"        # repo-relative "web/src/x" -> "src/x"
  rel="${rel##*/web/}"   # absolute "/a/web/src/x" -> "src/x"
  case "$rel" in
    *generated/*) continue ;;               # codegen bindings: never linted
    src/*.ts | src/*.tsx) files+=("$rel") ;;
  esac
done

[ ${#files[@]} -eq 0 ] && { echo "web-lint: no web/src ts/tsx changes"; exit 0; }

fail=0
echo "web-lint: eslint ${#files[@]} file(s)…"
npx eslint --max-warnings 0 "${files[@]}" || fail=1
echo "web-lint: prettier --check…"
npx prettier --check "${files[@]}" || fail=1

if [ "$fail" -ne 0 ]; then
  echo "" >&2
  echo "web-lint FAILED — fix the eslint/prettier violations above." >&2
  echo "(full project gate: eslint . / tsc -b / type-coverage run in CI)" >&2
fi
exit $fail
