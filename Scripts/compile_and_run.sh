#!/usr/bin/env bash
# Package first, then relaunch the validated app through LaunchServices.
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONFIGURATION=release
RUN_TESTS=0
for arg in "$@"; do
  case "$arg" in
    --debug) CONFIGURATION=debug ;;
    --test|-t) RUN_TESTS=1 ;;
    --help|-h) echo "Usage: $(basename "$0") [--debug] [--test]"; exit 0 ;;
    *) echo "Unknown argument: $arg" >&2; exit 1 ;;
  esac
done
cd "$ROOT_DIR"
if [[ "$RUN_TESTS" == 1 ]]; then
  cargo test --locked --package karaoke-mic
fi
"$ROOT_DIR/Scripts/package_app.sh" "$CONFIGURATION"
osascript -e 'tell application id "com.karaokemic.KaraokeMic" to quit' >/dev/null 2>&1 || true
open "$ROOT_DIR/dist/KaraokeMic.app"
echo "Opened $ROOT_DIR/dist/KaraokeMic.app"
