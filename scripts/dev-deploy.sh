#!/usr/bin/env bash
# Build + deploy to ~/.atta for local development testing.
#
# Usage:
#   ./scripts/dev-deploy.sh              # build all + deploy + restart
#   ./scripts/dev-deploy.sh --skip-build # deploy only (no build)
#   ./scripts/dev-deploy.sh server       # rebuild server only + deploy + restart
#   ./scripts/dev-deploy.sh webui        # rebuild webui only + deploy + restart
set -euo pipefail
source "$(dirname "$0")/_env.sh"

SKIP_BUILD=0
COMPONENT="all"
SCRIPT_DIR="$(dirname "$0")"

for arg in "$@"; do
    case "$arg" in
        --skip-build) SKIP_BUILD=1 ;;
        server|cli|webui|shell) COMPONENT="$arg" ;;
    esac
done

# ── Build ──
if [ "$SKIP_BUILD" = "0" ]; then
    "$SCRIPT_DIR/build.sh" "$COMPONENT"
fi

# ── Stop → Deploy → Start ──
"$SCRIPT_DIR/dev-service.sh" stop
deploy_artifacts
"$SCRIPT_DIR/dev-service.sh" start
