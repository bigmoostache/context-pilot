#!/bin/bash
# Container pid 1 (under tini). Seeds what the image cannot bake, validates the
# first-boot credentials, then hands off to the orchestrator.
#
# Why seeding runs at every start: $HOME is a volume, so anything written to
# /data at build time is masked the moment the volume is mounted over it.
# Everything below is therefore idempotent and a no-op on a populated volume.
set -euo pipefail

: "${HOME:=/data}"
CP_DIR="$HOME/.context-pilot"

die() { echo "context-pilot: $*" >&2; exit 1; }

# ── Provider / integration API keys ──────────────────────────────────────────
# The appliance ships these as a systemd EnvironmentFile (deploy/ansible
# templates/providers.env.j2); the container equivalent is a mounted file named
# by CP_PROVIDERS_ENV_FILE. Loading it into the process environment — rather
# than writing it into ~/.context-pilot/.env — matches what the vault expects
# (env is step 2 of its cascade, cp-vault/src/local.rs:45) and leaves the .env
# the cockpit itself writes to untouched, so a key set from the UI is never
# clobbered at the next restart.
#
# Parsed literally, not sourced: `.` would execute the file as shell. systemd's
# EnvironmentFile does not, and neither do we — an operator pasting a key
# containing a backtick should get that key, not a command.
if [ -n "${CP_PROVIDERS_ENV_FILE:-}" ]; then
  [ -r "$CP_PROVIDERS_ENV_FILE" ] || die \
    "CP_PROVIDERS_ENV_FILE=$CP_PROVIDERS_ENV_FILE is not readable by uid $(id -u) — a 0600 file owned by the host user is invisible here; see deploy/docker/README.md"
  n=0
  while IFS= read -r line || [ -n "$line" ]; do
    case "$line" in ''|'#'*) continue ;; esac
    key=${line%%=*}
    val=${line#*=}
    [ "$key" = "$line" ] && continue                       # no '=' → not an assignment
    case "$key" in [A-Za-z_]*) ;; *) continue ;; esac
    # Strip one layer of matching quotes, as systemd does.
    case "$val" in
      \"*\") val=${val#\"}; val=${val%\"} ;;
      \'*\') val=${val#\'}; val=${val%\'} ;;
    esac
    export "$key=$val"
    n=$((n + 1))
  done < "$CP_PROVIDERS_ENV_FILE"
  echo "context-pilot: loaded $n key(s) from $CP_PROVIDERS_ENV_FILE"
fi

# ── First-boot credentials ───────────────────────────────────────────────────
# The orchestrator seeds accounts only when the user table is EMPTY
# (runtime/seed.rs), so these matter on the very first start and are ignored
# afterwards. We check them here rather than let the binary skip silently: a box
# that boots with no account and no message is the worst possible first contact,
# and the failure would only surface at the login screen.
#
# Same default the binary derives (auth/helpers.rs:62) — first boot is "this
# database does not exist yet".
AUTH_DB="${CP_AUTH_DB:-$CP_DIR/orchestrator/auth.db}"

have_secret() {  # <prefix> → true if a password or a readable password file is set
  local p="$1" f
  eval "f=\${${p}_PASSWORD_FILE:-}"
  [ -n "$f" ] && [ -r "$f" ] && [ -s "$f" ] && return 0
  eval "[ -n \"\${${p}_PASSWORD:-}\" ]" && return 0
  return 1
}

if [ "${CP_AUTH_ENABLED:-1}" = "1" ] && [ ! -f "$AUTH_DB" ]; then
  # Superadmin — the vendor account, and the only role that can ever create
  # another superadmin (auth/capabilities.rs). Omit it on first boot and the
  # deployment can never have one, by any in-product path.
  [ -n "${CP_SEED_SUPERADMIN_EMAIL:-}" ] || die \
    "first boot: CP_SEED_SUPERADMIN_EMAIL is required (see deploy/docker/README.md)"
  have_secret CP_SEED_SUPERADMIN || die \
    "first boot: set CP_SEED_SUPERADMIN_PASSWORD_FILE (preferred) or CP_SEED_SUPERADMIN_PASSWORD"

  # Admin — the customer's top account. Optional: a superadmin can create it
  # later from the cockpit. But half-configured is a typo, not an intent.
  if [ -n "${CP_SEED_ADMIN_EMAIL:-}" ]; then
    have_secret CP_SEED_ADMIN || die \
      "first boot: CP_SEED_ADMIN_EMAIL is set but no password — set CP_SEED_ADMIN_PASSWORD_FILE or CP_SEED_ADMIN_PASSWORD"
  fi
  echo "context-pilot: first boot — seeding accounts (password change forced at first login)"
fi

# ── Meilisearch ──────────────────────────────────────────────────────────────
# cp-mod-search expects the binary at ~/.context-pilot/meilisearch/bin/ and
# DOWNLOADS it from github.com on first use when absent
# (cp-mod-search/src/meili/server/download.rs). The bundle already ships the
# pinned build, so install it here: the download would otherwise be a surprise
# egress call at first search, and a hang wherever egress is filtered.
meili_bin="$CP_DIR/meilisearch/bin/meilisearch"
if [ ! -x "$meili_bin" ] && [ -x /opt/cp/meilisearch ]; then
  echo "context-pilot: seeding meilisearch into $meili_bin"
  mkdir -p "$(dirname "$meili_bin")"
  cp /opt/cp/meilisearch "$meili_bin"
  chmod 755 "$meili_bin"
fi

# Agent realms land under CP_AGENTS_ROOT, the registry under ~/.context-pilot/agents.
# Create both so the first agent creation doesn't fail on a missing parent.
mkdir -p "${CP_AGENTS_ROOT:-$HOME/code}" "$CP_DIR/agents"

# ── Exposure notice ──────────────────────────────────────────────────────────
# We cannot see the host's port mapping from in here, so this is a notice, not a
# check. It is worth printing because the failure it warns about is silent and
# very hard to diagnose: the SPA builds every agent command with
# crypto.randomUUID (web/src/lib/api/client/index.ts:47), which exists only in a
# secure context. Served over http:// from anything but localhost, the cockpit
# loads and logs in, then throws on every command.
cat <<'EOF'
context-pilot: reachable at http://127.0.0.1:<published-port>
  Serving this over plain http:// on a LAN address BREAKS the cockpit: agent
  commands need a secure context. Publish on loopback, or put TLS in front.
EOF

exec /opt/cp/cp-orchestrator "$@"
