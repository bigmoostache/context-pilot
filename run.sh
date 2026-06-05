#!/bin/bash
# Supervisor script for TUI — headless daemon/client architecture.
#
# Builds both binaries, launches in headless mode (daemon + auto-attach),
# and handles rebuild-on-reload. The daemon runs as a background process;
# the client runs as the foreground process. On reload the daemon exits,
# the client exits (CP_RUN_SH=1 skips reconnection), and this script
# rebuilds from source before relaunching.

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
RESUME_STREAM=false
for arg in "$@"; do
    if [ "$arg" = "--telemetry" ]; then
        export CP_FLAMEGRAPH=1
        echo "🔥 Telemetry mode: flame graph instrumentation enabled"
    elif [ "$arg" = "--resume-stream" ]; then
        RESUME_STREAM=true
    else
        ARGS+=("$arg")
    fi
done

# Prefer system OpenSSL over vendored build (avoids Perl dependency maze)
if pkg-config --exists openssl 2>/dev/null; then
    export OPENSSL_NO_VENDOR=1
fi

export CP_RUN_SH=1

# Clean up daemon on script exit (normal exit, Ctrl+C, or signal)
cleanup() {
    ./target/release/tui --stop 2>/dev/null
}
trap cleanup EXIT

while true; do
    # Build both binaries (console server + TUI)
    cargo build --release -p cp-console-server
    cargo build --release

    # Assemble launch args: --headless + any user flags + optional --resume-stream
    LAUNCH_ARGS=("--headless" "${ARGS[@]}")
    if [ "$RESUME_STREAM" = true ]; then
        LAUNCH_ARGS+=("--resume-stream")
    fi

    # Launch in headless mode:
    # - spawns daemon (--daemon-internal) as background process
    # - auto-attaches as foreground client
    # CP_FLAMEGRAPH persists across reloads via env
    ./target/release/tui "${LAUNCH_ARGS[@]}"

    # Check if reload was requested
    if [ -f "$CONFIG_FILE" ]; then
        RELOAD=$(grep -E '"reload_requested":\s*true' "$CONFIG_FILE" 2>/dev/null)
        if [ -n "$RELOAD" ]; then
            echo "Reload requested, rebuilding..."
            # Ensure old daemon is stopped before rebuild
            ./target/release/tui --stop 2>/dev/null
            sleep 0.2
            RESUME_STREAM=true
            continue
        fi
    fi

    # No reload requested, exit
    break
done
