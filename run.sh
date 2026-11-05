#!/usr/bin/env bash
# Non-Windows counterpart of run.bat.
# Usage: ./run.sh [action] [extra arguments passed to the binary]
# Set JACKETT_PROFILE=release to use target/release.
set -euo pipefail
cd "$(dirname "$0")"

if [[ "${JACKETT_PROFILE:-}" == release ]]; then
  BIN="$PWD/target/release"
  PROFILE=(--release)
else
  BIN="$PWD/target/debug"
  PROFILE=()
fi

action="${1:-server}"
[[ $# -gt 0 ]] && shift
args=("$@")

ensure() {
  # Always do an incremental build from a checkout so a source change is
  # picked up instead of silently starting a stale binary.
  if [[ -f Cargo.toml ]] && command -v cargo >/dev/null 2>&1; then
    log="$(mktemp)"
    if ! cargo build --workspace "${PROFILE[@]}" >"$log" 2>&1; then
      cat "$log" >&2
      rm -f "$log"
      exit 1
    fi
    rm -f "$log"
  fi
  if [[ ! -x "$BIN/$1" ]]; then
    echo "$1 not found. Run ./run.sh build first." >&2
    exit 1
  fi
}

case "$action" in
  help|-h|--help)
    cat <<'EOF'
Usage: ./run.sh [action] [extra arguments]
  server (default)  run jackett-server in foreground
  service           run jackett-service (use --service for the SCM host)
  install/uninstall write/remove the systemd user unit
  start/stop/status manage the installed service
  tray              start jackett-tray (headless loop off Windows)
  updater           run jackett-updater
  check             check GitHub for the latest release
  build/test        cargo build/test --workspace
EOF
    ;;
  build) cargo build --workspace "${PROFILE[@]}" "${args[@]}" ;;
  test)  cargo test --workspace "${PROFILE[@]}" "${args[@]}" ;;
  server|run)
    ensure jackett-server
    echo "Starting jackett-server (Ctrl+C to stop)..." >&2
    exec "$BIN/jackett-server" "${args[@]}"
    ;;
  service)  ensure jackett-service; exec "$BIN/jackett-service" "${args[@]}" ;;
  install)  ensure jackett-service; exec "$BIN/jackett-service" --install "${args[@]}" ;;
  uninstall) ensure jackett-service; exec "$BIN/jackett-service" --uninstall "${args[@]}" ;;
  start)    ensure jackett-service; exec "$BIN/jackett-service" --start "${args[@]}" ;;
  stop)     ensure jackett-service; exec "$BIN/jackett-service" --stop "${args[@]}" ;;
  status)   ensure jackett-service; exec "$BIN/jackett-service" --status ;;
  tray)     ensure jackett-tray; exec "$BIN/jackett-tray" "${args[@]}" ;;
  updater|update) ensure jackett-updater; exec "$BIN/jackett-updater" "${args[@]}" ;;
  check)    ensure jackett-updater; exec "$BIN/jackett-updater" --check "${args[@]}" ;;
  *) echo "Unknown action: $action (try ./run.sh help)" >&2; exit 2 ;;
esac
