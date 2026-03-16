#!/usr/bin/env bash
# Shared environment and helpers. Source this — do not execute directly.

export PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export ATTA_HOME="${ATTA_HOME:-$HOME/.atta}"
export ATTA_PORT="${ATTA_PORT:-3000}"

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[0;33m'; CYAN='\033[0;36m'; NC='\033[0m'
info()  { echo -e "${CYAN}[atta]${NC} $*"; }
ok()    { echo -e "${GREEN}[atta]${NC} $*"; }
warn()  { echo -e "${YELLOW}[atta]${NC} $*"; }
err()   { echo -e "${RED}[atta]${NC} $*" >&2; }

# Ensure ~/.atta directory structure exists
ensure_home() {
    local dirs=(
        "$ATTA_HOME/bin"  "$ATTA_HOME/etc"   "$ATTA_HOME/data"
        "$ATTA_HOME/log"  "$ATTA_HOME/cache"  "$ATTA_HOME/run"
        "$ATTA_HOME/lib/webui"  "$ATTA_HOME/lib/skills"
        "$ATTA_HOME/lib/flows"  "$ATTA_HOME/lib/tools"
        "$ATTA_HOME/exts/skills" "$ATTA_HOME/exts/flows"
        "$ATTA_HOME/exts/tools"  "$ATTA_HOME/exts/mcp"
    )
    for dir in "${dirs[@]}"; do mkdir -p "$dir"; done
}

# Deploy build artifacts to $ATTA_HOME
deploy_artifacts() {
    ensure_home

    # Symlink attaos binary
    rm -f "$ATTA_HOME/bin/attaos"
    ln -s "$PROJECT_ROOT/target/debug/attaos" "$ATTA_HOME/bin/attaos"

    # WebUI
    if [ -d "$PROJECT_ROOT/webui/dist" ]; then
        rm -rf "$ATTA_HOME/lib/webui/"*
        cp -r "$PROJECT_ROOT/webui/dist/"* "$ATTA_HOME/lib/webui/"
    fi

    # Skills & Flows
    [ -d "$PROJECT_ROOT/skills" ] && cp -r "$PROJECT_ROOT/skills/"* "$ATTA_HOME/lib/skills/" 2>/dev/null || true
    [ -d "$PROJECT_ROOT/flows" ] && cp -r "$PROJECT_ROOT/flows/"* "$ATTA_HOME/lib/flows/" 2>/dev/null || true

    # Ensure manifest exists (attash needs it to enter installed mode)
    if [ ! -f "$ATTA_HOME/.manifest.json" ]; then
        cat > "$ATTA_HOME/.manifest.json" << 'EOF'
{
  "version": "0.1.0-dev",
  "installed_at": "dev",
  "platform": "dev",
  "components": ["attaos", "webui", "skills", "flows"]
}
EOF
    fi

    ok "Deployed to $ATTA_HOME"
}
