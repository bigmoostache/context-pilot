#!/bin/sh
# photonicat bootstrap — single-script day-0 provisioning
# Run once per box: WiFi → Tailscale install + persistent service → tailscale up --ssh
# Saves itself to /mnt/data/bootstrap.sh (persists across reboots).

set -e

echo "============================================"
echo " photonicat bootstrap"
echo "============================================"
echo ""

# ── Step 1: Connect to WiFi ────────────────────────────────────────────────
echo "── WiFi setup ──"

while true; do
    echo ""
    printf "WiFi SSID: "
    read -r WIFI_SSID
    printf "Password:  "
    read -r WIFI_PASS

    if [ -z "$WIFI_SSID" ] || [ -z "$WIFI_PASS" ]; then
        echo "Both SSID and password are required."
        continue
    fi

    echo "Configuring UCI..."

    # Create wwan network interface (DHCP client on the WiFi STA)
    uci -q delete network.wwan 2>/dev/null || true
    uci set network.wwan=interface
    uci set network.wwan.proto=dhcp
    uci commit network

    # Find the radio device name (e.g. "radio0")
    RADIO=$(uci show wireless 2>/dev/null | grep -F "=wifi-device" | head -1 | cut -d= -f1 | cut -d. -f2)
    RADIO="${RADIO:-radio0}"

    # Add STA wifi-iface (keep existing AP iface untouched)
    uci -q delete wireless.sta 2>/dev/null || true
    uci set wireless.sta=wifi-iface
    uci set wireless.sta.device="$RADIO"
    uci set wireless.sta.mode=sta
    uci set wireless.sta.network=wwan
    uci set wireless.sta.ssid="$WIFI_SSID"
    uci set wireless.sta.encryption=psk2
    uci set wireless.sta.key="$WIFI_PASS"
    uci commit wireless

    echo "Restarting network..."
    wifi reload
    ifup wwan 2>/dev/null || true
    sleep 5

    # Find the STA interface name
    STA_IFACE=$(iwinfo 2>/dev/null | grep -B1 "ESSID.*$WIFI_SSID" | head -1 | awk '{print $1}' 2>/dev/null || echo "")
    if [ -z "$STA_IFACE" ]; then
        STA_IFACE=$(iwinfo 2>/dev/null | awk '/ESSID.*unknown/{print $1; exit}' 2>/dev/null || echo "")
    fi

    echo "Checking connectivity..."
    if ping -c2 -W3 1.1.1.1 >/dev/null 2>&1; then
        echo ""
        echo "✓ WiFi connected! ($STA_IFACE → $WIFI_SSID)"
        break
    else
        echo "✗ No internet. Check SSID/password and retry."
        if [ -n "$STA_IFACE" ]; then
            echo "  STA interface $STA_IFACE — check: iwinfo $STA_IFACE info"
        fi
        # Clean up failed attempt
        uci -q delete wireless.sta 2>/dev/null || true
        uci commit wireless
        wifi reload
    fi
done

# ── Step 2: Install Tailscale ──────────────────────────────────────────────
echo ""
echo "── Tailscale ──"

# Install if not present
if ! command -v tailscale >/dev/null 2>&1; then
    echo "Installing Tailscale via opkg..."
    opkg update
    opkg install tailscale
fi

TAILSCALE_VERSION=$(tailscale version 2>/dev/null | head -1 || echo "unknown")
echo "Tailscale version: $TAILSCALE_VERSION"

# ── Step 3: Install persistent init.d service ───────────────────────────────
echo "Installing /etc/init.d/tailscale..."

cat > /etc/init.d/tailscale << 'INITEOF'
#!/bin/sh /etc/rc.common
# Tailscale daemon — OpenWrt procd service (Photonicat)
# Auto-starts at boot, respawns on crash, survives reboots.

USE_PROCD=1
START=90
STOP=11

PROG=/usr/sbin/tailscaled
TS_STATE=/mnt/data/context-pilot/tailscale
SOCK=/var/run/tailscale/tailscaled.sock

start_service() {
	mkdir -p "$TS_STATE"
	mkdir -p "$(dirname "$SOCK")"

	procd_open_instance
	procd_set_param command "$PROG" \
		--statedir="$TS_STATE" \
		--socket="$SOCK"
	procd_set_param respawn 3600 5 5
	procd_set_param stdout 1
	procd_set_param stderr 1
	procd_close_instance
}

stop_service() {
	# Do NOT run "tailscale down" — that sets wantRunning=false in prefs,
	# which would prevent auto-rejoin after reboot.
	# SIGTERM via procd is enough; the node goes offline and reconnects
	# on next start, persisting its enrollment state.
	return 0
}
INITEOF

chmod +x /etc/init.d/tailscale
/etc/init.d/tailscale enable

echo "Starting tailscaled (procd-supervised)..."
/etc/init.d/tailscale start
sleep 3

if ! pgrep tailscaled >/dev/null 2>&1; then
    echo "⚠ tailscaled failed to start. Falling back to manual launch..."
    nohup tailscaled --statedir=/mnt/data/context-pilot/tailscale --socket=/var/run/tailscale/tailscaled.sock >/dev/null 2>&1 &
    sleep 3
fi

if pgrep tailscaled >/dev/null 2>&1; then
    echo "✓ tailscaled running (PID $(pgrep tailscaled | head -1))"
else
    echo "✗ tailscaled still not running. Check: logread -e tailscaled"
    exit 1
fi

# ── Step 4: Authenticate + join tailnet ─────────────────────────────────────
echo ""
echo "── Tailscale enrollment ──"

# Check if already enrolled
CURRENT_STATUS=$(tailscale status --json 2>/dev/null | grep -o '"BackendState":"[^"]*"' | cut -d'"' -f4 || echo "NoState")
if [ "$CURRENT_STATUS" = "Running" ]; then
    echo "Already enrolled and running."
    tailscale status
    echo ""
    echo "Done. SSH:  ssh root@$(tailscale status --json 2>/dev/null | grep -o '"Self":{"ID":"[^"]*","PublicKey":"[^"]*","HostName":"[^"]*"' | grep -o '"HostName":"[^"]*"' | cut -d'"' -f4 || tailscale status | head -1 | awk '{print $2}')"
    exit 0
fi

# Prompt for authkey or hostname
printf "Tailscale auth key (tskey-auth-...): "
read -r AUTHKEY
printf "Hostname for this box [photonicat-3f]: "
read -r HOSTNAME
HOSTNAME="${HOSTNAME:-photonicat-3f}"
printf "Client tag [cp-tintin]: "
read -r CLIENT_TAG
CLIENT_TAG="${CLIENT_TAG:-cp-tintin}"

echo "Joining tailnet as $HOSTNAME..."
tailscale up \
    --authkey="$AUTHKEY" \
    --advertise-tags="tag:$CLIENT_TAG" \
    --hostname="$HOSTNAME" \
    --ssh \
    --accept-routes=false

sleep 3
echo ""
echo "============================================"
echo " Bootstrap complete!"
echo " Tailscale IP: $(tailscale ip -4)"
echo " Hostname:     $HOSTNAME"
echo " SSH:          ssh root@$HOSTNAME"
echo "============================================"
tailscale status
