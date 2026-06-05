#!/bin/bash
# Supervisor script for TUI - handles reload requests

CONFIG_FILE=".context-pilot/config.json"

# Raise FD limit — macOS defaults to 256 which is too low for kqueue
# (1 FD per watched file/dir). A session with 60+ tree folders open
# plus .git/ watches easily exceeds the default.
ulimit -n 2048 2>/dev/null

# Load environment variables from .env
if [ -f .env ]; then
    export $(grep -v '^#' .env | xargs)
fi

# Parse --telemetry flag: enable flame graph instrumentation
ARGS=()
for arg in "$@"; do
    if [ "$arg" = "--telemetry" ]; then
        export CP_FLAMEGRAPH=1
        echo "🔥 Telemetry mode: flame graph instrumentation enabled"
    else
        ARGS+=("$arg")
    fi
done

# Prefer system OpenSSL over vendored build (avoids Perl dependency maze)
if pkg-config --exists openssl 2>/dev/null; then
    export OPENSSL_NO_VENDOR=1
fi

# Build both binaries (TUI + console server) upfront
cargo build --release -p cp-console-server
cargo build --release

while true; do
    # Run the TUI (CP_FLAMEGRAPH persists across reloads via env)
    cargo run --release -- "${ARGS[@]}"

    # Check if reload was requested
    if [ -f "$CONFIG_FILE" ]; then
        RELOAD=$(grep -E '"reload_requested":\s*true' "$CONFIG_FILE" 2>/dev/null)
        if [ -n "$RELOAD" ]; then
            echo "Reload requested, restarting..."
            # Small delay to ensure file is fully written
            sleep 0.2
            # Add --resume-stream if not already present
            if [[ ! " ${ARGS[*]} " =~ " --resume-stream " ]]; then
                ARGS+=("--resume-stream")
            fi
            continue
        fi
    fi

    # No reload requested, exit
    break
done
