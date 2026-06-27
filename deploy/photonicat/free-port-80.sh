#!/bin/sh
# Free TCP :80 (and :443) for Caddy by moving the Photonicat vendor admin web
# (pcat-manager-web) to :8088 — Obj 3.3.1 of the local-TLS onboarding.
#
# Run ON THE BOX as part of image provisioning, BEFORE enabling the caddy
# service. Idempotent: re-running it is a no-op once the vendor web is on :8088.
# The vendor admin stays reachable at http://<box>:8088 for hardware settings.
#
# NOTE: must be verified on real hardware — the vendor web's service mechanism
# varies by firmware. This script handles the two common cases (a uhttpd
# instance, or a standalone init service with a PORT in its config) and prints a
# clear message if neither matches so the operator can adjust by hand.
set -eu

NEW_PORT=8088

moved=0      # we repointed something this run
already=0    # the vendor web is already on :$NEW_PORT (idempotent re-run)

# Case 1: the vendor UI is a uhttpd instance (most OpenWrt-based firmwares).
# Repoint every :80/:443 listen directive at :$NEW_PORT and drop TLS (Caddy owns
# TLS now). We scan all uhttpd sections rather than assume a section name.
if command -v uci >/dev/null 2>&1 && uci show uhttpd >/dev/null 2>&1; then
	for sect in $(uci show uhttpd 2>/dev/null | sed -n "s/^uhttpd\.\([^.]*\)=uhttpd$/\1/p"); do
		# A section already on the new port is fine — record and skip.
		if uci -q get "uhttpd.$sect.listen_http" | grep -q ":$NEW_PORT"; then
			already=1
			continue
		fi
		uci -q delete "uhttpd.$sect.listen_https" || true
		uci set "uhttpd.$sect.listen_http=0.0.0.0:$NEW_PORT [::]:$NEW_PORT"
		moved=1
	done
	if [ "$moved" = 1 ]; then
		uci commit uhttpd
		/etc/init.d/uhttpd restart || true
		echo "free-port-80: moved uhttpd vendor web to :$NEW_PORT"
	fi
fi

# Case 2: a dedicated pcat-manager-web service with a port in its config.
if [ "$moved" = 0 ] && [ "$already" = 0 ] && [ -f /etc/config/pcat-manager-web ]; then
	if command -v uci >/dev/null 2>&1; then
		uci set pcat-manager-web.@main[0].port="$NEW_PORT" 2>/dev/null || true
		uci commit pcat-manager-web 2>/dev/null || true
	fi
	[ -x /etc/init.d/pcat-manager-web ] && /etc/init.d/pcat-manager-web restart || true
	echo "free-port-80: moved pcat-manager-web to :$NEW_PORT"
	moved=1
fi

# Idempotent success: already on :$NEW_PORT, nothing to do.
if [ "$moved" = 0 ] && [ "$already" = 1 ]; then
	echo "free-port-80: vendor web already on :$NEW_PORT — nothing to do."
	exit 0
fi

# Last resort: nothing matched and nothing is already on the new port. Only fail
# if something is actually still bound to :80 (otherwise :80 is already free).
if [ "$moved" = 0 ]; then
	if (netstat -ltn 2>/dev/null || ss -ltn 2>/dev/null) | grep -qE "[:.]80[[:space:]]"; then
		echo "free-port-80: WARNING — something still holds :80 but no known vendor web found." >&2
		echo "  Inspect it ('netstat -ltnp | grep :80') and move it off :80 before caddy." >&2
		exit 1
	fi
	echo "free-port-80: :80 is already free (no vendor web found)."
	exit 0
fi

echo "free-port-80: :80 and :443 are now free for Caddy."
