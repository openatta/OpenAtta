#!/usr/bin/env bash
# Full build: server + cli + webui + shell (Tauri).
#
# Usage:
#   ./scripts/build.sh              # build all
#   ./scripts/build.sh server       # attaos only
#   ./scripts/build.sh cli          # attacli only
#   ./scripts/build.sh webui        # webui only
#   ./scripts/build.sh shell        # attash (Tauri) only
set -euo pipefail
source "$(dirname "$0")/_env.sh"

COMPONENT="${1:-all}"

build_server() {
    info "Building attaos..."
    cargo build -p atta-server --features desktop
    ok "attaos  -> target/debug/attaos"
}

build_cli() {
    info "Building attacli..."
    cargo build -p atta-cli
    ok "attacli -> target/debug/attacli"
}

build_webui() {
    info "Building WebUI..."
    (cd "$PROJECT_ROOT/webui" && npm run build)
    ok "WebUI   -> webui/dist/"
}

build_shell() {
    info "Building attash (Tauri)..."
    (cd "$PROJECT_ROOT/apps/shell" && npm run tauri build -- --debug)
    ok "attash built (debug)"
}

case "$COMPONENT" in
    all)
        build_server
        build_cli
        build_webui
        ;;
    server)  build_server ;;
    cli)     build_cli ;;
    webui)   build_webui ;;
    shell)   build_shell ;;
    *)
        err "Unknown: $COMPONENT"
        echo "Usage: $0 [all|server|cli|webui|shell]"
        exit 1
        ;;
esac
