#!/usr/bin/env bash
# Package first, then relaunch the validated app through LaunchServices.
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONFIGURATION=release
RUN_TESTS=0
RELEASE_ARCHES=""
RELEASE_ARCHES_SET=0
DEBUG_SET=0
while [[ $# -gt 0 ]]; do
  arg="$1"
  shift
  case "$arg" in
    --debug) CONFIGURATION=debug; DEBUG_SET=1 ;;
    --test|-t) RUN_TESTS=1 ;;
    --release-universal)
      RELEASE_ARCHES="arm64 x86_64"
      RELEASE_ARCHES_SET=1
      ;;
    --release-arches=*)
      RELEASE_ARCHES="${arg#*=}"
      RELEASE_ARCHES_SET=1
      ;;
    --release-arches)
      [[ $# -gt 0 ]] || { echo "--release-arches requires a non-empty value." >&2; exit 1; }
      RELEASE_ARCHES="$1"
      shift
      RELEASE_ARCHES_SET=1
      ;;
    --help|-h)
      echo "Usage: $(basename "$0") [--debug] [--test] [--release-universal | --release-arches='arm64 x86_64']"
      exit 0
      ;;
    *) echo "Unknown argument: $arg" >&2; exit 1 ;;
  esac
done
if [[ "$RELEASE_ARCHES_SET" == 1 ]]; then
  [[ -n "$RELEASE_ARCHES" && "$RELEASE_ARCHES" =~ [^[:space:]] ]] || {
    echo "--release-arches must not be empty." >&2
    exit 1
  }
  ARCHES_INPUT="${RELEASE_ARCHES//$'\n'/ }"
  read -r -a ARCH_LIST <<< "$ARCHES_INPUT"
  [[ ${#ARCH_LIST[@]} -gt 0 ]] || { echo "--release-arches must include arm64 and/or x86_64." >&2; exit 1; }
  for ARCH in "${ARCH_LIST[@]}"; do
    case "$ARCH" in
      arm64|x86_64) ;;
      *) echo "Unsupported architecture '$ARCH'; use arm64 and/or x86_64." >&2; exit 1 ;;
    esac
    COUNT=0
    for CHECK_ARCH in "${ARCH_LIST[@]}"; do
      [[ "$CHECK_ARCH" != "$ARCH" ]] || ((COUNT += 1))
    done
    [[ "$COUNT" -eq 1 ]] || { echo "Duplicate architecture '$ARCH' in --release-arches." >&2; exit 1; }
  done
  [[ "$DEBUG_SET" == 0 ]] || {
    echo "--release-universal and --release-arches cannot be used with --debug." >&2
    exit 1
  }
fi
cd "$ROOT_DIR"
if [[ "$RUN_TESTS" == 1 ]]; then
  cargo test --locked --package karaoke-mic
fi
if [[ "$RELEASE_ARCHES_SET" == 1 ]]; then
  ARCHES="$RELEASE_ARCHES" "$ROOT_DIR/Scripts/package_app.sh" "$CONFIGURATION"
else
  "$ROOT_DIR/Scripts/package_app.sh" "$CONFIGURATION"
fi
osascript -e 'tell application id "com.karaokemic.KaraokeMic" to quit' >/dev/null 2>&1 || true
# Opening while the old instance is still quitting fails with LaunchServices error -600.
for _ in {1..50}; do
  pgrep -xq KaraokeMic || break
  sleep 0.1
done
open "$ROOT_DIR/dist/KaraokeMic.app"
echo "Opened $ROOT_DIR/dist/KaraokeMic.app"
