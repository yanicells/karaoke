#!/usr/bin/env bash
# Build KaraokeMic, install it to ~/Applications, and register it with LaunchServices
# so Spotlight opens the current app bundle instead of an old Xcode DerivedData copy.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_NAME="KaraokeMic"
PROJECT="${ROOT_DIR}/${APP_NAME}.xcodeproj"
SCHEME="${APP_NAME}"
BUILD_DIR="${ROOT_DIR}/.build/xcode"
BUILT_APP="${BUILD_DIR}/Build/Products/Debug/${APP_NAME}.app"
INSTALL_DIR="${HOME}/Applications"
INSTALL_APP="${INSTALL_DIR}/${APP_NAME}.app"
RESET_MIC_PERMISSION=0
LAUNCH_AFTER_INSTALL=1

usage() {
  cat <<USAGE
Usage: $(basename "$0") [--no-launch] [--reset-mic-permission]

Builds ${APP_NAME}, installs it to:
  ${INSTALL_APP}

Options:
  --no-launch              Install and register the app without launching it.
  --reset-mic-permission   Reset macOS microphone permission for the app bundle ID
                           so the next launch asks again.
USAGE
}

for arg in "$@"; do
  case "$arg" in
    --no-launch) LAUNCH_AFTER_INSTALL=0 ;;
    --reset-mic-permission) RESET_MIC_PERMISSION=1 ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $arg" >&2
      usage >&2
      exit 1
      ;;
  esac
done

if [[ ! -d "$PROJECT" ]]; then
  if command -v xcodegen >/dev/null 2>&1; then
    (cd "$ROOT_DIR" && xcodegen generate)
  else
    echo "Missing ${PROJECT}. Install XcodeGen or run 'xcodegen generate' first." >&2
    exit 1
  fi
fi

echo "==> Building ${APP_NAME}"
xcodebuild \
  -project "$PROJECT" \
  -scheme "$SCHEME" \
  -configuration Debug \
  -derivedDataPath "$BUILD_DIR" \
  build

if [[ ! -d "$BUILT_APP" ]]; then
  echo "Build succeeded, but app bundle was not found at ${BUILT_APP}" >&2
  exit 1
fi

echo "==> Installing to ${INSTALL_APP}"
mkdir -p "$INSTALL_DIR"
/usr/bin/ditto "$BUILT_APP" "$INSTALL_APP"
xattr -cr "$INSTALL_APP"

echo "==> Registering with LaunchServices"
/System/Library/Frameworks/CoreServices.framework/Versions/Current/Frameworks/LaunchServices.framework/Versions/Current/Support/lsregister \
  -f -R -trusted "$INSTALL_APP"
mdimport "$INSTALL_APP" >/dev/null 2>&1 || true

if [[ "$RESET_MIC_PERMISSION" == "1" ]]; then
  echo "==> Resetting microphone permission for com.karaokemic.KaraokeMic"
  tccutil reset Microphone com.karaokemic.KaraokeMic >/dev/null 2>&1 || true
fi

if [[ "$LAUNCH_AFTER_INSTALL" == "1" ]]; then
    echo "==> Relaunching ${APP_NAME}"
    osascript -e "tell application \"${APP_NAME}\" to quit" >/dev/null 2>&1 || true
    for _ in {1..20}; do
      if ! pgrep -x "$APP_NAME" >/dev/null 2>&1; then
        break
      fi
      sleep 0.25
    done

    if ! open "$INSTALL_APP"; then
      echo "WARN: open failed; launching app binary directly." >&2
      "${INSTALL_APP}/Contents/MacOS/${APP_NAME}" >/dev/null 2>&1 &
      disown
    fi
fi

echo "Installed ${INSTALL_APP}"
