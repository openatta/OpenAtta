#!/usr/bin/env bash
# AttaOS Install Script
#
# Creates the $ATTA_HOME directory structure and optionally copies
# built-in assets (webui, skills, flows) into place.
#
# Usage:
#   ./scripts/install.sh                # uses default ~/.atta
#   ATTA_HOME=/opt/atta ./scripts/install.sh

set -euo pipefail

ATTA_HOME="${ATTA_HOME:-$HOME/.atta}"

echo "==> Installing AttaOS"
echo "    ATTA_HOME = $ATTA_HOME"

# Create directory structure
dirs=(
    "$ATTA_HOME/etc"
    "$ATTA_HOME/data"
    "$ATTA_HOME/log"
    "$ATTA_HOME/cache"
    "$ATTA_HOME/run"
    "$ATTA_HOME/lib/webui"
    "$ATTA_HOME/lib/skills"
    "$ATTA_HOME/lib/flows"
    "$ATTA_HOME/lib/tools"
    "$ATTA_HOME/exts/skills"
    "$ATTA_HOME/exts/flows"
    "$ATTA_HOME/exts/tools"
    "$ATTA_HOME/exts/mcp"
)

for dir in "${dirs[@]}"; do
    mkdir -p "$dir"
done
echo "    Created directory structure"

# Find script directory (project root)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Copy built-in skills
if [ -d "$PROJECT_ROOT/skills" ]; then
    cp -r "$PROJECT_ROOT/skills/"* "$ATTA_HOME/lib/skills/" 2>/dev/null || true
    echo "    Copied built-in skills to $ATTA_HOME/lib/skills/"
fi

# Copy built-in flows
if [ -d "$PROJECT_ROOT/flows" ]; then
    cp -r "$PROJECT_ROOT/flows/"* "$ATTA_HOME/lib/flows/" 2>/dev/null || true
    echo "    Copied built-in flows to $ATTA_HOME/lib/flows/"
fi

# Copy WebUI dist if available
if [ -d "$PROJECT_ROOT/webui/dist" ]; then
    cp -r "$PROJECT_ROOT/webui/dist/"* "$ATTA_HOME/lib/webui/" 2>/dev/null || true
    echo "    Copied WebUI to $ATTA_HOME/lib/webui/"
else
    echo "    WebUI not built (run 'cd webui && npm run build' first)"
fi

# Copy binary if built
BINARY=""
for candidate in \
    "$PROJECT_ROOT/target/release/attaos" \
    "$PROJECT_ROOT/target/debug/attaos"; do
    if [ -f "$candidate" ]; then
        BINARY="$candidate"
        break
    fi
done

if [ -n "$BINARY" ]; then
    INSTALL_DIR="/usr/local/bin"
    if [ -w "$INSTALL_DIR" ]; then
        cp "$BINARY" "$INSTALL_DIR/attaos"
        echo "    Installed attaos to $INSTALL_DIR/attaos"
    else
        echo "    Binary found at $BINARY"
        echo "    Run: sudo cp $BINARY $INSTALL_DIR/attaos"
    fi
else
    echo "    Binary not built (run 'cargo build -p atta-server --release' first)"
fi

echo ""
echo "==> Installation complete!"
echo "    Start with: attaos --home $ATTA_HOME"
