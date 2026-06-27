#!/bin/sh
# Open the client-facing Context Pilot services on the interface the client
# reaches the box through. OpenWrt classifies that uplink (eth0) as the `wan`
# zone (input REJECT by default), so without this the cockpit (:443) and the IT
# maintenance plane (:9090) are unreachable from the client's corporate/local LAN
# — which is exactly how they're meant to be accessed.
#
# Ports opened (TCP, from the wan zone):
#   80   — redirect → 443 (cockpit)
#   443  — product cockpit (served once provisioned)
#   9090 — IT maintenance plane (admin-RBAC password-gated; the client's IT owns
#          their own network perimeter — "we specify, they act")
#
# The vendor admin path (SSH + remote management) stays on the tailnet, NOT here.
#
# Idempotent: a single NAMED uci section, deleted+recreated each run, so re-runs
# never stack duplicate rules. Install/run from Ansible (deploy step).
set -eu

uci -q delete firewall.cp_client_access || true
uci set firewall.cp_client_access='rule'
uci set firewall.cp_client_access.name='Allow-ContextPilot-client'
uci set firewall.cp_client_access.src='wan'
uci set firewall.cp_client_access.proto='tcp'
uci add_list firewall.cp_client_access.dest_port='80'
uci add_list firewall.cp_client_access.dest_port='443'
uci add_list firewall.cp_client_access.dest_port='9090'
uci set firewall.cp_client_access.target='ACCEPT'
uci commit firewall

/etc/init.d/firewall reload >/dev/null 2>&1
echo "client-facing firewall rule applied (tcp 80/443/9090 from wan)"
