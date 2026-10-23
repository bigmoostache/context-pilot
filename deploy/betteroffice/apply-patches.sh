#!/usr/bin/env bash
# apply-patches.sh: produce a patched BetterOffice build tree.
#
# vendor/betteroffice is a git submodule pinned at the upstream commit we build
# from. Our changes live as ordered .diff files in web/vendor/betteroffice/
# (0001-*.diff, 0002-*.diff, ...). They are never applied inside the submodule
# itself, which stays clean; instead this script exports the pinned tree to a
# build directory and applies every patch there, in order.
#
# The same patched tree feeds both the browser packages (WASM + .tgz) and the
# native cp-office-engine, so browser and engine always build identical bases.
#
# Usage: deploy/betteroffice/apply-patches.sh [OUT_DIR]
#   OUT_DIR defaults to target/betteroffice (gitignored). It is wiped first.
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
SUB="$ROOT/vendor/betteroffice"
PATCHES="$ROOT/web/vendor/betteroffice"
OUT="${1:-$ROOT/target/betteroffice}"

if [ ! -e "$SUB/.git" ]; then
  echo "vendor/betteroffice missing: run 'git submodule update --init vendor/betteroffice'" >&2
  exit 1
fi

pin="$(git -C "$SUB" rev-parse HEAD)"
expected="$(git -C "$ROOT" ls-tree HEAD vendor/betteroffice | awk '{print $3}')"
if [ -n "$expected" ] && [ "$pin" != "$expected" ]; then
  echo "vendor/betteroffice is at $pin but the repo pins $expected" >&2
  echo "run 'git submodule update vendor/betteroffice'" >&2
  exit 1
fi

rm -rf "$OUT"
mkdir -p "$OUT"
git -C "$SUB" archive --format=tar "$pin" | tar -x -C "$OUT"

shopt -s nullglob
count=0
for patch in "$PATCHES"/[0-9][0-9][0-9][0-9]-*.diff; do
  echo "applying $(basename "$patch")"
  patch -d "$OUT" -p1 --forward --silent < "$patch"
  count=$((count + 1))
done

echo "$pin + $count patch(es) -> $OUT"
