# Photonicat Deployment Checklist

Complete guide to deploying Context Pilot on a Photonicat (aarch64 OpenWrt).
Based on the photonicat-3f (tintin) deployment — every step battle-tested.

## Prerequisites

- Photonicat with OpenWrt (aarch64, musl libc)
- SSD/microSD card for agent data (internal flash is ≤4 GB — too small)
- A Mac or Linux box for cross-compilation
- Tailscale account

---

## 1. Tailscale — stable SSH access

Install Tailscale on the Photonicat first. Without it, you're dependent on
LAN connectivity which breaks when the device switches to 5G or changes
networks.

```bash
# On the Photonicat (via initial LAN SSH)
opkg update
opkg install tailscale
tailscale up --hostname=tintin
```

Note the Tailscale IP (e.g. `100.81.116.102`). Add an SSH config entry on
your workstation:

```
# ~/.ssh/config
Host tintin
    HostName 100.81.116.102
    User root
    StrictHostKeyChecking no
```

From now on, use `ssh tintin` for all access.

---

## 2. SSD setup

The internal flash is ≤4 GB — agent data (oplogs, conversations, search
indexes) must live on the SSD.

```bash
# Identify the SSD
lsblk
# Typically /dev/mmcblk1p1 (microSD) or /dev/sda1 (USB SSD)

# Format as ext4 (destructive — wipes all data)
mkfs.ext4 /dev/mmcblk1p1

# Create mount point and mount
mkdir -p /mnt/ssd
mount /dev/mmcblk1p1 /mnt/ssd

# Add fstab entry for boot persistence
echo '/dev/mmcblk1p1 /mnt/ssd ext4 defaults,noatime 0 2' >> /etc/fstab

# Create agent data directory
mkdir -p /mnt/ssd/context-pilot/agents
```

---

## 3. Cross-compile binaries

On your Mac/Linux workstation. The Photonicat runs musl libc — all binaries
must be statically linked against `aarch64-unknown-linux-musl`.

```bash
# Install cross (Docker-based cross-compilation)
cargo install cross --version 0.2.5

# Add the musl target
rustup target add aarch64-unknown-linux-musl

# Build orchestrator
cross build --release --target aarch64-unknown-linux-musl -p cp-orchestrator
# Output: target/aarch64-unknown-linux-musl/release/cp-orchestrator
```

Download the agent (`cpilot`) and console server from the latest GitHub
release — the release CI already builds aarch64 musl binaries:

```bash
# Download latest release tarball
gh release download --repo <owner>/context-pilot -p 'context-pilot-linux-aarch64.tar.gz'
tar xzf context-pilot-linux-aarch64.tar.gz
# Contains: cpilot, cp-console-server
```

---

## 4. Deploy binaries

```bash
# Create directory on the Photonicat
ssh tintin 'mkdir -p /opt/context-pilot/bin'

# Copy binaries
scp target/aarch64-unknown-linux-musl/release/cp-orchestrator tintin:/opt/context-pilot/bin/
scp cpilot tintin:/opt/context-pilot/bin/
scp cp-console-server tintin:/opt/context-pilot/bin/

# Verify they run
ssh tintin '/opt/context-pilot/bin/cp-orchestrator --version'
```

---

## 5. Deploy frontend

Build the web frontend on your workstation and copy the dist to the
Photonicat:

```bash
cd web
pnpm install
pnpm build
# Output: web/dist/

# Copy to Photonicat
scp -r dist tintin:/opt/context-pilot/web
```

---

## 6. Self-signed SSL certificate

```bash
ssh tintin '
mkdir -p /opt/context-pilot/ssl
openssl req -x509 -newkey rsa:2048 -nodes \
  -keyout /opt/context-pilot/ssl/key.pem \
  -out /opt/context-pilot/ssl/cert.pem \
  -days 3650 \
  -subj "/CN=tintin.local"
'
```

---

## 7. haproxy — reverse proxy

Install haproxy and configure it to terminate SSL:

```bash
ssh tintin 'opkg update && opkg install haproxy'
```

Create `/etc/haproxy.cfg`:

```haproxy
global
    maxconn 256

defaults
    mode http
    timeout connect 5s
    timeout client 30s
    timeout server 30s

frontend https
    bind *:443 ssl crt /opt/context-pilot/ssl/combined.pem
    # API requests → orchestrator
    acl is_api path_beg /api
    use_backend orchestrator if is_api
    # Everything else → static frontend
    default_backend frontend

frontend http
    bind *:80
    redirect scheme https code 302

backend orchestrator
    server orch 127.0.0.1:7878

backend frontend
    server web 127.0.0.1:3000
```

haproxy needs a combined PEM (cert + key in one file):

```bash
ssh tintin '
cat /opt/context-pilot/ssl/cert.pem /opt/context-pilot/ssl/key.pem \
  > /opt/context-pilot/ssl/combined.pem
chmod 600 /opt/context-pilot/ssl/combined.pem
'
```

---

## 8. uhttpd — static frontend server

uhttpd is pre-installed on OpenWrt. It serves the static frontend on port
3000 (haproxy proxies to it).

No separate config file needed — the init script starts it with flags (see
step 10).

---

## 9. mDNS — local network discovery

```bash
ssh tintin '
opkg install avahi-daemon
# Set hostname
uci set system.@system[0].hostname="tintin"
uci commit system

# Configure avahi
cat > /etc/avahi/avahi-daemon.conf << EOF
[server]
host-name=tintin
use-ipv4=yes
use-ipv6=yes

[publish]
publish-addresses=yes
publish-workstation=no

[reflector]

[rlimits]
EOF

# Allow mDNS through firewall
uci add firewall rule
uci set firewall.@rule[-1].name="Allow-mDNS"
uci set firewall.@rule[-1].src="*"
uci set firewall.@rule[-1].dest_port="5353"
uci set firewall.@rule[-1].proto="udp"
uci set firewall.@rule[-1].target="ACCEPT"
uci commit firewall
/etc/init.d/firewall reload

# Start avahi
/etc/init.d/avahi-daemon enable
/etc/init.d/avahi-daemon start
'
```

The device will be reachable at `tintin.local` from the LAN.

---

## 10. Init script — boot persistence

Create `/etc/init.d/context-pilot`:

```bash
#!/bin/sh /etc/rc.common
START=99
STOP=10
USE_PROCD=1

start_service() {
    # uhttpd for static frontend on :3000
    procd_open_instance uhttpd
    procd_set_param command /usr/sbin/uhttpd \
        -f -p 127.0.0.1:3000 -h /opt/context-pilot/web -E /index.html -n 3
    procd_set_param respawn
    procd_close_instance

    # orchestrator on :7878
    procd_open_instance orchestrator
    procd_set_param command /opt/context-pilot/bin/cp-orchestrator
    procd_set_param env \
        HOME=/root \
        CP_AUTH_ENABLED=true \
        CP_ORCH_PORT=7878 \
        CP_AGENTS_ROOT=/mnt/ssd/context-pilot/agents \
        CP_AGENT_BINARY=/opt/context-pilot/bin/cpilot \
        FIRECRAWL_API_KEY=<your-key> \
        BRAVE_API_KEY=<your-key> \
        DATALAB_API_KEY=<your-key> \
        VOYAGE_API_KEY=<your-key> \
        GITHUB_TOKEN=<your-token>
    procd_set_param respawn
    procd_set_param stderr 1
    procd_set_param stdout 1
    procd_close_instance

    # haproxy reverse proxy
    procd_open_instance haproxy
    procd_set_param command /usr/sbin/haproxy -f /etc/haproxy.cfg -db
    procd_set_param respawn
    procd_close_instance
}
```

```bash
chmod +x /etc/init.d/context-pilot
/etc/init.d/context-pilot enable
/etc/init.d/context-pilot start
```

### ⚠️ Critical gotcha: HOME=/root

procd sets `HOME=/` by default. The agent writes its registration JSON to
`~/.context-pilot/agents/<id>.json`. Without `HOME=/root`, this lands at
`/.context-pilot/agents/` instead of `/root/.context-pilot/agents/`, and
the orchestrator won't find it.

### ⚠️ Critical gotcha: do NOT set CP_AGENTS_DIR

The agent **ignores** `CP_AGENTS_DIR` — it always writes its registration
JSON to `~/.context-pilot/agents/` (HOME-based). The orchestrator's default
scan path is the same `~/.context-pilot/agents/`. Setting `CP_AGENTS_DIR`
to the SSD path breaks discovery because the agent still writes to HOME.

Use `CP_AGENTS_ROOT` (not `CP_AGENTS_DIR`) to control where agent **realm
folders** (conversations, oplogs) are created on the SSD.

---

## 11. Firewall — allow HTTP/HTTPS on WAN

OpenWrt classifies eth0 as the WAN zone and **rejects all incoming traffic
by default**. If the Mac connects to the Photonicat's eth0 IP (typical on
a shared LAN), ports 80/443 are blocked.

```bash
ssh tintin '
# Allow HTTPS
uci add firewall rule
uci set firewall.@rule[-1].name="Allow-HTTPS-WAN"
uci set firewall.@rule[-1].src="wan"
uci set firewall.@rule[-1].dest_port="443"
uci set firewall.@rule[-1].proto="tcp"
uci set firewall.@rule[-1].target="ACCEPT"

# Allow HTTP (for redirect)
uci add firewall rule
uci set firewall.@rule[-1].name="Allow-HTTP-WAN"
uci set firewall.@rule[-1].src="wan"
uci set firewall.@rule[-1].dest_port="80"
uci set firewall.@rule[-1].proto="tcp"
uci set firewall.@rule[-1].target="ACCEPT"

uci commit firewall
/etc/init.d/firewall reload
'
```

---

## 12. Bootstrap superadmin

After the orchestrator starts:

```bash
# Check auth status
curl -s http://127.0.0.1:7878/api/auth/status
# → {"enabled":true,"bootstrapped":false}

# Register first user (becomes admin automatically)
curl -s -X POST http://127.0.0.1:7878/api/auth/register \
  -H "Content-Type: application/json" \
  -d '{"email":"you@example.com","name":"Your Name","password":"your-password"}'

# Login to get a token
curl -s -X POST http://127.0.0.1:7878/api/auth/login \
  -H "Content-Type: application/json" \
  -d '{"email":"you@example.com","password":"your-password"}'
# → {"token":"<bearer-token>","user":{...}}
```

### ⚠️ The endpoint is `/api/auth/register`, NOT `/api/auth/bootstrap`

There is no `/bootstrap` endpoint. `/register` doubles as bootstrap when
zero users exist — the first registered user becomes admin.

---

## 13. Spawn agent

```bash
TOKEN="<from-login-above>"

# Create agent with realm folder on SSD
curl -s -X POST \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  http://127.0.0.1:7878/api/fleet/create \
  -d '{"name":"tintin-agent","folder":"/mnt/ssd/context-pilot/agents/tintin-agent"}'

# Wait a few seconds, then verify
curl -s -H "Authorization: Bearer $TOKEN" http://127.0.0.1:7878/api/fleet
# Should show the agent with lifecycle=running, phase=idle
```

The orchestrator spawns the agent on a PTY with `CP_BRIDGE=1`. The console
server auto-starts alongside the agent.

---

## 14. LLM keys (user-managed)

Add your Anthropic / OpenAI / other LLM provider keys. Edit the init
script env vars and restart:

```bash
vi /etc/init.d/context-pilot
# Add: ANTHROPIC_API_KEY=sk-ant-...
/etc/init.d/context-pilot restart
```

Then re-bootstrap and re-spawn the agent (the auth DB is on internal flash
and survives restarts, but the agent process needs to be re-created).

---

## 15. Verification checklist

Run these from your workstation to confirm everything works:

```bash
# mDNS resolution
dns-sd -G v4 tintin.local
# → should resolve to the Photonicat's LAN IP

# HTTPS frontend
curl -sk https://tintin.local/
# → HTML of the web frontend

# API health
curl -sk https://tintin.local/api/health
# → 200

# Auth status
curl -sk https://tintin.local/api/auth/status
# → {"enabled":true,"bootstrapped":true}

# Tailscale access
curl -sk https://100.81.116.102/
# → same frontend
```

---

## Known limitations

### Meilisearch — incompatible with musl

Meilisearch only provides glibc builds. OpenWrt uses musl libc. The binary
fails with "not found" (dynamic linker mismatch). This is a
[known issue](https://github.com/meilisearch/meilisearch/issues/4377).

**Impact**: Full-text search across files and logs won't work. Everything
else works normally.

**Future options**:
1. Cross-compile Meilisearch for musl (~30 min build)
2. Run Meilisearch on a separate glibc machine
3. Wait for official musl build

---

## Disk layout

| Filesystem | Mount | Content |
|------------|-------|---------|
| Internal flash (`/overlay`) | `/` | OS, binaries (`/opt/context-pilot/bin/`), frontend (`/opt/context-pilot/web/`), SSL certs, configs, auth DB |
| SSD (`/dev/mmcblk1p1`) | `/mnt/ssd` | Agent realm data: conversations, oplogs, search indexes, uploaded files |

Binaries and configs are small and static — fine on internal flash.
Agent data grows — must be on SSD.

---

## Troubleshooting

### Fleet returns empty `{"data":{}}`
The agent hasn't registered. Check:
1. Is `HOME=/root` set in the init script? (procd defaults to `HOME=/`)
2. Is there a `.json` file in `/root/.context-pilot/agents/`?
3. Is the agent process running? (`pgrep cpilot`)

### Connection refused from browser but works from localhost
OpenWrt firewall WAN zone rejects incoming traffic. Add the port 80/443
rules (step 11).

### Bootstrap returns "missing authorization"
You're hitting the wrong endpoint. Use `POST /api/auth/register`, not
`/api/auth/bootstrap`.

### Agent boots but bridge is inert
Check `HOME` — if it's `/` instead of `/root`, the registration JSON goes
to `/.context-pilot/agents/` which the orchestrator doesn't scan.
