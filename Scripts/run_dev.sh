#!/usr/bin/env bash
# Launch a debug bundle with the same microphone permission identity as release.
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
exec "$ROOT_DIR/Scripts/compile_and_run.sh" --debug "$@"
