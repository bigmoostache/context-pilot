# Context Pilot — container image (orchestrator + agent + console server + Meilisearch).
#
# The cloud/self-hosted counterpart of deploy/ansible + deploy/photonicat: the
# same release bundle, the same binaries, packaged instead of provisioned. The
# product is NEVER patched here — everything below is packaging and first-boot
# policy.
#
#   docker build -t context-pilot:0.2.13 deploy/docker/
#
# Two bundle sources, selected by --build-arg BUNDLE_SOURCE=:
#   release (default) — downloads the `cpilot-linux-x86_64` asset of a published
#                       release, checksum-verified. Reproducible, ~1 min.
#   local             — uses deploy/docker/.artifacts/cpilot-linux-x86_64.tar.gz,
#                       for testing an unreleased build (see README).
ARG BUNDLE_SOURCE=release

# ── Bundle acquisition ───────────────────────────────────────────────────────
FROM debian:trixie-slim AS bundle-release
# Pinned tag + checksum, not "latest": an image that silently changes product
# version between two builds makes every bug report ambiguous. Bump both
# together — a VERSION bump alone fails loudly at the sha256 check, which is the
# intended behaviour, not an annoyance. Set BUNDLE_SHA256="" to skip the check.
ARG VERSION=v0.2.13-d4b747c
ARG BUNDLE_SHA256=7eadbb712a1e67103a45872a9160ac8a6fd6341cc38219f42571bb9fe69cda57
ARG REPO=bigmoostache/context-pilot
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
 && rm -rf /var/lib/apt/lists/*
RUN curl -fSL -o /tmp/bundle.tar.gz \
      "https://github.com/${REPO}/releases/download/${VERSION}/cpilot-linux-x86_64.tar.gz" \
 && if [ -n "${BUNDLE_SHA256}" ]; then echo "${BUNDLE_SHA256}  /tmp/bundle.tar.gz" | sha256sum -c -; fi

FROM debian:trixie-slim AS bundle-local
COPY .artifacts/cpilot-linux-x86_64.tar.gz /tmp/bundle.tar.gz

FROM bundle-${BUNDLE_SOURCE} AS bundle

# ── Runtime ──────────────────────────────────────────────────────────────────
# trixie, not bookworm: the x86_64 release target is x86_64-unknown-linux-gnu
# built on ubuntu-24.04, and the binaries require GLIBC_2.39 (verified with
# objdump). trixie ships 2.41; bookworm's 2.36 cannot run them at all.
FROM debian:trixie-slim AS runtime

# A floor, not a toolchain: the agent executes shell and installs what a project
# needs. ca-certificates is the load-bearing one — without it every HTTPS call
# fails with an opaque TLS error. No libssl needed: openssl is vendored into the
# binaries (Cargo.toml, features = ["vendored"]), so they link only libc.
RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates curl git jq less procps xz-utils tini \
 && rm -rf /var/lib/apt/lists/*

# Non-root. Root in the container is root on the host if anything escapes, and
# this product runs arbitrary shell by design. The uid is fixed so a bind-mounted
# volume's ownership is predictable from outside.
RUN useradd --uid 10001 --create-home --home-dir /data --shell /bin/bash cp

COPY --from=bundle /tmp/bundle.tar.gz /tmp/bundle.tar.gz
RUN mkdir -p /opt/cp \
 && tar -xzf /tmp/bundle.tar.gz -C /opt/cp \
 && rm /tmp/bundle.tar.gz \
 && chmod 755 /opt/cp/cpilot /opt/cp/cp-console-server /opt/cp/cp-orchestrator /opt/cp/meilisearch

ENV HOME=/data \
    CP_AUTH_ENABLED=1 \
    CP_ORCH_PORT=7878 \
    CP_WEB_ROOT=/opt/cp/web \
    CP_AGENT_BINARY=/opt/cp/cpilot \
    CP_AGENTS_ROOT=/data/code
# 0.0.0.0, overriding the binary's 127.0.0.1 default. That default protects the
# appliance, where Caddy is the only LAN-facing surface and a backend on the LAN
# would bypass it. In a container the isolation boundary is the network
# namespace and the published ports, not the bind address: bound to loopback the
# orchestrator would be unreachable through `-p` (which forwards to the
# container's bridge IP) while an in-container healthcheck still passed — health
# green, cockpit refusing connections. Decide exposure with `-p 127.0.0.1:…`.
ENV CP_ORCH_BIND=0.0.0.0
# CP_CADDYFILE / CP_CADDY_BIN are deliberately UNSET: undefined, the
# orchestrator's Caddy integration exits cleanly instead of writing and
# reloading a Caddyfile of its own (transport/it/caddy/mod.rs:306), which is what
# lets an external reverse proxy front this container.
#
# The network-applier gates (CP_NMCLI_BIN, CP_IW_BIN, CP_NETWORKD_DIR, …) are
# unset for the same reason and must STAY unset in every profile: they are what
# switches the backend from "persist the network document" to "reconfigure this
# machine". A container has no business rewriting its host's network, and with
# them absent the applier is inert.

COPY entrypoint.sh /usr/local/bin/cp-entrypoint
RUN chmod 755 /usr/local/bin/cp-entrypoint

USER cp
WORKDIR /data
EXPOSE 7878

# /api/health is unauthenticated (deploy/ansible/tasks/start.yml probes it the
# same way). start-period is generous: a cold volume seeds Meilisearch first.
HEALTHCHECK --interval=30s --timeout=5s --start-period=90s --retries=3 \
  CMD curl -fsS -o /dev/null http://127.0.0.1:7878/api/health || exit 1

# tini reaps what the orchestrator spawns — agents, Meilisearch, console servers.
# Without a real pid 1 they accumulate as zombies against --pids-limit.
ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/cp-entrypoint"]
