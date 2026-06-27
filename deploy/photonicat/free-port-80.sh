#!/bin/sh
# Free TCP :80 (and :443) for Caddy (the Context Pilot cockpit). The Photonicat
# ships TWO vendor web UIs that grab these ports — handle BOTH:
#   - uhttpd (LuCI)            → moved to :8088 (kept for IT hardware config).
#   - pcat-manager-web         → Photonicat board dashboard, a Python app whose
#                                port is HARDCODED on :80 (no /etc/config, no init
#                                arg) so it CANNOT be moved → DISABLED. Product
#                                decision: clients see Context Pilot; IT uses LuCI
#                                on :8088 for the hardware.
#
# Run ON THE BOX before enabling caddy. Idempotent. Prints a change line per
# action so Ansible can report `changed`. ALWAYS verifies :80 is actually free at
# the end (the previous version assumed moving uhttpd freed it and missed
# pcat-manager-web — that bug is fixed here).
set -eu

NEW_PORT=8088
changed=0

# 1) uhttpd (LuCI): repoint every :80/:443 listener to :$NEW_PORT, drop its TLS
# (Caddy owns TLS now). Scan all sections rather than assume a name.
if command -v uci >/dev/null 2>&1 && uci show uhttpd >/dev/null 2>&1; then
	moved_uhttpd=0
	for sect in $(uci show uhttpd 2>/dev/null | sed -n "s/^uhttpd\.\([^.]*\)=uhttpd$/\1/p"); do
		# Already on the new port → idempotent, skip.
		uci -q get "uhttpd.$sect.listen_http" | grep -q ":$NEW_PORT" && continue
		uci -q delete "uhttpd.$sect.listen_https" || true
		uci set "uhttpd.$sect.listen_http=0.0.0.0:$NEW_PORT [::]:$NEW_PORT"
		moved_uhttpd=1
	done
	if [ "$moved_uhttpd" = 1 ]; then
		uci commit uhttpd
		/etc/init.d/uhttpd restart || true
		echo "free-port-80: moved uhttpd (LuCI) to :$NEW_PORT"
		changed=1
	fi
fi

# 2) pcat-manager-web (board dashboard): hardcoded :80, no config knob → disable
# it. Idempotent: only acts while it is still enabled.
if [ -x /etc/init.d/pcat-manager-web ] && /etc/init.d/pcat-manager-web enabled 2>/dev/null; then
	/etc/init.d/pcat-manager-web disable || true
	/etc/init.d/pcat-manager-web stop || true
	echo "free-port-80: disabled pcat-manager-web (board dashboard held :80)"
	changed=1
fi

# 3) ALWAYS confirm :80 is now actually free. Fail loudly (with the culprit) if
# something still holds it, so we never let caddy fight for :80 silently.
if (netstat -ltn 2>/dev/null || ss -ltn 2>/dev/null) | grep -qE "[:.]80[[:space:]]"; then
	echo "free-port-80: WARNING — something still holds :80:" >&2
	(netstat -ltnp 2>/dev/null || ss -ltnp 2>/dev/null) | grep -E "[:.]80[[:space:]]" >&2 || true
	echo "  Move/stop it before caddy takes :80/:443." >&2
	exit 1
fi

if [ "$changed" = 1 ]; then
	echo "free-port-80: :80 and :443 are now free for Caddy."
else
	echo "free-port-80: :80 already free — nothing to do."
fi
exit 0
