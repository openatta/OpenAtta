#!/usr/bin/env bash
# Manage the local attaos dev server.
#
# Usage:
#   ./scripts/dev-service.sh start     # start attaos in background
#   ./scripts/dev-service.sh stop      # stop attaos
#   ./scripts/dev-service.sh restart   # stop + start
#   ./scripts/dev-service.sh status    # show running status
#   ./scripts/dev-service.sh shell     # stop server + launch attash (Tauri dev)
set -euo pipefail
source "$(dirname "$0")/_env.sh"

BINARY="$PROJECT_ROOT/target/debug/attaos"
PID_FILE="$ATTA_HOME/run/attaos.pid"

do_stop() {
    [ -f "$PID_FILE" ] || { info "Not running."; return 0; }
    local pid; pid=$(cat "$PID_FILE")
    if kill -0 "$pid" 2>/dev/null; then
        info "Stopping attaos (PID $pid)..."
        kill "$pid" 2>/dev/null || true
        for _ in $(seq 1 10); do
            kill -0 "$pid" 2>/dev/null || break
            sleep 0.5
        done
        if kill -0 "$pid" 2>/dev/null; then
            warn "Force killing..."
            kill -9 "$pid" 2>/dev/null || true
        fi
        ok "Stopped."
    else
        info "Not running (stale PID file)."
    fi
    rm -f "$PID_FILE"
}

do_start() {
    if [ ! -f "$BINARY" ]; then
        err "attaos not found at $BINARY"
        err "Run ./scripts/build.sh first."
        exit 1
    fi

    # Already running?
    if [ -f "$PID_FILE" ]; then
        local pid; pid=$(cat "$PID_FILE")
        if kill -0 "$pid" 2>/dev/null; then
            warn "Already running (PID $pid)."
            return 0
        fi
        rm -f "$PID_FILE"
    fi

    mkdir -p "$ATTA_HOME/log" "$ATTA_HOME/run"

    info "Starting attaos on port $ATTA_PORT..."
    RUST_LOG="${RUST_LOG:-info}" \
    nohup "$BINARY" --home "$ATTA_HOME" --port "$ATTA_PORT" \
        > "$ATTA_HOME/log/attaos.log" 2>&1 &
    echo $! > "$PID_FILE"

    sleep 1
    if kill -0 "$(cat "$PID_FILE")" 2>/dev/null; then
        ok "Running (PID $(cat "$PID_FILE")) — http://localhost:$ATTA_PORT"
    else
        err "Failed to start. Check $ATTA_HOME/log/attaos.log"
        rm -f "$PID_FILE"
        exit 1
    fi
}

do_status() {
    if [ -f "$PID_FILE" ]; then
        local pid; pid=$(cat "$PID_FILE")
        if kill -0 "$pid" 2>/dev/null; then
            ok "Running (PID $pid) — http://localhost:$ATTA_PORT"
            return 0
        fi
        warn "Not running (stale PID file)."
        return 1
    fi
    info "Not running."
    return 1
}

do_shell() {
    do_stop
    info "Launching attash (Tauri dev mode)..."
    info "  ATTA_HOME=$ATTA_HOME  ATTA_PORT=$ATTA_PORT"
    export ATTA_HOME ATTA_PORT
    (cd "$PROJECT_ROOT/apps/shell" && npm run tauri dev)
}

case "${1:-}" in
    start)   do_start ;;
    stop)    do_stop ;;
    restart) do_stop; do_start ;;
    status)  do_status ;;
    shell)   do_shell ;;
    *)
        echo "Usage: $0 {start|stop|restart|status|shell}"
        exit 1
        ;;
esac
