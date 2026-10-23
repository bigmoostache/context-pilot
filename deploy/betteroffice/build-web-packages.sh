#!/usr/bin/env bash
# build-web-packages.sh: build BetterOffice browser packages from the pinned,
# patched tree and pack them as .tgz into web/vendor/betteroffice/.
#
# Every browser editor must come from the same commit as cp-office-engine:
# replicas built from different commits don't share a CRDT schema (npm pptx
# 0.2.0 rejects the pinned engine's state). This script is that guarantee.
#
# Usage: deploy/betteroffice/build-web-packages.sh <suffix> <family>...
#   suffix  version tag appended to upstream's next patch version,
#           e.g. cp.20261023.1 -> pptx 0.2.1-cp.20261023.1
#   family  pptx | docx | xlsx (each = engine + i18n + react packages)
# Needs bun, wasm-pack, binaryen and the wasm32 Rust target on PATH.
set -euo pipefail

if [ "$#" -lt 2 ]; then
  echo "usage: $0 <suffix> <pptx|docx|xlsx>..." >&2
  exit 2
fi
SUFFIX="$1"
shift

ROOT="$(git rev-parse --show-toplevel)"
TREE="$ROOT/target/betteroffice"
OUT="$ROOT/web/vendor/betteroffice"

"$ROOT/deploy/betteroffice/apply-patches.sh" "$TREE"
(cd "$TREE" && bun install --frozen-lockfile)

# upstream "0.2.0" -> "0.2.1-<suffix>"
next_version() {
  local v="$1"
  echo "${v%.*}.$(( ${v##*.} + 1 ))-$SUFFIX"
}

# Pin a package's version and replace its workspace:* deps on siblings by the
# exact versions being packed, so the .tgz resolves without the workspace.
set_versions() {
  local dir="$1" family="$2" version="$3"
  (cd "$dir" && bun -e '
    const fs = require("fs");
    const [family, version] = process.argv.slice(1);
    const pkg = JSON.parse(fs.readFileSync("package.json", "utf8"));
    pkg.version = version;
    for (const name of Object.keys(pkg.dependencies ?? {})) {
      if (pkg.dependencies[name].startsWith("workspace:")) {
        if (!name.startsWith(`@betteroffice/${family}`)) {
          throw new Error(`unexpected workspace dep ${name}`);
        }
        pkg.dependencies[name] = version;
      }
    }
    fs.writeFileSync("package.json", JSON.stringify(pkg, null, 2) + "\n");
  ' "$family" "$version")
}

for family in "$@"; do
  base="$(cd "$TREE/packages/$family" && bun -e 'console.log(require("./package.json").version)')"
  version="$(next_version "$base")"
  echo "== $family $base -> $version"
  for pkg in "$family" "$family-i18n" "$family-react"; do
    set_versions "$TREE/packages/$pkg" "$family" "$version"
  done
  for pkg in "$family" "$family-i18n" "$family-react"; do
    (cd "$TREE/packages/$pkg" && bun run build)
  done
  for pkg in "$family" "$family-i18n" "$family-react"; do
    (cd "$TREE/packages/$pkg" && npm pack --silent --pack-destination "$OUT" >/dev/null)
    echo "packed betteroffice-$pkg-$version.tgz"
  done
done
