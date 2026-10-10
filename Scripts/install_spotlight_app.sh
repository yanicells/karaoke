#!/usr/bin/env bash
# Install a validated replacement before registering its stable Spotlight identity.
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INSTALL_DIR="$HOME/Applications"
INSTALL_APP="$INSTALL_DIR/KaraokeMic.app"
RESET_MIC_PERMISSION=0
LAUNCH_AFTER_INSTALL=1
usage() {
  cat <<USAGE
Usage: $(basename "$0") [--no-launch] [--reset-mic-permission]

Builds a release bundle incrementally and installs it to $INSTALL_APP.
  --no-launch              Install and register without launching or quitting the app.
  --reset-mic-permission   Reset this app's microphone permission so it asks again.
USAGE
}
for arg in "$@"; do
  case "$arg" in
    --no-launch) LAUNCH_AFTER_INSTALL=0 ;;
    --reset-mic-permission) RESET_MIC_PERMISSION=1 ;;
    --help|-h) usage; exit 0 ;;
    *) echo "Unknown argument: $arg" >&2; usage >&2; exit 1 ;;
  esac
done
"$ROOT_DIR/Scripts/package_app.sh" release
mkdir -p "$INSTALL_DIR"
STAGING_DIR="$(mktemp -d "$INSTALL_DIR/.karaokemic-install.XXXXXX")"
cleanup() {
  if [[ -d "$STAGING_DIR/previous.app" && ! -e "$INSTALL_APP" ]]; then
    if ! mv "$STAGING_DIR/previous.app" "$INSTALL_APP"; then
      echo "Could not restore the previous app; preserved at $STAGING_DIR/previous.app" >&2
      return 1
    fi
  fi
  rm -rf "$STAGING_DIR"
}
trap cleanup EXIT
STAGED_APP="$STAGING_DIR/KaraokeMic.app"
/usr/bin/ditto "$ROOT_DIR/dist/KaraokeMic.app" "$STAGED_APP"
"$ROOT_DIR/Scripts/validate_bundle.sh" "$STAGED_APP"
if [[ "$LAUNCH_AFTER_INSTALL" == 1 ]]; then
  osascript -e 'tell application id "com.karaokemic.KaraokeMic" to quit' >/dev/null 2>&1 || true
  # Opening while the old instance is still quitting fails with LaunchServices error -600.
  for _ in {1..50}; do
    pgrep -xq KaraokeMic || break
    sleep 0.1
  done
fi
[[ ! -e "$INSTALL_APP" ]] || mv "$INSTALL_APP" "$STAGING_DIR/previous.app"
mv "$STAGED_APP" "$INSTALL_APP"
/System/Library/Frameworks/CoreServices.framework/Versions/Current/Frameworks/LaunchServices.framework/Versions/Current/Support/lsregister \
  -f -R -trusted "$INSTALL_APP"
mdimport "$INSTALL_APP" >/dev/null 2>&1 || true
if [[ "$RESET_MIC_PERMISSION" == 1 ]]; then
  tccutil reset Microphone com.karaokemic.KaraokeMic
fi
if [[ "$LAUNCH_AFTER_INSTALL" == 1 ]]; then
  open "$INSTALL_APP"
fi
echo "Installed $INSTALL_APP"
