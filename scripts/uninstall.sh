#!/usr/bin/env bash
# AttaOS Uninstall Script
#
# Stops the running server and optionally removes $ATTA_HOME.
#
# Usage:
#   ./scripts/uninstall.sh                # uses default ~/.atta
#   ATTA_HOME=/opt/atta ./scripts/uninstall.sh

set -euo pipefail

ATTA_HOME="${ATTA_HOME:-$HOME/.atta}"

echo "==> Uninstalling AttaOS"
echo "    ATTA_HOME = $ATTA_HOME"

# Stop running server via PID file
PID_FILE="$ATTA_HOME/run/attaos.pid"
if [ -f "$PID_FILE" ]; then
    PID=$(cat "$PID_FILE")
    if kill -0 "$PID" 2>/dev/null; then
        echo "    Stopping attaos (PID $PID)..."
        kill "$PID"
        sleep 2
        # Force kill if still running
        if kill -0 "$PID" 2>/dev/null; then
            kill -9 "$PID" 2>/dev/null || true
        fi
        echo "    Server stopped"
    else
        echo "    PID $PID not running"
    fi
    rm -f "$PID_FILE"
fi

# Remove binary
for bin_path in /usr/local/bin/attaos /usr/local/bin/attacli; do
    if [ -f "$bin_path" ]; then
        if [ -w "$bin_path" ]; then
            rm "$bin_path"
            echo "    Removed $bin_path"
        else
            echo "    Found $bin_path — run: sudo rm $bin_path"
        fi
    fi
done

# Ask about ATTA_HOME removal
if [ -d "$ATTA_HOME" ]; then
    echo ""
    echo "    $ATTA_HOME contains your data (database, configs, skills)."
    read -rp "    Remove $ATTA_HOME? [y/N] " answer
    case "$answer" in
        [yY]|[yY][eE][sS])
            rm -rf "$ATTA_HOME"
            echo "    Removed $ATTA_HOME"
            ;;
        *)
            echo "    Kept $ATTA_HOME"
            ;;
    esac
fi

echo ""
echo "==> Uninstall complete"
