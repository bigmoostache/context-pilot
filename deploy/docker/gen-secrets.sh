#!/usr/bin/env bash
# Generate the first-boot credential files docker-compose.yml expects, and print
# them once. Idempotent: an existing password file is never overwritten, so
# re-running before `docker compose up` cannot desync the file from the account
# already seeded from it.
#
#   deploy/docker/gen-secrets.sh
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DIR="$HERE/secrets"
# 0700 on the directory, 0644 on the files it holds. The container runs as uid
# 10001 and compose bind-mounts these files into it, so a 0600 file owned by the
# host user is simply unreadable there — the orchestrator would refuse to start.
# Protection therefore lives on the directory: no other host user can traverse
# it, while the daemon (root) can still mount what is inside.
mkdir -p "$DIR"
chmod 700 "$DIR"

# `cut`, not `head -c`: head closes the pipe early, tr dies on SIGPIPE, and
# `set -o pipefail` would turn that into a failed run (exit 141).
gen() { head -c 96 /dev/urandom | base64 | LC_ALL=C tr -dc 'A-Za-z0-9' | cut -c1-24; }

for name in superadmin admin; do
  f="$DIR/$name.pw"
  if [ -s "$f" ]; then
    echo "  $name.pw   (kept) $(cat "$f")"
  else
    (umask 022; gen > "$f")
    echo "  $name.pw   $(cat "$f")"
  fi
done
chmod 644 "$DIR"/*.pw

# The provider file must exist even when empty: compose mounts it as a secret,
# and a missing source file is a hard `docker compose up` error.
if [ ! -f "$DIR/providers.env" ]; then
  (umask 022; cat > "$DIR/providers.env" <<'EOF'
# Provider / integration API keys, one KEY=value per line, no export, no quotes
# needed. Loaded into the orchestrator's environment at every start and never
# written to the volume. Keys set from the cockpit are stored separately and are
# not affected by this file.
#
# Model providers (at least one is needed for an agent to answer):
#   ANTHROPIC_API_KEY=
#   XAI_API_KEY=
#   GROQ_API_KEY=
#   DEEPSEEK_API_KEY=
#   MINIMAX_API_KEY=
# Integrations:
#   BRAVE_API_KEY=          FIRECRAWL_API_KEY=      DATALAB_API_KEY=
#   VOYAGE_API_KEY=         GITHUB_TOKEN=
EOF
  )
  echo "  providers.env  created — add your API keys before 'docker compose up'"
fi

echo
echo "Passwords are shown once here and are also in $DIR."
echo "Both accounts must change their password at first login."
