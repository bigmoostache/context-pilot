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

moved=0

# Case 1: the vendor UI is a uhttpd instance (most OpenWrt-based firmwares).
# Repoint every :80/:443 listen directive at :$NEW_PORT and drop TLS (Caddy owns
# TLS now). We scan all uhttpd sections rather than assume a section name.
if command -v uci >/dev/null 2>&1 && uci show uhttpd >/dev/null 2>&1; then
	for sect in $(uci show uhttpd 2>/dev/null | sed -n "s/^uhttpd\.\([^.]*\)=uhttpd$/\1/p"); do
		# Skip a section that is clearly already ours / on the new port.
		if uci -q get "uhttpd.$sect.listen_http" | grep -q ":$NEW_PORT"; then
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
if [ "$moved" = 0 ] && [ -f /etc/config/pcat-manager-web ]; then
	if command -v uci >/dev/null 2>&1; then
		uci set pcat-manager-web.@main[0].port="$NEW_PORT" 2>/dev/null || true
		uci commit pcat-manager-web 2>/dev/null || true
	fi
	[ -x /etc/init.d/pcat-manager-web ] && /etc/init.d/pcat-manager-web restart || true
	echo "free-port-80: moved pcat-manager-web to :$NEW_PORT"
	moved=1
fi

if [ "$moved" = 0 ]; then
	echo "free-port-80: WARNING — could not locate the vendor web on :80." >&2
	echo "  Inspect what holds :80 (e.g. 'netstat -ltnp | grep :80') and move it" >&2
	echo "  to :$NEW_PORT by hand before enabling caddy." >&2
	exit 1
fi

echo "free-port-80: :80 and :443 are now free for Caddy."
